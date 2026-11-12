//! 单线程协作调度器.
//!
//! 主脚本与 Actor 都是任务, 每个任务按指令预算推进; 挂起时保存抽象机
//! 控制状态, 唤醒时把结果交回续延. 阻塞 I/O 交给宿主工作线程, 调度器
//! 自身仍是单线程协作式, 并发模型的远期方向见 `docs/design.md` 第 14 节.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use super::exec::{Machine, RunOutcome, Wait};
use super::gc::{GcRegistry, Marker};
use crate::ast::Program;
use crate::error::{ErrorKind, RuntimeError as Error};
use crate::io::IoKind;
use crate::source::Span;
use crate::value::{
    ActorHandle, ActorResult, Env, ExitReason, Value, join_result_value, option_none,
    option_some,
};

mod shared;

pub(crate) use shared::SchedulerHandle;
use shared::{Shared, Wake};

/// 每个任务一次调度的指令预算.
const BUDGET: usize = 1000;

/// 默认在新登记候选对象达到该数量后自动收集一次.
const DEFAULT_GC_THRESHOLD: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum TaskId {
    Main,
    Actor(u64),
}

struct ActorTask {
    machine: Machine,
    handle: Arc<ActorHandle>,
    wait: Option<Wait>,
}

/// 等待完成的 I/O 操作.
struct IoWaiter {
    task: TaskId,
    kind: IoKind,
}

/// 定时器最小堆元素; `id` 同时用作同截止时刻的稳定顺序与失效判定.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Timer {
    deadline: Instant,
    id: u64,
    task: TaskId,
    kind: TimerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TimerKind {
    Sleep,
    Receive,
}

/// 单线程调度器.
pub struct Scheduler {
    shared: Arc<Mutex<Shared>>,
    handle: SchedulerHandle,
    gc: Arc<GcRegistry>,
    actors: HashMap<u64, ActorTask>,
    main_machine: Machine,
    /// 启动后由主脚本与 Actor 共享的命令行参数.
    args: Vec<String>,
    main_wait: Option<Wait>,
    ready: VecDeque<TaskId>,
    join_waiters: HashMap<u64, Vec<TaskId>>,
    receive_waiters: HashMap<(u64, usize), TaskId>,
    io_waiters: HashMap<u64, IoWaiter>,
    /// 定时器最小堆; 已取消的条目延迟到堆顶时清理.
    timers: BinaryHeap<Reverse<Timer>>,
    /// 每个任务当前有效的定时器编号; 与堆内条目比对以忽略已取消的死条目.
    active_timers: HashMap<TaskId, u64>,
    next_timer_sequence: u64,
    main_result: Option<Result<Value, Error>>,
    /// 当前输入轮次; 每次 `run_program` 递增.
    current_epoch: u64,
    /// 当前输入轮次中启动的 Actor; 输入失败时统一终止.
    main_spawns: Vec<Arc<ActorHandle>>,
    /// 程序模式下由输入完成或输入驱动唤醒链加入的任务.
    input_driven: VecDeque<TaskId>,
    /// 是否正在执行 REPL 的输入驱动推进.
    pumping: bool,
    /// Actor 内的运行时错误; 共享观察标记以过滤已被观察的失败.
    actor_errors: Vec<(Arc<AtomicBool>, Error)>,
    /// 宿主或执行器在安全点需要保留的额外根.
    extra_roots: Vec<Vec<Value>>,
    /// 每个任务一次调度的指令预算; 测试可以调小以观察让出行为.
    slice_budget: usize,
    /// 新候选对象达到该数量时自动收集; 0 表示关闭自动收集.
    gc_threshold: usize,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    /// 新调度器: 空 Actor 集合与空主任务环境.
    #[must_use]
    pub fn new() -> Self {
        let gc = Arc::new(GcRegistry::new());
        let (handle, shared) = SchedulerHandle::new(Arc::clone(&gc));
        let mut main_machine = Machine::new();
        main_machine.set_scheduler(handle.clone());
        Self {
            shared,
            handle,
            gc,
            actors: HashMap::new(),
            main_machine,
            args: Vec::new(),
            main_wait: None,
            ready: VecDeque::new(),
            join_waiters: HashMap::new(),
            receive_waiters: HashMap::new(),
            io_waiters: HashMap::new(),
            timers: BinaryHeap::new(),
            active_timers: HashMap::new(),
            next_timer_sequence: 1,
            main_result: None,
            current_epoch: 0,
            main_spawns: Vec::new(),
            input_driven: VecDeque::new(),
            pumping: false,
            actor_errors: Vec::new(),
            extra_roots: Vec::new(),
            slice_budget: BUDGET,
            gc_threshold: DEFAULT_GC_THRESHOLD,
        }
    }

