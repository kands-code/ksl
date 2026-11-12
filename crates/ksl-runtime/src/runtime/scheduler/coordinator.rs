//! 运行时协调器.
//!
//! 持有 Prelude 任务, Actor 任务, 就绪队列, 定时器与等待表.
//! 事件经共享事件队列回到调度循环; 协调器在调用线程上协作推进,
//! 不另起线程执行用户代码.

use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use arcstr::ArcStr;
use crossbeam_channel::{Receiver, TryRecvError};
use ksl_base::source::Span;
use ksl_frontend::ast::Program;
use ksl_frontend::check::{Type, TypeTable};
use ksl_frontend::error::{ErrorKind, RuntimeError as Error};
use parking_lot::Mutex;

use self::join::JoinWaiter;
use self::monitors::MonitorEntry;
use self::names::RegisteredPid;
use self::send_timers::SendTimer;
use self::timers::TimerKind;
use super::ingress::{Ingress, SchedulerHandle, SpawnRequest};
use crate::ffi::ReleaseQueue;
use crate::io::Task as IoTask;
use crate::runtime::event::{Event, TaskId};
use crate::runtime::gc::Marker;
use crate::runtime::machine::{Effect, EffectResult, Machine, RunOutcome, Wait};
use crate::timer_heap::TimerHeap;
use crate::value::{
    ActorHandle, ActorResult, Env, ExitReason, Value, join_result_value, option_none,
    option_some,
};

/// Prelude 任务的固定编号; Actor 编号从 1 开始.
const PRELUDE_ID: u64 = 0;

mod effects;
mod join;
mod monitors;
mod names;
mod send_timers;
mod timers;

/// 任务在调度上的状态.
///
/// 任务只在调度线程推进; 推进中的任务暂时离开任务表, 因此不需要"执行中"状态.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Ready,
    Waiting,
    Finished,
}

/// 任务在语言层的角色.
enum Role {
    /// 模块顶层语句与交互输入; 不是 Actor, 没有句柄.
    Prelude,
    Actor {
        handle: Arc<ActorHandle>,
    },
}

/// 一个任务的执行状态.
struct Task {
    /// 推进期间机器由 `run_task` 的局部变量持有; 就绪与等待时保存在任务内.
    machine: Option<Machine>,
    role: Role,
    place: Place,
}

impl Task {
    fn machine(&self) -> &Machine {
        self.machine.as_ref().expect("任务机器不在任务内")
    }

    fn machine_mut(&mut self) -> &mut Machine {
        self.machine.as_mut().expect("任务机器不在任务内")
    }

    fn take_machine(&mut self) -> Machine {
        self.machine.take().expect("任务机器不在任务内")
    }

    fn actor(&self) -> Option<&Arc<ActorHandle>> {
        match &self.role {
            Role::Prelude => None,
            Role::Actor { handle } => Some(handle),
        }
    }

    fn is_prelude(&self) -> bool {
        matches!(self.role, Role::Prelude)
    }
}

/// 协调器持有的全部任务状态.
pub(super) struct Coordinator {
    handle: SchedulerHandle,
    ingress: Arc<Mutex<Ingress>>,
    /// 协调器收件箱; 跨线程完成与唤醒都经此入队.
    inbox: Receiver<Event>,
    /// Prelude 任务的编号; 模块顶层与交互输入复用同一台机器.
    prelude: TaskId,
    /// 任务表; 就绪, 等待与已终止的任务按编号登记.
    tasks: HashMap<TaskId, Task>,
    /// 正在推进的 Actor 句柄; 该任务此刻不在 `tasks` 中.
    running: Option<Arc<ActorHandle>>,
    ready: VecDeque<TaskId>,
    io_waiters: HashMap<u64, TaskId>,
    /// 任务等待超时堆; 每个任务至多一个活动定时器.
    timers: TimerHeap<TaskId, TimerKind>,
    /// 指令预算; 测试可动态调小.
    slice_budget: Arc<AtomicUsize>,
    /// 当前程序的根任务编号; Prelude 或 `@Main` 根 Actor.
    root: Option<TaskId>,
    /// 当前程序的结局; Prelude 与根 Actor 共用一个结果槽.
    program_result: Option<Result<Value, Error>>,
    /// 调度线程释放队列; 按线程亲和性路由释放任务.
    releases: Arc<ReleaseQueue>,
    next_id: u64,
    /// 命名注册表以运行时字符串为键, 不进入全局符号驻留表.
    names: HashMap<ArcStr, RegisteredPid>,
    names_by_pid: HashMap<u64, HashSet<ArcStr>>,
    next_monitor: u64,
    monitors: HashMap<u64, MonitorEntry>,
    monitors_by_target: HashMap<u64, HashSet<u64>>,
    monitors_by_watcher: HashMap<u64, HashSet<u64>>,
    /// 目标终止时等待 Join 的 Actor; 键是目标编号.
    join_waiters: HashMap<u64, Vec<JoinWaiter>>,
    next_timer: u64,
    /// 延迟投递定时器; 取消或目标终止后条目惰性失效.
    pub(super) send_timers: TimerHeap<u64, SendTimer>,
}

