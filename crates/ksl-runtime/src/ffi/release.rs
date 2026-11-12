//! 调度线程释放队列.
//!
//! 声明 `THREAD_RUNTIME` 的句柄在任意线程上的最后一个引用消失时只投递任务,
//! 由调度线程执行实际释放入口. 已经在调度线程上时就地执行.

use parking_lot::Mutex;

use crate::runtime::event::Event;

/// 一次调度线程释放任务.
pub(crate) type ReleaseJob = Box<dyn FnOnce() + Send + 'static>;

/// 调度线程释放队列的投递端.
///
/// 任务经通道交给调度线程侧的 `ReleaseDrain`, 投递端本身不保存任务,
/// 因此工作线程持有最后一个投递端时不会在 worker 上执行释放.
pub(crate) struct ReleaseQueue {
    jobs: crossbeam_channel::Sender<ReleaseJob>,
    /// 调度线程标识与本运行时的收件箱; 未附着时只就地执行.
    route: Mutex<Option<(std::thread::ThreadId, crossbeam_channel::Sender<Event>)>>,
}

/// 调度线程侧的释放任务接收端.
///
/// 接收端只由调度器持有, 与可跨线程共享的投递端分离.
pub(crate) struct ReleaseDrain {
    receiver: crossbeam_channel::Receiver<ReleaseJob>,
}

impl ReleaseQueue {
    pub(crate) fn new() -> (Self, ReleaseDrain) {
        let (jobs, receiver) = crossbeam_channel::unbounded();
        (
            Self {
                jobs,
                route: Mutex::new(None),
            },
            ReleaseDrain { receiver },
        )
    }

    /// 没有调度线程接收端的投递端; 释放任务就地执行.
    pub(crate) fn detached() -> Self {
        let (queue, drain) = Self::new();
        drop(drain);
        queue
    }

    /// 附着调度线程与收件箱; 每个运行时只附着一次.
    pub(crate) fn attach(&self, events: crossbeam_channel::Sender<Event>) {
        *self.route.lock() = Some((std::thread::current().id(), events));
    }

    /// 投递一次释放任务.
    pub(crate) fn push(&self, job: ReleaseJob) {
        let current = std::thread::current().id();
        let route = self.route.lock();
        let Some((thread, events)) = route.as_ref() else {
            drop(route);
            job();
            return;
        };
        if *thread == current {
            drop(route);
            job();
            return;
        }
        let events = events.clone();
        drop(route);
        // 接收端已销毁说明运行时正在关闭, 就地执行是唯一选择.
        if let Err(error) = self.jobs.send(job) {
            (error.0)();
            return;
        }
        // 容量不限, 仅用于唤醒; 运行时已关闭时忽略失败.
        let _ = events.try_send(Event::Wake);
    }
}

impl ReleaseDrain {
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }

    /// 取走当前全部释放任务; 调用方在调度线程执行.
    pub(crate) fn drain(&self) -> Vec<ReleaseJob> {
        let mut jobs = Vec::new();
        while let Ok(job) = self.receiver.try_recv() {
            jobs.push(job);
        }
        jobs
    }
}

impl Drop for ReleaseDrain {
    fn drop(&mut self) {
        // 调度器先行析构时仍由所在线程执行剩余任务.
        for job in self.drain() {
            job();
        }
    }
}
