//! 协作式调度器.
//!
//! 顶层语句是 Actor 之外的 Prelude, 与 Actor 在同一线程上按指令预算轮转.
//! 调度器持有就绪队列, Actor 表, 定时器与等待表; I/O 完成与 Actor 原语经
//! 统一事件队列回到调度器, 由调度循环推进.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crossbeam_channel::bounded;
use parking_lot::Mutex;

use super::exec::{Machine, RunOutcome, Wait};
use super::gc::{GcRegistry, Marker};
use crate::ast::{ExprKind, Program, Special};
use crate::error::{ErrorKind, RuntimeError as Error};
use crate::value::{Env, Value};

mod shard;
mod shared;
mod timer_heap;

use shard::Shard;
pub(crate) use shared::SchedulerHandle;
use shared::Shared;

/// 每个任务一次调度的指令预算.
const BUDGET: usize = 1000;

/// 默认在新登记候选对象达到该数量后自动收集一次.
const DEFAULT_GC_THRESHOLD: usize = 4096;

/// 协作式调度器.
pub struct Scheduler {
    shared: Arc<Mutex<Shared>>,
    handle: SchedulerHandle,
    gc: Arc<GcRegistry>,
    shard: Shard,
    /// 顶层 Prelude 的抽象机; 它不是 Actor, 不进入 Actor 就绪队列.
    main_machine: Machine,
    main_wait: Option<Wait>,
    main_ready: bool,
    main_result: Option<Result<Value, Error>>,
    /// 宿主或执行器在安全点需要保留的额外根.
    extra_roots: Vec<Vec<Value>>,
    /// Prelude 任务每次调度的指令预算.
    slice_budget: Arc<AtomicUsize>,
    /// 新候选对象达到该数量时自动收集; 0 表示关闭自动收集.
    gc_threshold: Arc<AtomicUsize>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    /// 新调度器.
    #[must_use]
    pub fn new() -> Self {
        let gc = Arc::new(GcRegistry::new());
        let slice_budget = Arc::new(AtomicUsize::new(BUDGET));
        let gc_threshold = Arc::new(AtomicUsize::new(DEFAULT_GC_THRESHOLD));
        let (wake, wake_rx) = bounded(1);
        let (handle, shared) = SchedulerHandle::new(Arc::clone(&gc), wake);
        let mut main_machine = Machine::new();
        main_machine.set_scheduler(handle.clone());
        let runtime = Shard::new(
            handle.clone(),
            Arc::clone(&shared),
            wake_rx,
            Arc::clone(&slice_budget),
        );
        Self {
            shared,
            handle,
            gc,
            shard: runtime,
            main_machine,
            main_wait: None,
            main_ready: false,
            main_result: None,
            extra_roots: Vec::new(),
            slice_budget,
            gc_threshold,
        }
    }

