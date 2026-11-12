//! 固定大小工作线程池.
//!
//! 任务按提交顺序排队, 由固定线程并发执行.
//! 池内任务不携带语言值; 完成回调负责把结果交回调度器.
//! 取消只对尚未开始的任务生效, 已经开始的阻塞工作会自行结束.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::{Receiver, Sender, unbounded};
use ksl_frontend::error::RuntimeError as Error;

use super::{ComputeJob, Done};

/// 固定大小工作线程池.
pub(super) struct ThreadPool {
    jobs: Sender<Message>,
}

enum Message {
    Job {
        job: ComputeJob,
        cancel: Arc<AtomicBool>,
        done: Done,
    },
}

impl ThreadPool {
    /// 启动指定数量的工作线程; 池在通道断开且队列排空后退出.
    pub(super) fn start(workers: usize, name: &str) -> Self {
        let (jobs, receiver) = unbounded();
        let mut started = 0;
        for index in 0..workers.max(1) {
            let receiver = receiver.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("{name}-{index}"))
                .spawn(move || worker(&receiver));
            if spawned.is_ok() {
                started += 1;
            }
        }
        if started == 0 {
            // 没有任何工作线程时关闭通道, 提交方按池已停止处理.
            drop(receiver);
        }
        Self { jobs }
    }

    /// 提交一个任务; 池已关闭时以内部错误完成.
    pub(super) fn submit(&self, job: ComputeJob, cancel: Arc<AtomicBool>, done: Done) {
        let message = Message::Job { job, cancel, done };
        if let Err(error) = self.jobs.send(message)
            && let Message::Job { done, .. } = error.into_inner()
        {
            done(Err(Error::internal("工作线程池已停止")));
        }
    }
}

fn worker(receiver: &Receiver<Message>) {
    while let Ok(Message::Job { job, cancel, done }) = receiver.recv() {
        if cancel.load(Ordering::Relaxed) {
            continue;
        }
        let result = catch_unwind(AssertUnwindSafe(job)).unwrap_or_else(|payload| {
            Err(Error::internal(format!(
                "工作线程任务 panic: {}",
                crate::runtime::panic::message(payload.as_ref())
            )))
        });
        done(result);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    use crossbeam_channel::bounded;

    use super::ThreadPool;

    #[test]
    fn panic_completes_with_internal_error() {
        let pool = ThreadPool::start(1, "ksl-pool-test");
        let (results, received) = bounded(1);
        let job: crate::io::ComputeJob = Box::new(|| panic!("任务 panic"));
        pool.submit(
            job,
            Arc::new(AtomicBool::new(false)),
            Box::new(move |result| {
                let _ = results.send(result);
            }),
        );
        let result = received
            .recv_timeout(Duration::from_secs(5))
            .expect("panic 后仍应投递完成结果");
        assert!(result.is_err());
    }
}