impl Coordinator {
    pub(super) fn new(
        handle: SchedulerHandle,
        ingress: Arc<Mutex<Ingress>>,
        inbox: Receiver<Event>,
        slice_budget: Arc<AtomicUsize>,
        releases: Arc<ReleaseQueue>,
    ) -> Self {
        let mut main_machine = Machine::new();
        main_machine.set_gc(Arc::clone(handle.registry()));
        main_machine.set_runtime(
            Arc::clone(handle.io()),
            handle.events_sender().clone(),
            Arc::clone(&releases),
        );
        let mut tasks = HashMap::new();
        tasks.insert(
            PRELUDE_ID,
            Task {
                machine: Some(main_machine),
                role: Role::Prelude,
                place: Place::Finished,
            },
        );
        Self {
            handle,
            ingress,
            inbox,
            prelude: PRELUDE_ID,
            tasks,
            running: None,
            ready: VecDeque::new(),
            io_waiters: HashMap::new(),
            timers: TimerHeap::new(),
            slice_budget,
            root: None,
            program_result: None,
            releases,
            next_id: 1,
            names: HashMap::new(),
            names_by_pid: HashMap::new(),
            next_monitor: 1,
            monitors: HashMap::new(),
            monitors_by_target: HashMap::new(),
            monitors_by_watcher: HashMap::new(),
            join_waiters: HashMap::new(),
            next_timer: 1,
            send_timers: TimerHeap::new(),
        }
    }

    /// 调整单次调度的指令预算; 供测试缩小规模.
    #[cfg(test)]
    pub(super) fn set_slice_budget(&mut self, budget: usize) {
        self.slice_budget.store(budget, Ordering::Relaxed);
    }

    fn prelude_task(&self) -> &Task {
        self.tasks.get(&self.prelude).expect("Prelude 任务缺失")
    }

    fn prelude_task_mut(&mut self) -> &mut Task {
        self.tasks.get_mut(&self.prelude).expect("Prelude 任务缺失")
    }

    /// 注入 Prelude 的命令行参数.
    pub(super) fn set_args(&mut self, args: Vec<String>) {
        self.prelude_task_mut().machine_mut().set_args(args);
    }

    /// 替换 Prelude 的全局环境.
    pub(super) fn set_globals(&mut self, env: Env) {
        self.prelude_task_mut().machine_mut().set_globals(env);
    }

    #[must_use]
    pub(super) fn globals(&self) -> &Env {
        self.prelude_task().machine().globals()
    }

    /// 装入一段 Prelude 程序并标记为就绪.
    pub(super) fn start_prelude(&mut self, program: &Program, types: &TypeTable) {
        let task = self.prelude_task_mut();
        task.machine_mut().start_program(program, types);
        task.place = Place::Ready;
        self.root = Some(self.prelude);
        self.program_result = None;
    }

    #[must_use]
    pub(super) fn prelude_ready(&self) -> bool {
        self.prelude_task().place == Place::Ready
    }

    /// 推进一次 Prelude 片段.
    pub(super) fn run_prelude(&mut self) {
        self.run_task(self.prelude);
    }

    /// 推进一个就绪任务; 没有就绪任务时返回 false.
    pub(super) fn run_next(&mut self) -> bool {
        let Some(id) = self.ready.pop_front() else {
            return false;
        };
        self.run_task(id);
        true
    }