    /// 注入命令行参数, `Args` 内置函数读取.
    ///
    /// 参数在启动后由全部任务共享, Actor 内读取结果与主脚本一致.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.args.clone_from(&args);
        self.main_machine.set_args(args);
    }

    /// 开始一次根加载/输入, 递增 Actor 清理轮次.
    ///
    /// 上一轮成功时保留的 Actor 句柄在此丢弃, 但 Actor 本身继续运行;
    /// 失败回滚只针对当前轮次.
    pub fn begin_root(&mut self) {
        self.current_epoch += 1;
        self.main_machine.set_epoch(self.current_epoch);
        self.main_spawns.clear();
        self.actor_errors.clear();
    }

    /// 结束当前根轮次.
    ///
    /// 失败时终止本轮启动的全部 Actor, 避免留下无法再通过 Pid 访问的
    /// 孤立 Actor; 成功时丢弃失败回滚用的句柄. 观察标记随错误记录共享,
    /// 因此丢弃句柄不影响 `@Join` / `@Monitor` 的过滤.
    pub(crate) fn finish_root(&mut self, success: bool) {
        if success {
            self.main_spawns.clear();
        } else {
            self.abort_main_spawns();
        }
    }

    /// 替换主任务环境; 模块加载与 REPL 导出重绑定使用.
    pub(crate) fn set_globals(&mut self, env: crate::value::Env) {
        self.main_machine.set_globals(env);
    }

    #[must_use]
    pub(crate) fn globals(&self) -> &crate::value::Env {
        self.main_machine.globals()
    }

    /// 快照全局环境; 交互模式在输入失败时回滚到快照.
    #[must_use]
    pub fn snapshot_globals(&self) -> crate::value::Env {
        self.main_machine.globals().clone()
    }

    /// 恢复全局环境快照.
    pub fn restore_globals(&mut self, env: crate::value::Env) {
        self.main_machine.set_globals(env);
    }

    /// 压入一组额外根; 必须在同一安全点内成对弹出.
    ///
    /// 执行中途收集时, 调度器状态之外的 `Value` 只能通过这里保住,
    /// 例如模块导出值, 回滚快照与宿主自持的运行时值.
    pub fn push_extra_roots(&mut self, roots: Vec<Value>) {
        self.extra_roots.push(roots);
    }

    /// 压入环境中已初始化的值; 供回滚快照与模块基环境使用.
    pub fn push_extra_env(&mut self, env: &Env) {
        self.extra_roots.push(env.values().cloned().collect());
    }

    /// 弹出一组额外根.
    pub fn pop_extra_roots(&mut self) {
        self.extra_roots.pop();
    }

    /// 设置自动收集阈值; 0 表示关闭自动收集.
    pub fn set_gc_threshold(&mut self, threshold: usize) {
        self.gc_threshold = threshold;
    }

    /// 已执行的收集次数; 供测试确认自动触发.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn collection_count(&self) -> usize {
        self.gc.collection_count()
    }

    /// 当前仍存活的 Actor 任务数; 供测试确认回滚清理.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn actor_count(&self) -> usize {
        self.actors.len()
    }

    /// 调整单次调度的指令预算; 供测试缩小规模.
    #[cfg(test)]
    pub(crate) fn set_slice_budget(&mut self, budget: usize) {
        self.slice_budget = budget;
    }

    /// 新候选对象达到阈值时收集一次; 只在调度器循环的安全点调用.
    fn maybe_collect_cycles(&mut self) {
        if self.gc_threshold > 0 && self.gc.allocations_since_collection() >= self.gc_threshold
        {
            self.collect_cycles(&[]);
        }
    }

    /// 回收引用计数无法释放的环.
    ///
    /// 只能在安全点调用: 除 `external_roots` 与本调度器内部状态外,
    /// Rust 栈上不能还有额外的 `Value`, `Closure` 或 `ActorHandle`.
    /// REPL 在每次输入开始前调用, 此时上一条结果已经离开栈.
    pub fn collect_cycles(&mut self, external_roots: &[Value]) {
        let mut marker = Marker::new();
        self.trace_roots(&mut marker);
        for value in external_roots {
            marker.mark_value(value);
        }
        marker.run();
        self.gc.sweep(&marker);
    }

    fn trace_roots(&self, marker: &mut Marker) {
        for roots in &self.extra_roots {
            for value in roots {
                marker.mark_value(value);
            }
        }
        self.main_machine.trace(marker);
        if let Some(wait) = &self.main_wait {
            wait.trace(marker);
        }
        if let Some(Ok(value)) = &self.main_result {
            marker.mark_value(value);
        }
        for handle in &self.main_spawns {
            marker.mark_actor(handle);
        }
        for task in self.actors.values() {
            task.machine.trace(marker);
            marker.mark_actor(&task.handle);
            if let Some(wait) = &task.wait {
                wait.trace(marker);
            }
        }
        self.shared.lock().trace(marker);
    }

    /// 运行一段顶层程序; Actor 保留到调度器关闭或后续输入继续使用.
    ///
    /// 运行失败时全局绑定回滚到调用前的状态, 输入不留半程绑定.
    ///
    /// # Errors
    ///
    /// 主脚本的运行时错误或 `@Exit` 在此返回.
    pub fn run_program(
        &mut self,
        program: &Program,
        types: &crate::check::TypeTable,
    ) -> Result<Value, Error> {
        self.begin_root();
        let checkpoint = self.snapshot_globals();
        // 执行中途收集时, 回滚快照可能只存在于 Rust 栈上.
        self.push_extra_env(&checkpoint);
        let result = self.run_program_in_root(program, types);
        self.pop_extra_roots();
        self.finish_root(result.is_ok());
        if result.is_err() {
            self.restore_globals(checkpoint);
        }
        result
    }

    /// 在当前根轮次内运行一个程序; 不重置 Actor 清理记录.
    ///
    /// 供交互模式把模块执行与随后的输入求值放在同一轮内.
    ///
    /// # Errors
    ///
    /// 主脚本的运行时错误或 `@Exit` 在此返回.
    pub fn run_program_in_root(
        &mut self,
        program: &Program,
        types: &crate::check::TypeTable,
    ) -> Result<Value, Error> {
        self.main_machine.start_program(program, types);
        self.main_wait = None;
        self.main_result = None;
        self.ready.push_back(TaskId::Main);
        self.loop_run()
    }

    /// 终止所有 Actor; 批处理结束或 REPL 退出时调用.
    pub fn shutdown(&mut self) {
        self.kill_all();
    }

    /// 仍在等待标准输入的 I/O 操作数.
    #[must_use]
    pub fn pending_input(&self) -> usize {
        self.io_waiters
            .values()
            .filter(|waiter| waiter.kind == IoKind::Input)
            .count()
    }

    /// 记录一个由输入完成或输入驱动唤醒链加入的任务.
    fn mark_input_driven(&mut self, task: TaskId) {
        if !self.input_driven.contains(&task) {
            self.input_driven.push_back(task);
        }
    }

    /// 推进由标准输入完成驱动的 Actor 及其唤醒链.
    ///
    /// REPL 在提示符前调用. 只运行输入完成的任务和这些任务直接或间接
    /// 唤醒的任务; CPU 密集但没有输入关联的 Actor 留在 ready 队列里.
    /// 输入请求清空且本轮唤醒链结束后立即返回, 不等待文件 I/O 与未到期
    /// 的定时器.
    pub fn pump_program(&mut self) {
        self.pumping = true;
        self.input_driven.clear();
        loop {
            self.drain_spawns();
            self.drain_wakes();
            self.drain_io_results();
            self.prune_stale_timers();
            if !self.timers.is_empty() {
                self.fire_timers();
            }
            self.fire_send_timers();
            self.maybe_collect_cycles();
            if let Some(task) = self.input_driven.front().copied() {
                self.ready.retain(|queued| *queued != task);
                if let TaskId::Actor(id) = task {
                    self.actor_slice(id);
                }
                // 发送唤醒在任务仍属于输入驱动队列时落入队列,
                // 先处理它们, 再移除本任务.
                self.drain_wakes();
                self.input_driven.pop_front();
                continue;
            }
            if self.pending_input() == 0 || self.io_waiters.is_empty() {
                self.input_driven.clear();
                self.pumping = false;
                return;
            }
            // 输入线程不能唤醒调度器, 以短轮询等待输入到达.
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// 取走本轮到目前收集到的未观察 Actor 运行时错误; 宿主负责渲染.
    ///
    /// Actor 的错误不会终止主脚本, 因此单独收集而不是混入主任务结果.
    /// 已由 `@Join` 或 `@Monitor` 观察的失败不再重复报告; 观察标记与句柄
    /// 共享, 句柄释放后仍能正确过滤.
    #[must_use]
    pub fn take_actor_errors(&mut self) -> Vec<Error> {
        std::mem::take(&mut self.actor_errors)
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

    fn loop_run(&mut self) -> Result<Value, Error> {
        loop {
            self.drain_spawns();
            self.drain_wakes();
            self.drain_io_results();
            self.prune_stale_timers();
            if !self.timers.is_empty() {
                self.fire_timers();
            }
            self.fire_send_timers();
            if let Some(result) = self.main_result.take() {
                // Actor 清理由根轮次的调用方决定; 模块执行等中间步骤
                // 不能提前丢弃本轮更早步骤启动的 Actor.
                return result;
            }
            self.maybe_collect_cycles();
            if let Some(task) = self.ready.pop_front() {
                match task {
                    TaskId::Main => self.main_slice(),
                    TaskId::Actor(id) => self.actor_slice(id),
                }
                continue;
            }
            if !self.io_waiters.is_empty() {
                // I/O 工作线程不能唤醒调度器, 以短轮询等待完成.
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            if self.timers.is_empty() && !self.has_send_timers() {
                return Err(Error::runtime(ErrorKind::Deadlock, None));
            }
            self.sleep_until_next_timer();
        }
    }

    fn main_slice(&mut self) {
        match self.main_machine.run_budget(self.slice_budget) {
            Ok(RunOutcome::Finished(value)) => {
                self.main_result = Some(Ok(value));
            },
            Ok(RunOutcome::Yield) => {
                self.ready.push_back(TaskId::Main);
            },
            Ok(RunOutcome::Suspended(wait)) => {
                self.main_wait(wait);
            },
            Ok(RunOutcome::ActorExit(_)) => {
                self.main_result = Some(Err(Error::internal("主脚本不能终止 Actor")));
            },
            Err(error) => {
                self.main_result = Some(Err(error));
            },
        }
    }

    fn main_wait(&mut self, wait: Wait) {
        match wait {
            Wait::Join { target } => {
                if let Some(result) = target.result() {
                    self.resume_main(join_result_value(&result));
                } else {
                    self.join_waiters
                        .entry(target.id)
                        .or_default()
                        .push(TaskId::Main);
                    self.main_wait = Some(Wait::Join { target });
                }
            },
            Wait::Io { op_id, kind } => {
                self.io_waiters.insert(
                    op_id,
                    IoWaiter {
                        task: TaskId::Main,
                        kind,
                    },
                );
                self.main_wait = Some(Wait::Io { op_id, kind });
            },
            // 检查器已禁止主脚本使用 Receive/Sleep.
            Wait::Receive { .. } | Wait::Sleep { .. } => {
                self.main_result = Some(Err(Error::internal("主脚本不能挂起在 Actor 原语上")));
            },
        }
    }

    fn resume_main(&mut self, value: Value) {
        self.main_wait = None;
        self.main_machine.resume(value);
        self.ready.push_back(TaskId::Main);
    }

    fn actor_slice(&mut self, id: u64) {
        let Some(mut task) = self.actors.remove(&id) else {
            return;
        };
        if task.handle.kill_requested() {
            self.finish_actor(&task, &Err(ExitReason::Killed));
            return;
        }
        match task.machine.run_budget(self.slice_budget) {
            Ok(RunOutcome::Finished(value)) => {
                self.finish_actor(&task, &Ok(value));
            },
            Ok(RunOutcome::Yield) => {
                if task.handle.kill_requested() {
                    self.finish_actor(&task, &Err(ExitReason::Killed));
                } else {
                    self.actors.insert(id, task);
                    self.ready.push_back(TaskId::Actor(id));
                }
            },
            Ok(RunOutcome::Suspended(wait)) => {
                if task.handle.kill_requested() {
                    self.finish_actor(&task, &Err(ExitReason::Killed));
                } else {
                    self.actor_wait(id, task, wait);
                }
            },
            Ok(RunOutcome::ActorExit(reason)) => {
                self.finish_actor(&task, &Err(reason));
            },
            Err(error) => {
                self.actor_errors
                    .push((task.handle.observation_flag(), error.clone()));
                self.finish_actor(&task, &Err(error_to_exit(&error)));
            },
        }
    }

    fn actor_wait(&mut self, id: u64, mut task: ActorTask, wait: Wait) {
        match wait {
            Wait::Join { target } => {
                if let Some(result) = target.result() {
                    task.machine.resume(join_result_value(&result));
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.ready.push_back(TaskId::Actor(id));
                } else {
                    self.join_waiters
                        .entry(target.id)
                        .or_default()
                        .push(TaskId::Actor(id));
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
                    self.ready.push_back(TaskId::Actor(id));
                    return;
                }
                self.receive_waiters.insert((id, index), TaskId::Actor(id));
                if let Some(millis) = timeout
                    && millis > 0
                    && let Err(error) =
                        self.add_timer(millis, TaskId::Actor(id), TimerKind::Receive, span)
                {
                    self.receive_waiters.remove(&(id, index));
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.fail_task(TaskId::Actor(id), error);
                    return;
                }
                task.wait = Some(Wait::Receive {
                    index,
                    timeout,
                    span,
                });
                self.actors.insert(id, task);
            },
            Wait::Io { op_id, kind } => {
                self.io_waiters.insert(
                    op_id,
                    IoWaiter {
                        task: TaskId::Actor(id),
                        kind,
                    },
                );
                task.wait = Some(Wait::Io { op_id, kind });
                self.actors.insert(id, task);
            },
            Wait::Sleep { millis, span } => {
                if millis > 0 {
                    if let Err(error) =
                        self.add_timer(millis, TaskId::Actor(id), TimerKind::Sleep, span)
                    {
                        task.wait = None;
                        self.actors.insert(id, task);
                        self.fail_task(TaskId::Actor(id), error);
                        return;
                    }
                } else {
                    task.machine.resume(Value::Nil);
                    task.wait = None;
                    self.actors.insert(id, task);
                    self.ready.push_back(TaskId::Actor(id));
                    return;
                }
                task.wait = Some(Wait::Sleep { millis, span });
                self.actors.insert(id, task);
            },
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
        self.clear_wait(TaskId::Actor(id));
        if let Some(waiters) = self.join_waiters.remove(&id) {
            let value = join_result_value(result);
            for waiter in waiters {
                self.resume_task(waiter, value.clone());
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
        if let Some(task) = self.actors.remove(&id) {
            self.ready.retain(|task_id| *task_id != TaskId::Actor(id));
            task.handle.request_kill();
            if let Some(Wait::Io { op_id, .. }) = &task.wait {
                self.handle.cancel_io(*op_id);
                self.io_waiters.remove(op_id);
            }
            self.finish_actor(&task, &Err(ExitReason::Killed));
        }
    }

    /// 终止当前输入轮次启动的 Actor; 失败的 REPL 输入不应留下不可达的 Pid.
    fn abort_main_spawns(&mut self) {
        let spawns = std::mem::take(&mut self.main_spawns);
        for handle in spawns {
            handle.request_kill();
            if self.actors.contains_key(&handle.id) {
                self.kill_actor(handle.id);
            } else {
                self.finish_pending(&handle, ExitReason::Killed);
            }
        }
    }

    fn kill_all(&mut self) {
        if let Some(Wait::Io { op_id, .. }) = &self.main_wait {
            self.handle.cancel_io(*op_id);
        }
        let ids: Vec<u64> = self.actors.keys().copied().collect();
        for id in ids {
            self.kill_actor(id);
        }
        self.ready.clear();
        self.timers.clear();
        self.active_timers.clear();
        self.receive_waiters.clear();
        self.io_waiters.clear();
        self.join_waiters.clear();
        self.handle.clear_names();
        self.handle.clear_monitors();
        self.handle.clear_send_timers();
    }

    fn resume_task(&mut self, task: TaskId, value: Value) {
        match task {
            TaskId::Main => self.resume_main(value),
            TaskId::Actor(id) => {
                if let Some(mut task) = self.actors.remove(&id) {
                    self.clear_wait(TaskId::Actor(id));
                    task.wait = None;
                    task.machine.resume(value);
                    self.actors.insert(id, task);
                    self.ready.push_back(TaskId::Actor(id));
                }
            },
        }
    }

    fn fail_task(&mut self, task: TaskId, error: Error) {
        match task {
            TaskId::Main => {
                self.main_wait = None;
                self.main_machine.resume_error(error);
                self.ready.push_back(TaskId::Main);
            },
            TaskId::Actor(id) => {
                if let Some(mut task) = self.actors.remove(&id) {
                    self.clear_wait(TaskId::Actor(id));
                    task.wait = None;
                    task.machine.resume_error(error);
                    self.actors.insert(id, task);
                    self.ready.push_back(TaskId::Actor(id));
                }
            },
        }
    }

    fn clear_wait(&mut self, task: TaskId) {
        // 定时器条目留在堆中延迟清理; 移除有效编号后该条目即失效.
        self.active_timers.remove(&task);
        self.receive_waiters.retain(|_, waiter| *waiter != task);
        self.join_waiters.retain(|_, waiters| {
            waiters.retain(|waiter| *waiter != task);
            !waiters.is_empty()
        });
    }

    fn drain_spawns(&mut self) {
        let spawns = {
            let mut shared = self.shared.lock();
            std::mem::take(&mut shared.spawns)
        };
        for request in spawns {
            let id = request.handle.id;
            if request.epoch == self.current_epoch {
                self.main_spawns.push(Arc::clone(&request.handle));
            }
            // 父 Actor 可能在子 Actor 被调度前就已终止, 或子 Actor 已被 Kill;
            // 这两种情况下都不再启动子 Actor.
            let parent_finished = request
                .handle
                .parent
                .as_ref()
                .is_some_and(|parent| parent.is_finished());
            if request.handle.kill_requested() || parent_finished {
                self.finish_pending(&request.handle, ExitReason::Killed);
                continue;
            }
            match Machine::start_actor(
                request.handler,
                Arc::clone(&request.handle),
                request.args,
                self.args.clone(),
                self.handle.clone(),
                request.epoch,
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
                    self.ready.push_back(TaskId::Actor(id));
                },
                Err(error) => {
                    self.actor_errors
                        .push((request.handle.observation_flag(), error.clone()));
                    self.finish_pending(&request.handle, error_to_exit(&error));
                },
            }
        }
    }

    fn drain_io_results(&mut self) {
        let results = {
            let mut shared = self.shared.lock();
            std::mem::take(&mut shared.io_results)
        };
        for (id, result) in results {
            let Some(waiter) = self.io_waiters.remove(&id) else {
                continue;
            };
            if self.pumping && waiter.kind == IoKind::Input {
                self.mark_input_driven(waiter.task);
            }
            match result {
                Ok(value) => self.resume_task(waiter.task, value),
                Err(error) => self.fail_task(waiter.task, error),
            }
        }
    }

    fn drain_wakes(&mut self) {
        let wakes = {
            let mut shared = self.shared.lock();
            std::mem::take(&mut shared.wakes)
        };
        for wake in wakes {
            match wake {
                Wake::Send {
                    source,
                    target,
                    index,
                } => self.wake_send(source, target, index),
                Wake::Kill { target } => self.kill_actor(target),
            }
        }
    }

    fn wake_send(&mut self, source: Option<u64>, target: u64, index: usize) {
        let input_source = self.pumping
            && source.is_some_and(|source| self.input_driven.contains(&TaskId::Actor(source)));
        let Some(waiter) = self.receive_waiters.remove(&(target, index)) else {
            if input_source && self.actors.get(&target).is_some_and(|task| task.wait.is_none())
            {
                self.mark_input_driven(TaskId::Actor(target));
            }
            return;
        };
        let TaskId::Actor(id) = waiter else {
            return;
        };
        if let Some(mut task) = self.actors.remove(&id) {
            self.clear_wait(TaskId::Actor(id));
            let value = task
                .handle
                .try_recv(index)
                .map_or_else(option_none, option_some);
            task.wait = None;
            task.machine.resume(value);
            self.actors.insert(id, task);
            self.ready.push_back(TaskId::Actor(id));
            if input_source {
                self.mark_input_driven(TaskId::Actor(id));
            }
        }
    }

    fn add_timer(
        &mut self,
        millis: u64,
        task: TaskId,
        kind: TimerKind,
        span: Span,
    ) -> Result<(), Error> {
        let duration = Duration::from_millis(millis);
        let Some(deadline) = Instant::now().checked_add(duration) else {
            return Err(Error::runtime(ErrorKind::TimeoutOutOfRange, Some(span)));
        };
        let id = self.next_timer_sequence;
        self.next_timer_sequence += 1;
        self.active_timers.insert(task, id);
        self.timers.push(Reverse(Timer {
            deadline,
            id,
            task,
            kind,
        }));
        Ok(())
    }

    /// 清理堆顶已取消的定时器; 更深的死条目留到成为堆顶时再处理.
    fn prune_stale_timers(&mut self) {
        while let Some(Reverse(timer)) = self.timers.peek().copied() {
            if self.active_timers.get(&timer.task) == Some(&timer.id) {
                break;
            }
            self.timers.pop();
        }
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        self.prune_stale_timers();
        let mut due = Vec::new();
        while let Some(Reverse(timer)) = self.timers.peek().copied() {
            if timer.deadline > now {
                break;
            }
            self.timers.pop();
            if self.active_timers.get(&timer.task) == Some(&timer.id) {
                self.active_timers.remove(&timer.task);
                due.push((timer.task, timer.kind));
            }
        }
        for (task, kind) in due {
            match task {
                TaskId::Main => {
                    self.main_result = Some(Err(Error::internal("主脚本不能挂起在计时器上")));
                },
                TaskId::Actor(id) => {
                    let value = match kind {
                        TimerKind::Sleep => Value::Nil,
                        TimerKind::Receive => option_none(),
                    };
                    self.resume_task(TaskId::Actor(id), value);
                },
            }
        }
    }

    fn fire_send_timers(&mut self) {
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
                self.wake_send(None, timer.target_id, timer.index);
            }
        }
    }

    fn has_send_timers(&self) -> bool {
        !self.shared.lock().send_timers.is_empty()
    }

    fn sleep_until_next_timer(&mut self) {
        self.prune_stale_timers();
        let send_deadline = self.shared.lock().next_send_deadline();
        let deadline = self
            .timers
            .peek()
            .map(|Reverse(timer)| timer.deadline)
            .into_iter()
            .chain(send_deadline)
            .min();
        let Some(deadline) = deadline else {
            return;
        };
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
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

#[cfg(test)]
mod tests;
