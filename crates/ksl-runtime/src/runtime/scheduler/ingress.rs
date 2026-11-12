//! 协调器入口状态.
//!
//! 命令行参数, I/O 编号与未观察错误在此跨线程共享.
//! 跨线程状态变化经 `runtime::event` 的收件箱回到协调器.
//! Join 与定时器等语义表由协调器持有, 不在本层.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use crossbeam_channel::Sender;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_io::terminal::state::RawModeHolder;
use ksl_value::value::{ActorHandle, Value};
use parking_lot::Mutex;

use crate::runtime::event::{Event, TaskId};
use crate::runtime::gc::GcRegistry;

#[derive(Clone)]
pub(crate) struct SchedulerHandle {
    ingress: Arc<Mutex<Ingress>>,
    gc: Arc<GcRegistry>,
    /// 运行时拥有的 I/O 服务.
    io: Arc<ksl_io::io::IoRuntime>,
    /// 协调器收件箱的发送端; 跨线程完成都经此入队.
    events: Sender<Event>,
}

pub(super) struct Ingress {
    /// 全部任务共享的命令行参数; 启动 Actor 时读取.
    pub(super) args: Value,
    /// 未观察的 Actor 运行时错误; 由宿主统一取走并渲染.
    pub(super) actor_errors: Vec<(Arc<std::sync::atomic::AtomicBool>, Error)>,
}

pub(super) struct SpawnRequest {
    pub(super) handle: Arc<ActorHandle>,
    pub(super) handler: Value,
    pub(super) args: Vec<Value>,
}

impl SchedulerHandle {
    pub(super) fn new(
        gc: Arc<GcRegistry>,
        io: Arc<ksl_io::io::IoRuntime>,
        events: Sender<Event>,
    ) -> (Self, Arc<Mutex<Ingress>>) {
        let ingress = Arc::new(Mutex::new(Ingress {
            args: Value::list(Vec::new()),
            actor_errors: Vec::new(),
        }));
        (
            Self {
                ingress: Arc::clone(&ingress),
                gc,
                io,
                events,
            },
            ingress,
        )
    }

    /// 更新全部任务共享的命令行参数.
    pub(crate) fn set_args(&self, args: Value) {
        self.ingress.lock().args = args;
    }

    /// 调度器共享的环回收注册表.
    #[must_use]
    pub(crate) fn registry(&self) -> &Arc<GcRegistry> {
        &self.gc
    }

    /// 运行时拥有的 I/O 服务; 抽象机内联输出与当前调用使用.
    #[must_use]
    pub(crate) fn io(&self) -> &Arc<ksl_io::io::IoRuntime> {
        &self.io
    }

    /// 协调器收件箱的发送端; 抽象机内联唤醒使用.
    #[must_use]
    pub(crate) fn events_sender(&self) -> &Sender<Event> {
        &self.events
    }

    /// 释放某个终端持有者; 持有者终止时调用.
    pub(crate) fn release_terminal_holder(&self, holder: RawModeHolder) {
        self.io.release_terminal_holder(holder);
    }

    /// 把 I/O 任务提交到线程池或专用输入线程.
    ///
    /// `task` 是等待该操作的任务; `owner` 是终端原始模式的持有者,
    /// Prelude 与线程池调用为 None.
    pub(crate) fn submit_io(
        &self,
        job: ksl_io::io::Task,
        task: TaskId,
        owner: Option<u64>,
        root: Option<u64>,
    ) -> u64 {
        let id = self.io.allocate_op_id();
        let holder = RawModeHolder::from_owner(owner, root);
        let events = self.events.clone();
        self.io.submit_task(job, id, holder, move |result| {
            let _ = events.send(Event::Io {
                task,
                op_id: id,
                result,
            });
        });
        id
    }

    /// 请求取消一个 I/O 操作; 终端读取与网络操作会响应取消标记与通知.
    pub(crate) fn cancel_io(&self, id: u64) {
        self.io.cancel_task(id);
    }

    pub(crate) fn take_actor_errors(&self) -> Vec<Error> {
        let errors = std::mem::take(&mut self.ingress.lock().actor_errors);
        errors
            .into_iter()
            .filter_map(|(observed, error)| {
                if observed.load(Ordering::Relaxed) {
                    None
                } else {
                    Some(error)
                }
            })
            .collect()
    }

    /// 清空共享状态; 调度器关闭时调用.
    pub(crate) fn clear_runtime_state(&self) {
        self.ingress.lock().actor_errors.clear();
    }
}