    /// 在调度线程推进一个任务; 推进期间任务暂时离开任务表.
    fn run_task(&mut self, id: TaskId) {
        let Some(mut task) = self.tasks.remove(&id) else {
            return;
        };
        if task.place != Place::Ready {
            self.tasks.insert(id, task);
            return;
        }
        if task.actor().is_some_and(|handle| handle.kill_requested()) {
            if self.is_root(id) {
                self.set_program_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
            }
            self.finish_task(task, &Err(ExitReason::Killed));
            return;
        }
        let mut machine = task.take_machine();
        self.running = task.actor().cloned();
        let budget = self.slice_budget();
        let outcome = run_slice(&mut machine, budget);
        task.machine = Some(machine);
        self.handle_task_outcome(id, task, outcome);
        self.running = None;
    }

    fn prelude_wait(&mut self, id: u64, mut task: Task, wait: Wait) {
        let Wait::Io { op_id, .. } = &wait else {
            // 检查器已禁止 Prelude 使用 Receive/Join/Sleep.
            task.machine_mut()
                .resume_error(Error::internal("Prelude 不能挂起在 Actor 原语上"));
            self.requeue(id, task);
            return;
        };
        self.io_waiters.insert(*op_id, id);
        task.machine_mut().park(wait);
        task.place = Place::Waiting;
        self.tasks.insert(id, task);
    }

    /// 取走当前程序的结局.
    pub(super) fn take_program_result(&mut self) -> Option<Result<Value, Error>> {
        self.program_result.take()
    }

    /// 取消 Prelude 等待中的 I/O 并清空任务状态.
    pub(super) fn clear_prelude(&mut self) {
        let id = self.prelude;
        if let Some(task) = self.tasks.get_mut(&id) {
            if let Some(Wait::Io { op_id, .. }) = task.machine.as_ref().and_then(Machine::wait)
            {
                self.handle.cancel_io(*op_id);
            }
            task.place = Place::Finished;
        }
        if self.root == Some(id) {
            self.root = None;
        }
        self.program_result = None;
    }

