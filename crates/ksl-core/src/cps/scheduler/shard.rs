//! Actor 调度状态.
//!
//! 持有就绪队列, Actor 表, 定时器与等待表; 事件经共享事件队列回到调度循环.
//! 调度器在调用线程上协作推进, 不另起线程执行用户代码.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use parking_lot::Mutex;

use super::shared::{SchedulerHandle, ShardEvent, Shared, SpawnRequest};
use super::timer_heap::TimerHeap;
use crate::cps::exec::{Machine, RunOutcome, Wait};
use crate::cps::gc::Marker;
use crate::error::{ErrorKind, RuntimeError as Error};
use crate::source::Span;
use crate::value::{
    ActorHandle, ActorResult, ExitReason, Value, join_result_value, option_none, option_some,
};

struct ActorTask {
    machine: Machine,
    handle: Arc<ActorHandle>,
    wait: Option<Wait>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TimerKind {
    Sleep,
    Receive,
}

/// 调度器持有的全部 Actor 任务状态.
pub(super) struct Shard {
    handle: SchedulerHandle,
    shared: Arc<Mutex<Shared>>,
    /// I/O 完成与事件通知; 调度器空闲时在此等待.
    wake: Receiver<()>,
    actors: HashMap<u64, ActorTask>,
    ready: VecDeque<u64>,
    receive_waiters: HashMap<(u64, usize), u64>,
    io_waiters: HashMap<u64, u64>,
    /// 任务等待超时堆; 每个 Actor 至多一个活动定时器.
    timers: TimerHeap<u64, TimerKind>,
    /// 指令预算; 测试可动态调小.
    slice_budget: Arc<AtomicUsize>,
    /// `@Main` 根 Actor 的编号; 只在调度器线程上读写, 不进入共享状态.
    root_actor: Option<u64>,
    /// 根 Actor 的结局; 同上.
    root_result: Option<Result<Value, Error>>,
}

impl Shard {
    pub(super) fn new(
        handle: SchedulerHandle,
        shared: Arc<Mutex<Shared>>,
        wake: Receiver<()>,
        slice_budget: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            handle,
            shared,
            wake,
            actors: HashMap::new(),
            ready: VecDeque::new(),
            receive_waiters: HashMap::new(),
            io_waiters: HashMap::new(),
            timers: TimerHeap::new(),
            slice_budget,
            root_actor: None,
            root_result: None,
        }
    }

    pub(super) fn pop_ready(&mut self) -> Option<u64> {
        self.ready.pop_front()
    }

    pub(super) fn has_io_waiters(&self) -> bool {
        !self.io_waiters.is_empty()
    }

    pub(super) fn has_timers(&self) -> bool {
        !self.timers.is_empty()
    }

    /// 遍历调度状态中的运行时值; 环回收根标记用.
    pub(super) fn trace(&self, marker: &mut Marker) {
        for task in self.actors.values() {
            task.machine.trace(marker);
            marker.mark_actor(&task.handle);
            if let Some(wait) = &task.wait {
                wait.trace(marker);
            }
        }
        if let Some(Ok(value)) = &self.root_result {
            marker.mark_value(value);
        }
    }

    /// 处理事件队列中的全部待处理事件.
    pub(super) fn drain_events(&mut self) {
        // 空转路径免锁: 标记为假时事件队列必为空.
        if !self.handle.take_events_pending() {
            return;
        }
        let events = {
            let mut shared = self.shared.lock();
            std::mem::take(&mut shared.events)
        };
        for event in events {
            match event {
                ShardEvent::Spawn(request) => self.start_spawn(request),
                ShardEvent::Send { target, index } => self.wake_send(target, index),
                ShardEvent::Kill { target } => self.kill_actor(target),
                ShardEvent::Io { id, result } => self.deliver_io(id, result),
                ShardEvent::Join { actor, value } => self.resume_actor(actor, value),
            }
        }
    }

    /// 创建根 Actor 并登记启动请求; 根编号先于启动事件就绪, 避免调度循环
    /// 把根当普通 Actor 启动而丢失根结局.
    pub(super) fn spawn_root(
        &mut self,
        mailboxes: usize,
        handler: Value,
        args: Vec<Value>,
    ) -> Value {
        let handle = {
            let mut shared = self.shared.lock();
            let id = shared.alloc_id();
            let handle = Arc::new(ActorHandle::new(id, mailboxes, None));
            self.handle.registry().register_actor(&handle);
            shared.events.push(ShardEvent::Spawn(SpawnRequest {
                handle: Arc::clone(&handle),
                handler,
                args,
            }));
            handle
        };
        self.root_actor = Some(handle.id);
        self.root_result = None;
        self.handle.mark_events_pending();
        Value::Pid(handle)
    }