    /// 注入命令行参数, `Args` 内置函数读取.
    ///
    /// 参数在启动后由全部任务共享, Actor 内读取结果与 Prelude 一致.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.handle.set_args(args.clone());
        self.main_machine.set_args(args);
    }

    /// 替换 Prelude 环境; 模块加载与 REPL 导出重绑定使用.
    pub(crate) fn set_globals(&mut self, env: Env) {
        self.main_machine.set_globals(env);
    }

    #[must_use]
    pub(crate) fn globals(&self) -> &Env {
        self.main_machine.globals()
    }

    /// 快照 Prelude 全局环境; 交互模式在输入失败时回滚到快照.
    #[must_use]
    pub fn snapshot_globals(&self) -> Env {
        self.main_machine.globals().clone()
    }

    /// 恢复 Prelude 全局环境快照.
    pub fn restore_globals(&mut self, env: Env) {
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
        self.gc_threshold.store(threshold, Ordering::Relaxed);
    }

    /// 已执行的收集次数; 供测试确认自动触发.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn collection_count(&self) -> usize {
        self.gc.collection_count()
    }

    /// 调整单次调度的指令预算; 供测试缩小规模.
    #[cfg(test)]
    pub(crate) fn set_slice_budget(&mut self, budget: usize) {
        self.slice_budget.store(budget, Ordering::Relaxed);
    }

    fn should_collect(&self) -> bool {
        let threshold = self.gc_threshold.load(Ordering::Relaxed);
        threshold > 0 && self.gc.allocations_since_collection() >= threshold
    }

    /// 新候选对象达到阈值时收集一次; 只在调度器循环的安全点调用.
    fn maybe_collect_cycles(&mut self) {
        if self.should_collect() {
            self.collect_cycles(&[]);
        }
    }

    /// 回收引用计数无法释放的环.
    ///
    /// 只能在安全点调用: 除 `external_roots` 与本调度器内部状态外,
    /// Rust 栈上不能还有额外的 `Value`, `Closure` 或 `ActorHandle`.
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
        self.trace_host_roots(marker);
        self.shard.trace(marker);
    }

    /// Prelude 与共享状态的根.
    fn trace_host_roots(&self, marker: &mut Marker) {
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
        self.shared.lock().trace(marker);
    }

    /// 运行一段顶层程序; Actor 保留到调度器关闭或后续输入继续使用.
    ///
    /// 运行失败时全局绑定回滚到调用前的状态, 输入不留半程绑定.
    ///
    /// # Errors
    ///
    /// 顶层 Prelude 的运行时错误或 `@Exit` 在此返回.
    pub fn run_program(
        &mut self,
        program: &Program,
        types: &crate::check::TypeTable,
    ) -> Result<Value, Error> {
        let checkpoint = self.snapshot_globals();
        // 执行中途收集时, 回滚快照可能只存在于 Rust 栈上.
        self.push_extra_env(&checkpoint);
        let has_main = program
            .tail
            .as_ref()
            .is_some_and(|tail| matches!(&tail.node, ExprKind::Special(Special::Main { .. })));
        let mut result = self.run_program_in_root(program, types);
        if has_main && let Ok(handler) = result {
            result = match crate::check::main_mailboxes(program, types) {
                Some(mailboxes) => self.run_main(handler, mailboxes),
                None => Err(Error::internal("@Main 缺少信箱数")),
            };
        }
        self.pop_extra_roots();
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
    /// 顶层 Prelude 的运行时错误或 `@Exit` 在此返回.
    pub fn run_program_in_root(
        &mut self,
        program: &Program,
        types: &crate::check::TypeTable,
    ) -> Result<Value, Error> {
        self.main_machine.start_program(program, types);
        self.main_wait = None;
        self.main_result = None;
        self.main_ready = true;
        self.loop_run()
    }

    /// 以 `@Main` 的 Handler 启动根 Actor, 运行到根结束.
    ///
    /// 根 Actor 走普通 Actor 任务路径; 它的正常返回是程序结果, `@Exit`
    /// 是程序退出, 其他运行时错误终止程序. 根的子 Actor 随根结束终止.
    ///
    /// # Errors
    ///
    /// 根 Actor 的运行时错误或 `@Exit`.
    pub(crate) fn run_main(
        &mut self,
        handler: Value,
        mailboxes: usize,
    ) -> Result<Value, Error> {
        self.run_main_with_args(handler, mailboxes, Vec::new())
    }

    /// 以指定实参启动根 Actor; 供测试把外部 Pid 注入 Handler.
    pub(crate) fn run_main_with_args(
        &mut self,
        handler: Value,
        mailboxes: usize,
        args: Vec<Value>,
    ) -> Result<Value, Error> {
        let Value::Pid(_) = self.handle.spawn_root(mailboxes, handler, args) else {
            return Err(Error::internal("Main 的启动结果不是 Pid"));
        };
        self.main_wait = None;
        self.main_ready = false;
        self.main_result = None;
        self.loop_run()
    }

    /// 终止所有 Actor; 批处理结束或 REPL 退出时调用.
    pub fn shutdown(&mut self) {
        if let Some(Wait::Io { op_id }) = &self.main_wait {
            self.handle.cancel_io(*op_id);
        }
        self.main_wait = None;
        self.main_ready = false;
        self.main_result = None;
        self.shard.kill_all();
        self.handle.clear_runtime_state();
    }

    /// 取走本轮到目前收集到的未观察 Actor 运行时错误; 宿主负责渲染.
    ///
    /// Actor 的错误不会终止 Prelude, 因此单独收集而不是混入 Prelude 结果.
    /// 已由 `@Join` 或 `@Monitor` 观察的失败不再重复报告; 观察标记与句柄
    /// 共享, 句柄释放后仍能正确过滤.
    #[must_use]
    pub fn take_actor_errors(&mut self) -> Vec<Error> {
        self.handle.take_actor_errors()
    }

    /// 调度器主循环: Prelude 与 Actor 在同一线程上按预算轮转.
    fn loop_run(&mut self) -> Result<Value, Error> {
        loop {
            self.shard.drain_events();
            self.drain_prelude_io();
            self.shard.fire_timers();
            self.shard.fire_send_timers();
            if let Some(result) = self.main_result.take() {
                return result;
            }
            if let Some(result) = self.handle.take_root_result() {
                return result;
            }
            self.maybe_collect_cycles();
            if self.main_ready {
                self.main_ready = false;
                self.main_slice();
                continue;
            }
            if let Some(id) = self.shard.pop_ready() {
                self.shard.actor_slice(id);
                continue;
            }
            let has_io = self.main_wait.is_some() || self.shard.has_io_waiters();
            let has_timers = self.shard.has_timers() || self.handle.has_send_timers();
            let has_events = self.handle.has_pending_events();
            if !has_io && !has_timers && !has_events {
                return Err(Error::runtime(ErrorKind::Deadlock, None));
            }
            self.shard.wait_for_next_event();
        }
    }

    fn drain_prelude_io(&mut self) {
        let results = self.handle.take_prelude_io();
        if results.is_empty() {
            return;
        }
        let Some(Wait::Io { op_id }) = self.main_wait.as_ref() else {
            return;
        };
        let op_id = *op_id;
        for (id, result) in results {
            if id != op_id {
                continue;
            }
            self.main_wait = None;
            match result {
                Ok(value) => self.main_machine.resume(value),
                Err(error) => self.main_machine.resume_error(error),
            }
            self.main_ready = true;
            return;
        }
    }

    fn main_slice(&mut self) {
        let budget = self.slice_budget.load(Ordering::Relaxed);
        match self.main_machine.run_budget(budget) {
            Ok(RunOutcome::Finished(value)) => {
                self.main_result = Some(Ok(value));
            },
            Ok(RunOutcome::Yield) => {
                self.main_ready = true;
            },
            Ok(RunOutcome::Suspended(wait)) => {
                self.main_wait(&wait);
            },
            Ok(RunOutcome::ActorExit(_)) => {
                self.main_result = Some(Err(Error::internal("Prelude 不能终止 Actor")));
            },
            Err(error) => {
                self.main_result = Some(Err(error));
            },
        }
    }

    fn main_wait(&mut self, wait: &Wait) {
        match wait {
            Wait::Io { op_id } => {
                self.main_wait = Some(Wait::Io { op_id: *op_id });
            },
            // 检查器已禁止 Prelude 使用 Receive/Join/Sleep.
            Wait::Join { .. } | Wait::Receive { .. } | Wait::Sleep { .. } => {
                self.main_result =
                    Some(Err(Error::internal("Prelude 不能挂起在 Actor 原语上")));
            },
        }
    }
}

#[cfg(test)]
mod tests;