    fn releases(&self) -> &Arc<ReleaseQueue> {
        &self.releases
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 创建 Actor 句柄并登记启动请求; 返回 Pid 值.
    fn spawn(
        &mut self,
        parent: Option<&Arc<ActorHandle>>,
        mailboxes: usize,
        detached: bool,
        handler: Value,
        args: Vec<Value>,
    ) -> Value {
        let id = self.alloc_id();
        let parent_handle = if detached { None } else { parent.cloned() };
        let handle = Arc::new(ActorHandle::new(id, mailboxes, parent_handle.clone()));
        self.handle.registry().register_actor(&handle);
        if let Some(parent) = &parent_handle {
            parent.add_child(id);
        }
        self.start_spawn(SpawnRequest {
            handle: Arc::clone(&handle),
            handler,
            args,
        });
        Value::Pid(handle)
    }

    pub(super) fn has_io_waiters(&self) -> bool {
        !self.io_waiters.is_empty()
    }

    pub(super) fn has_timers(&self) -> bool {
        !self.timers.is_empty()
    }

    /// 遍历协调器状态中的运行时值; 环回收根标记用.
    pub(super) fn trace(&self, marker: &mut Marker) {
        for task in self.tasks.values() {
            if let Some(machine) = &task.machine {
                machine.trace(marker);
            }
            if let Some(handle) = task.actor() {
                marker.mark_actor(handle);
            }
        }
        if let Some(Ok(value)) = &self.program_result {
            marker.mark_value(value);
        }
        for timer in self.send_timers.values() {
            marker.mark_value(&timer.message);
        }
    }

    /// 处理收件箱中的全部待处理事件.
    pub(super) fn drain_events(&mut self) {
        loop {
            match self.inbox.try_recv() {
                Ok(event) => self.handle_event(event),
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Io {
                task,
                op_id,
                result,
            } => self.deliver_io(task, op_id, result),
            Event::SendWake { target, index } => self.wake_send(target, index),
            Event::Wake => {},
        }
    }

    /// 创建根 Actor 并启动.
    pub(super) fn spawn_root(
        &mut self,
        mailboxes: usize,
        handler: Value,
        args: Vec<Value>,
    ) -> Value {
        let id = self.alloc_id();
        let handle = Arc::new(ActorHandle::new(id, mailboxes, None));
        self.handle.registry().register_actor(&handle);
        self.root = Some(handle.id);
        // 根 Actor 取代 Prelude 成为当前程序的结局来源.
        self.prelude_task_mut().place = Place::Finished;
        self.program_result = None;
        self.start_spawn(SpawnRequest {
            handle: Arc::clone(&handle),
            handler,
            args,
        });
        Value::Pid(handle)
    }

    pub(super) fn is_root(&self, id: u64) -> bool {
        self.root == Some(id)
    }

    pub(super) fn set_program_result(&mut self, result: Result<Value, Error>) {
        // 首个结局生效; 运行中的 Actor 随后返回的结果不覆盖程序结局.
        if self.program_result.is_none() {
            self.program_result = Some(result);
        }
    }

    /// 当前程序是否已有结局.
    pub(super) fn program_finished(&self) -> bool {
        self.program_result.is_some()
    }

    fn start_spawn(&mut self, request: super::ingress::SpawnRequest) {
        let id = request.handle.id;
        // 父 Actor 可能在子 Actor 被调度前就已终止, 或子 Actor 已被 Kill.
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
        let cli_args = self.ingress.lock().args.clone();
        match Machine::start_actor(
            request.handler,
            Arc::clone(&request.handle),
            request.args,
            cli_args,
            Arc::clone(self.handle.registry()),
        ) {
            Ok(mut machine) => {
                machine.set_runtime(
                    Arc::clone(self.handle.io()),
                    self.handle.events_sender().clone(),
                    Arc::clone(&self.releases),
                );
                self.tasks.insert(
                    id,
                    Task {
                        machine: Some(machine),
                        role: Role::Actor {
                            handle: request.handle,
                        },
                        place: Place::Ready,
                    },
                );
                self.ready.push_back(id);
            },
            Err(error) => {
                let (reason, report) = settle_failure(&error);
                if report {
                    self.ingress
                        .lock()
                        .actor_errors
                        .push((request.handle.observation_flag(), error));
                }
                self.finish_pending(&request.handle, reason);
            },
        }
    }

    fn deliver_io(&mut self, task: TaskId, op_id: u64, result: Result<Value, Error>) {
        if self.io_waiters.remove(&op_id) != Some(task) {
            return;
        }
        match result {
            Ok(value) => self.resume_task(task, value),
            Err(error) => self.fail_task(task, error),
        }
    }

    /// 按片段结局推进任务.
    fn handle_task_outcome(
        &mut self,
        id: u64,
        mut task: Task,
        outcome: Result<RunOutcome, Error>,
    ) {
        let is_root = self.is_root(id);
        let is_prelude = task.is_prelude();
        match outcome {
            Ok(RunOutcome::Finished(value)) => {
                if is_root {
                    self.set_program_result(Ok(value.clone()));
                }
                if is_prelude {
                    task.place = Place::Finished;
                    self.tasks.insert(id, task);
                } else {
                    self.finish_task(task, &Ok(value));
                }
            },
            Ok(RunOutcome::Effect(effect)) => {
                match self.apply_effect(task.actor(), id, effect) {
                    EffectResult::Resume(value) => {
                        task.machine_mut().resume(value);
                        self.requeue(id, task);
                    },
                    EffectResult::Park(wait) => {
                        if is_prelude {
                            self.prelude_wait(id, task, wait);
                        } else {
                            self.actor_wait(id, task, wait);
                        }
                    },
                    EffectResult::Fail(error) => {
                        task.machine_mut().resume_error(error);
                        self.requeue(id, task);
                    },
                }
            },
            Ok(RunOutcome::Yield) => self.after_yield(id, task, is_root, is_prelude),
            Ok(RunOutcome::Suspended(wait)) => {
                if !is_prelude && task.actor().is_some_and(|handle| handle.kill_requested()) {
                    if is_root {
                        self.set_program_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
                    }
                    self.finish_task(task, &Err(ExitReason::Killed));
                } else if is_prelude {
                    self.prelude_wait(id, task, wait);
                } else {
                    self.actor_wait(id, task, wait);
                }
            },
            Ok(RunOutcome::ActorExit(reason)) => {
                if is_prelude {
                    self.set_program_result(Err(Error::internal("Prelude 不能终止 Actor")));
                    task.place = Place::Finished;
                    self.tasks.insert(id, task);
                } else {
                    if is_root {
                        self.set_program_result(Err(Error::internal(
                            "@Main 不能以 Actor 方式终止",
                        )));
                    }
                    self.finish_task(task, &Err(reason));
                }
            },
            Err(error) => {
                if is_root {
                    self.set_program_result(Err(error.clone()));
                }
                if is_prelude {
                    task.place = Place::Finished;
                    self.tasks.insert(id, task);
                } else {
                    let (reason, report) = settle_failure(&error);
                    if !is_root && report {
                        let handle = task.actor().expect("非 Prelude 任务缺少句柄");
                        self.ingress
                            .lock()
                            .actor_errors
                            .push((handle.observation_flag(), error));
                    }
                    self.finish_task(task, &Err(reason));
                }
            },
        }
    }

    /// 让出片段后的统一处理; 预算耗尽与 Actor 让出共用.
    fn after_yield(&mut self, id: u64, task: Task, is_root: bool, is_prelude: bool) {
        if !is_prelude && task.actor().is_some_and(|handle| handle.kill_requested()) {
            if is_root {
                self.set_program_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
            }
            self.finish_task(task, &Err(ExitReason::Killed));
        } else {
            self.requeue(id, task);
        }
    }

    /// 把任务放回就绪队列; 状态与等待登记在同一处更新.
    ///
    /// Prelude 不进入就绪队列, 调度循环每轮直接检查它的状态.
    fn requeue(&mut self, id: u64, mut task: Task) {
        task.place = Place::Ready;
        let is_prelude = task.is_prelude();
        self.tasks.insert(id, task);
        if !is_prelude {
            self.ready.push_back(id);
        }
    }

    /// 当前片段的指令预算.
    pub(super) fn slice_budget(&self) -> usize {
        self.slice_budget.load(Ordering::Relaxed)
    }

    /// 收件箱是否还有未处理事件.
    #[must_use]
    pub(super) fn has_pending_events(&self) -> bool {
        !self.inbox.is_empty()
    }

    /// 最近的定时器截止时刻; 等待完成时用作超时.
    pub(super) fn next_deadline(&mut self) -> Option<Instant> {
        let send = self.next_send_deadline();
        self.timers.next_deadline().into_iter().chain(send).min()
    }

    fn actor_wait(&mut self, id: u64, mut task: Task, wait: Wait) {
        match wait {
            Wait::Join { target, span } => {
                if id == target.id {
                    task.machine_mut()
                        .resume_error(Error::runtime(ErrorKind::JoinSelf, Some(span)));
                    self.requeue(id, task);
                    return;
                }
                if self.is_root(target.id) {
                    task.machine_mut()
                        .resume_error(Error::runtime(ErrorKind::JoinRoot, Some(span)));
                    self.requeue(id, task);
                    return;
                }
                if let Some(value) = self.wait_join(&target, id) {
                    // 结果已交付给等待者, 该失败才算被观察.
                    target.mark_observed();
                    task.machine_mut().resume(value);
                    self.requeue(id, task);
                } else {
                    task.machine_mut().park(Wait::Join { target, span });
                    task.place = Place::Waiting;
                    self.tasks.insert(id, task);
                }
            },
            Wait::Receive {
                index,
                timeout,
                span,
            } => {
                // 先武装唤醒位再检查信箱: 此后到达的消息一定能看到该位.
                let handle = Arc::clone(task.actor().expect("@Receive 出现在非 Actor 任务"));
                handle.arm_wake(index);
                if handle.has_message(index) {
                    handle.clear_wake();
                    task.machine_mut().park(Wait::Receive {
                        index,
                        timeout,
                        span,
                    });
                    task.machine_mut().retry_receive();
                    self.requeue(id, task);
                    return;
                }
                if let Some(millis) = timeout
                    && millis > 0
                    && let Err(error) = self.add_timer(millis, id, TimerKind::Receive, span)
                {
                    task.machine_mut().resume_error(error);
                    self.requeue(id, task);
                    return;
                }
                task.machine_mut().park(Wait::Receive {
                    index,
                    timeout,
                    span,
                });
                task.place = Place::Waiting;
                self.tasks.insert(id, task);
            },
            wait @ Wait::Io { op_id, .. } => {
                self.io_waiters.insert(op_id, id);
                task.machine_mut().park(wait);
                task.place = Place::Waiting;
                self.tasks.insert(id, task);
            },
            Wait::Sleep { millis, span } => {
                if millis > 0 {
                    if let Err(error) = self.add_timer(millis, id, TimerKind::Sleep, span) {
                        task.machine_mut().resume_error(error);
                        self.requeue(id, task);
                        return;
                    }
                } else {
                    task.machine_mut().resume(Value::Nil);
                    self.requeue(id, task);
                    return;
                }
                task.machine_mut().park(Wait::Sleep { millis, span });
                task.place = Place::Waiting;
                self.tasks.insert(id, task);
            },
        }
    }

    fn resume_task(&mut self, id: u64, value: Value) {
        if let Some(mut task) = self.tasks.remove(&id) {
            self.clear_wait(id);
            if let Some(handle) = task.actor() {
                handle.clear_wake();
            }
            task.machine_mut().resume(value);
            self.requeue(id, task);
        }
    }

    fn fail_task(&mut self, id: u64, error: Error) {
        if let Some(mut task) = self.tasks.remove(&id) {
            self.clear_wait(id);
            if let Some(handle) = task.actor() {
                handle.clear_wake();
            }
            task.machine_mut().resume_error(error);
            self.requeue(id, task);
        }
    }

    /// 结束一个任务并清理其子 Actor; 先释放机器, 再把结局写入句柄.
    fn finish_task(&mut self, task: Task, result: &ActorResult) {
        let children = self.settle_task(task, result);
        self.kill_actors(children);
    }

    /// 释放任务机器并记录结局, 返回待终止的子 Actor 编号.
    fn settle_task(&mut self, mut task: Task, result: &ActorResult) -> Vec<u64> {
        task.place = Place::Finished;
        task.machine = None;
        self.finish_actor(&task, result)
    }

    fn finish_actor(&mut self, task: &Task, result: &ActorResult) -> Vec<u64> {
        let handle = Arc::clone(task.actor().expect("结束的任务缺少句柄"));
        handle.clear_wake();
        if !handle.finish(result.clone()) {
            return Vec::new();
        }
        self.announce_finished(&handle, result)
    }

    /// 记录 Actor 已终止并唤醒等待者, 返回仍需终止的子 Actor.
    fn announce_finished(
        &mut self,
        handle: &Arc<ActorHandle>,
        result: &ActorResult,
    ) -> Vec<u64> {
        let id = handle.id;
        // 已终止的子 Actor 不再需要父句柄代为清理.
        handle.detach_from_parent();
        self.cleanup_names(id);
        self.deliver_monitors(handle, result);
        self.cleanup_monitors(id);
        self.cleanup_send_timers(id);
        self.clear_wait(id);
        let holder = if self.is_root(id) {
            crate::terminal::state::RawModeHolder::Entry
        } else {
            crate::terminal::state::RawModeHolder::Actor(id)
        };
        self.handle.release_terminal_holder(holder);
        let waiters: Vec<_> = self
            .take_join_waiters(id)
            .into_iter()
            .filter(|waiter| self.tasks.contains_key(&waiter.actor))
            .collect();
        if !waiters.is_empty() {
            // 至少一个等待者实际取得结果, 该失败才算被观察.
            handle.mark_observed();
            let value = join_result_value(result);
            for waiter in waiters {
                self.resume_task(waiter.actor, value.clone());
            }
        }
        handle.take_children()
    }

    /// 把尚未启动的 Actor 直接置为终止, 并唤醒可能已经在 Join 它的任务.
    fn finish_pending(&mut self, handle: &Arc<ActorHandle>, reason: ExitReason) {
        let result = Err(reason);
        if handle.finish(result.clone()) {
            let children = self.announce_finished(handle, &result);
            self.kill_actors(children);
        }
    }

    fn kill_actor(&mut self, id: u64) -> Vec<u64> {
        if id == self.prelude {
            return Vec::new();
        }
        if self.is_root(id) {
            self.set_program_result(Err(Error::runtime(ErrorKind::KillRoot, None)));
        }
        let Some(task) = self.tasks.remove(&id) else {
            // 正在推进的任务尚未回到任务表; 只登记终止请求, 由下一个让出点结算.
            if let Some(handle) = &self.running
                && handle.id == id
            {
                handle.request_kill();
                handle.clear_wake();
            }
            return Vec::new();
        };
        if let Some(handle) = task.actor() {
            handle.request_kill();
            handle.clear_wake();
        }
        if let Some(Wait::Io { op_id, .. }) = task.machine.as_ref().and_then(Machine::wait) {
            self.handle.cancel_io(*op_id);
            self.io_waiters.remove(op_id);
        }
        self.settle_task(task, &Err(ExitReason::Killed))
    }

    /// 迭代终止一批 Actor 及其子树; 显式栈避免深树递归.
    fn kill_actors(&mut self, roots: impl IntoIterator<Item = u64>) {
        let mut pending: Vec<u64> = roots.into_iter().collect();
        pending.reverse();
        while let Some(id) = pending.pop() {
            let children = self.kill_actor(id);
            pending.extend(children.into_iter().rev());
        }
    }

    /// 终止全部 Actor 并清空调度状态; 调度器关闭时调用.
    pub(super) fn kill_all(&mut self) {
        let ids: Vec<u64> = self
            .tasks
            .iter()
            .filter_map(|(id, task)| task.actor().is_some().then_some(*id))
            .collect();
        self.kill_actors(ids);
        self.tasks.retain(|_, task| task.is_prelude());
        self.ready.clear();
        self.timers.clear();
        self.io_waiters.clear();
        self.clear_names();
        self.clear_monitors();
        self.join_waiters.clear();
        self.send_timers.clear();
        self.root = None;
        self.program_result = None;
    }

    fn wake_send(&mut self, target: u64, index: usize) {
        {
            let Some(task) = self.tasks.get_mut(&target) else {
                return;
            };
            if task.place != Place::Waiting {
                return;
            }
            let Some(handle) = task.actor().cloned() else {
                return;
            };
            let Some(Wait::Receive { index: waiting, .. }) = task.machine().wait() else {
                return;
            };
            if *waiting != index {
                return;
            }
            handle.clear_wake();
            task.machine_mut().retry_receive();
            task.place = Place::Ready;
        }
        self.clear_wait(target);
        self.ready.push_back(target);
    }

    /// 清除一个 Actor 的等待登记; 定时器条目留在堆中延迟清理.
    pub(super) fn clear_wait(&mut self, actor: u64) {
        self.timers.remove(&actor);
        self.remove_join_waiter(actor);
    }

    /// 等待事件或最近的截止时刻.
    ///
    /// 超时取任务定时堆, 延迟投递堆与 `cap` 的最小值; 三者都不存在时阻塞到事件到达.
    /// `cap` 是事件泵轮询给出的等待上限.
    pub(super) fn wait_for_next_event(&mut self, cap: Option<Duration>) {
        let deadline = self.next_deadline().map(|deadline| {
            let now = Instant::now();
            deadline.saturating_duration_since(now)
        });
        let timeout = match (deadline, cap) {
            (Some(deadline), Some(cap)) => Some(deadline.min(cap)),
            (deadline, cap) => deadline.or(cap),
        };
        let Some(timeout) = timeout else {
            if let Ok(event) = self.inbox.recv() {
                self.handle_event(event);
            }
            return;
        };
        if !timeout.is_zero()
            && let Ok(event) = self.inbox.recv_timeout(timeout)
        {
            // 超时与事件到达同等对待; 到达的事件在此直接处理.
            self.handle_event(event);
        }
    }
}

/// 推进一个执行片段.
///
/// 片段内的 panic 转为内部错误: 用户代码不能因为一次越界访问终止整个运行时.
/// panic 后机器的控制状态不可信, 调用方只按失败结束该任务, 不再继续推进.
fn run_slice(machine: &mut Machine, budget: usize) -> Result<RunOutcome, Error> {
    catch_unwind(AssertUnwindSafe(|| machine.run_budget(budget))).unwrap_or_else(|payload| {
        Err(Error::internal(format!(
            "片段 panic: {}",
            crate::runtime::panic::message(payload.as_ref())
        )))
    })
}

/// 失败任务的结算: 退出原因与是否作为未观察错误上报.
///
/// 输出流已关闭时下游已不在: 该失败按终止结算, 也不上报.
fn settle_failure(error: &Error) -> (ExitReason, bool) {
    let reason = match error {
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
        Error::BrokenPipe => ExitReason::Killed,
    };
    (reason, !matches!(error, Error::BrokenPipe))
}