    pub(super) fn is_root(&self, id: u64) -> bool {
        self.root_actor == Some(id)
    }

    pub(super) fn set_root_result(&mut self, result: Result<Value, Error>) {
        self.root_result = Some(result);
    }

    pub(super) fn take_root_result(&mut self) -> Option<Result<Value, Error>> {
        self.root_result.take()
    }

    fn start_spawn(&mut self, request: super::shared::SpawnRequest) {
        let id = request.handle.id;
        // 父 Actor 可能在子 Actor 被调度前就已终止, 或子 Actor 已被 Kill;
        // 这两种情况下都不再启动子 Actor.
        let parent_finished = request
            .handle
            .parent
            .as_ref()
            .is_some_and(|parent| parent.is_finished());
        if request.handle.kill_requested() || parent_finished {
            self.finish_pending(&request.handle, ExitReason::Killed);
            return;
        }
        let cli_args = self.shared.lock().args.clone();
        match Machine::start_actor(
            request.handler,
            Arc::clone(&request.handle),
            request.args,
            cli_args,
            self.handle.clone(),
        ) {
            Ok(machine) => {
                self.actors.insert(
                    id,
                    ActorTask {
                        machine,
                        handle: request.handle,
                        wait: None,
                    },
                );
                self.ready.push_back(id);
            },
            Err(error) => {
                self.shared
                    .lock()
                    .actor_errors
                    .push((request.handle.observation_flag(), error.clone()));
                self.finish_pending(&request.handle, error_to_exit(&error));
            },
        }
    }

    fn deliver_io(&mut self, op_id: u64, result: Result<Value, Error>) {
        let Some(actor) = self.io_waiters.remove(&op_id) else {
            return;
        };
        match result {
            Ok(value) => self.resume_actor(actor, value),
            Err(error) => self.fail_actor(actor, error),
        }
    }

    /// 推进一个 Actor; 返回后任务要么重新入队, 要么已终止或挂起.
    pub(super) fn actor_slice(&mut self, id: u64) {
        let Some(mut task) = self.actors.remove(&id) else {
            return;
        };
        let is_root = self.is_root(id);
        if task.handle.kill_requested() {
            if is_root {
                self.set_root_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
            }
            self.finish_actor(&task, &Err(ExitReason::Killed));
            return;
        }
        let budget = self.slice_budget.load(Ordering::Relaxed);
        match task.machine.run_budget(budget) {
            Ok(RunOutcome::Finished(value)) => {
                if is_root {
                    self.set_root_result(Ok(value.clone()));
                }
                self.finish_actor(&task, &Ok(value));
            },
            Ok(RunOutcome::Yield) => {
                if task.handle.kill_requested() {
                    if is_root {
                        self.set_root_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
                    }
                    self.finish_actor(&task, &Err(ExitReason::Killed));
                } else {
                    self.actors.insert(id, task);
                    self.ready.push_back(id);
                }
            },
            Ok(RunOutcome::Suspended(wait)) => {
                if task.handle.kill_requested() {
                    if is_root {
                        self.set_root_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
                    }
                    self.finish_actor(&task, &Err(ExitReason::Killed));
                } else {
                    self.actor_wait(id, task, wait);
                }
            },
            Ok(RunOutcome::ActorExit(reason)) => {
                if is_root {
                    self.set_root_result(Err(Error::internal("@Main 不能以 Actor 方式终止")));
                }
                self.finish_actor(&task, &Err(reason));
            },
            Err(error) => {
                if is_root {
                    self.set_root_result(Err(error.clone()));
                } else {
                    self.shared
                        .lock()
                        .actor_errors
                        .push((task.handle.observation_flag(), error.clone()));
                }
                self.finish_actor(&task, &Err(error_to_exit(&error)));
            },
        }
    }

    fn actor_wait(&mut self, id: u64, mut task: ActorTask, wait: Wait) {
        match wait {
            Wait::Join { target } => {
                if self.is_root(target.id) {
                    task.machine
                        .resume_error(Error::runtime(ErrorKind::JoinRoot, None));
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.ready.push_back(id);
                    return;
                }
                if let Some(value) = self.handle.wait_join(&target, id) {
                    task.machine.resume(value);
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.ready.push_back(id);
                } else {
                    task.wait = Some(Wait::Join { target });
                    self.actors.insert(id, task);
                }
            },
            Wait::Receive {
                index,
                timeout,
                span,
            } => {
                if let Some(message) = task.handle.try_recv(index) {
                    task.machine.resume(option_some(message));
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.ready.push_back(id);
                    return;
                }
                self.receive_waiters.insert((id, index), id);
                if let Some(millis) = timeout
                    && millis > 0
                    && let Err(error) = self.add_timer(millis, id, TimerKind::Receive, span)
                {
                    self.receive_waiters.remove(&(id, index));
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.fail_actor(id, error);
                    return;
                }
                task.wait = Some(Wait::Receive {
                    index,
                    timeout,
                    span,
                });
                self.actors.insert(id, task);
            },
            Wait::Io { op_id } => {
                self.io_waiters.insert(op_id, id);
                task.wait = Some(Wait::Io { op_id });
                self.actors.insert(id, task);
            },
            Wait::Sleep { millis, span } => {
                if millis > 0 {
                    if let Err(error) = self.add_timer(millis, id, TimerKind::Sleep, span) {
                        task.wait = None;
                        self.actors.insert(id, task);
                        self.fail_actor(id, error);
                        return;
                    }
                } else {
                    task.machine.resume(Value::Nil);
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.ready.push_back(id);
                    return;
                }
                task.wait = Some(Wait::Sleep { millis, span });
                self.actors.insert(id, task);
            },
        }
    }

    fn resume_actor(&mut self, id: u64, value: Value) {
        if let Some(mut task) = self.actors.remove(&id) {
            self.clear_wait(id);
            task.wait = None;
            task.machine.resume(value);
            self.actors.insert(id, task);
            self.ready.push_back(id);
        }
    }

    fn fail_actor(&mut self, id: u64, error: Error) {
        if let Some(mut task) = self.actors.remove(&id) {
            self.clear_wait(id);
            task.wait = None;
            task.machine.resume_error(error);
            self.actors.insert(id, task);
            self.ready.push_back(id);
        }
    }

    fn finish_actor(&mut self, task: &ActorTask, result: &ActorResult) {
        let handle = Arc::clone(&task.handle);
        if !handle.finish(result.clone()) {
            return;
        }
        self.announce_finished(&handle, result);
    }

    /// 记录 Actor 已终止并唤醒等待者; 尚未启动就终止的 Actor 也走这里.
    fn announce_finished(&mut self, handle: &Arc<ActorHandle>, result: &ActorResult) {
        let id = handle.id;
        self.handle.cleanup_names(id);
        self.handle.deliver_monitors(id, result);
        self.handle.cleanup_monitors(id);
        self.handle.cleanup_send_timers(id);
        self.clear_wait(id);
        let waiters = self.handle.take_join_waiters(id);
        if !waiters.is_empty() {
            let value = join_result_value(result);
            for waiter in waiters {
                self.handle.push_event(ShardEvent::Join {
                    actor: waiter.actor,
                    value: value.clone(),
                });
            }
        }
        for child in handle.take_children() {
            self.kill_actor(child);
        }
    }

    /// 把尚未启动的 Actor 直接置为终止, 并唤醒可能已经在 Join 它的任务.
    fn finish_pending(&mut self, handle: &Arc<ActorHandle>, reason: ExitReason) {
        let result = Err(reason);
        if handle.finish(result.clone()) {
            self.announce_finished(handle, &result);
        }
    }

    fn kill_actor(&mut self, id: u64) {
        if self.is_root(id) {
            self.set_root_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
        }
        if let Some(task) = self.actors.remove(&id) {
            self.ready.retain(|queued| *queued != id);
            task.handle.request_kill();
            if let Some(Wait::Io { op_id }) = &task.wait {
                self.handle.cancel_io(*op_id);
                self.io_waiters.remove(op_id);
            }
            self.finish_actor(&task, &Err(ExitReason::Killed));
        }
    }

    /// 终止全部 Actor 并清空调度状态; 调度器关闭时调用.
    pub(super) fn kill_all(&mut self) {
        let ids: Vec<u64> = self.actors.keys().copied().collect();
        for id in ids {
            self.kill_actor(id);
        }
        self.ready.clear();
        self.timers.clear();
        self.receive_waiters.clear();
        self.io_waiters.clear();
        self.root_actor = None;
        self.root_result = None;
    }

    fn wake_send(&mut self, target: u64, index: usize) {
        let Some(waiter) = self.receive_waiters.remove(&(target, index)) else {
            return;
        };
        let Some(mut task) = self.actors.remove(&waiter) else {
            return;
        };
        // 唤醒事件与消息消费存在竞争: 消息可能已被同一 Actor 在挂起前的
        // 非阻塞检查中取走, 此时事件已过期. 过期事件不能交付 None, 否则
        // 无限等待的 Receive 会被误唤醒; 恢复登记, 继续等待下一条消息.
        let Some(message) = task.handle.try_recv(index) else {
            self.receive_waiters.insert((target, index), waiter);
            self.actors.insert(waiter, task);
            return;
        };
        self.clear_wait(waiter);
        task.wait = None;
        task.machine.resume(option_some(message));
        self.actors.insert(waiter, task);
        self.ready.push_back(waiter);
    }

    /// 清除一个 Actor 的等待登记; 定时器条目留在堆中延迟清理.
    pub(super) fn clear_wait(&mut self, actor: u64) {
        self.timers.remove(&actor);
        self.receive_waiters.retain(|_, waiter| *waiter != actor);
        self.handle.remove_join_waiter(actor);
    }

    pub(super) fn add_timer(
        &mut self,
        millis: u64,
        actor: u64,
        kind: TimerKind,
        span: Span,
    ) -> Result<(), Error> {
        let duration = Duration::from_millis(millis);
        let Some(deadline) = Instant::now().checked_add(duration) else {
            return Err(Error::runtime(ErrorKind::TimeoutOutOfRange, Some(span)));
        };
        self.timers.insert(deadline, actor, kind);
        Ok(())
    }

    pub(super) fn fire_timers(&mut self) {
        // 没有活动定时器时不取当前时间; 调度内循环的热路径上这是系统调用开销.
        if self.timers.is_empty() {
            return;
        }
        for (actor, kind) in self.timers.take_due(Instant::now()) {
            let value = match kind {
                TimerKind::Sleep => Value::Nil,
                TimerKind::Receive => option_none(),
            };
            self.resume_actor(actor, value);
        }
    }

    /// 触发到期的延迟投递; 共享锁保证每个条目只被取走一次.
    pub(super) fn fire_send_timers(&mut self) {
        // 没有延迟消息时避免每轮取当前时间; 热路径上这是可观的系统调用开销.
        let due = {
            let mut shared = self.shared.lock();
            if shared.send_timers.is_empty() {
                return;
            }
            shared.take_due_send_timers(Instant::now())
        };
        for timer in due {
            let Some(target) = timer.target.upgrade() else {
                continue;
            };
            if target.is_finished() {
                continue;
            }
            if target.send(timer.index, timer.message) {
                self.handle.wake_send(timer.target_id, timer.index);
            }
        }
    }

    /// 等待 I/O 完成通知或最近的定时器.
    ///
    /// 截止时刻取任务定时堆与共享延迟投递堆的最小值; 没有定时器时阻塞
    /// 到 I/O 完成. 调用方保证此时至少有一个等待中的 I/O 或定时器, 否则
    /// 会永久阻塞而不是报告死锁.
    pub(super) fn wait_for_next_event(&mut self) {
        let send_deadline = self.shared.lock().next_send_deadline();
        let deadline = self
            .timers
            .next_deadline()
            .into_iter()
            .chain(send_deadline)
            .min();
        let Some(deadline) = deadline else {
            let _ = self.wake.recv();
            return;
        };
        let now = Instant::now();
        if deadline > now {
            // 超时与 I/O 完成同等对待; 循环顶端统一处理到期定时器与完成队列.
            let _ = self.wake.recv_timeout(deadline - now);
        }
    }
}

fn error_to_exit(error: &Error) -> ExitReason {
    match error {
        Error::Runtime(error) => ExitReason::Failed {
            code: i64::from(error.kind.number()),
            message: error.title(),
        },
        Error::Internal(message) => ExitReason::Failed {
            code: 0,
            message: message.clone(),
        },
        Error::Exit(code) => ExitReason::Failed {
            code: i64::from(*code),
            message: "Exit".to_string(),
        },
        // 输出流已关闭时不再产生 Actor 错误; 进程会很快没有可写的下游.
        Error::BrokenPipe => ExitReason::Killed,
    }
}
