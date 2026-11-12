//! 协作式调度器.
//!
//! 顶层语句是 Actor 之外的 Prelude, 与 Actor 由协调器按指令预算轮转.
//! 全部片段在调度线程内联执行; 平台事件泵在片段之间轮询.
//! 调度器持有就绪队列, Actor 表, 定时器与等待表. I/O 完成与 Actor 原语经统一事件队列回到调度器.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crossbeam_channel::unbounded;
use ksl_frontend::ast::{Expr, Program};
use ksl_frontend::error::{ErrorKind, RuntimeError as Error};
use parking_lot::Mutex;

use super::gc::{GcRegistry, Marker};
use super::pump::PumpRegistry;
use crate::io::{IoConfig, IoRuntime};
use crate::root::RootSet;
use crate::value::{Env, Value};

mod coordinator;
mod ingress;

use coordinator::Coordinator;
pub(crate) use ingress::SchedulerHandle;

/// 每个任务一次调度的指令预算.
const BUDGET: usize = 1000;

/// 根作用域登记表; 编号用于乱序释放时定位.
type RootEntries = Vec<(u64, RootSet)>;

/// 默认在新登记候选对象达到该数量后自动收集一次.
const DEFAULT_GC_THRESHOLD: usize = 4096;

/// 协作式调度器.
///
/// 对外是宿主使用的门面; 任务状态由 `Coordinator` 持有.
pub struct Scheduler {
    handle: SchedulerHandle,
    gc: Arc<GcRegistry>,
    coordinator: Coordinator,
    /// 调度线程释放投递端; 工作线程用它交回释放任务.
    releases: Arc<crate::ffi::ReleaseQueue>,
    /// 调度线程释放接收端; 只在本调度线程排空.
    release_drain: crate::ffi::ReleaseDrain,
    /// 宿主在安全点需要保留的额外根; 根作用域用 RAII 登记.
    extra_roots: Arc<Mutex<RootEntries>>,
    /// 根作用域编号.
    next_root: AtomicU64,
    /// 新候选对象达到该数量时自动收集; 0 表示关闭自动收集.
    gc_threshold: Arc<AtomicUsize>,
    /// 平台事件泵; 由调度循环按间隔轮询.
    pumps: Arc<PumpRegistry>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    /// 新调度器; 使用默认 I/O 配置.
    #[must_use]
    pub fn new() -> Self {
        Self::with_io(Arc::new(IoRuntime::new(IoConfig::default())))
    }

    /// 以给定 I/O 运行时建立调度器; 供宿主注入命令行配置.
    #[must_use]
    pub(crate) fn with_io(io: Arc<IoRuntime>) -> Self {
        let gc = Arc::new(GcRegistry::new());
        let slice_budget = Arc::new(AtomicUsize::new(BUDGET));
        let gc_threshold = Arc::new(AtomicUsize::new(DEFAULT_GC_THRESHOLD));
        let (events, inbox) = unbounded();
        let (releases, release_drain) = crate::ffi::ReleaseQueue::new();
        let releases = Arc::new(releases);
        releases.attach(events.clone());
        let pumps = Arc::new(PumpRegistry::new(true));
        let (handle, ingress) = SchedulerHandle::new(Arc::clone(&gc), io, events);
        let coordinator = Coordinator::new(
            handle.clone(),
            ingress,
            inbox,
            Arc::clone(&slice_budget),
            Arc::clone(&releases),
        );
        Self {
            handle,
            gc,
            coordinator,
            releases,
            release_drain,
            extra_roots: Arc::new(Mutex::new(Vec::new())),
            next_root: AtomicU64::new(1),
            gc_threshold,
            pumps,
        }
    }

    /// 注入命令行参数, `Args` 内置函数读取.
    ///
    /// 参数在启动后由全部任务共享, Actor 内读取结果与 Prelude 一致.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.handle.set_args(args.clone());
        self.coordinator.set_args(args);
    }

    /// 替换 Prelude 环境; 模块加载与 REPL 导出重绑定使用.
    pub(crate) fn set_globals(&mut self, env: Env) {
        self.coordinator.set_globals(env);
    }

    #[must_use]
    pub(crate) fn globals(&self) -> &Env {
        self.coordinator.globals()
    }

    /// 快照 Prelude 全局环境; 交互模式在输入失败时回滚到快照.
    ///
    /// 快照在回滚前只由返回的检查点持有; 检查点自带根作用域,
    /// 执行中途的环回收不会清空其中闭包的捕获.
    #[must_use]
    pub fn snapshot_globals(&self) -> GlobalsCheckpoint {
        let env = self.coordinator.globals().clone();
        let root = self.root_env_scope(&env);
        GlobalsCheckpoint { env, root }
    }

    /// 恢复 Prelude 全局环境快照.
    pub fn restore_globals(&mut self, checkpoint: GlobalsCheckpoint) {
        let GlobalsCheckpoint { env, root } = checkpoint;
        self.coordinator.set_globals(env);
        drop(root);
    }

    /// 登记一组额外根; 守卫释放时自动注销.
    ///
    /// 执行中途收集时, 调度器状态之外的 `Value` 只能通过根作用域保住,
    /// 例如模块导出值, 回滚快照与宿主自持的运行时值.
    pub(crate) fn root_scope(&self, roots: impl Into<RootSet>) -> RootGuard {
        let roots = roots.into();
        if roots.is_empty() {
            return RootGuard {
                roots: Arc::clone(&self.extra_roots),
                id: None,
            };
        }
        let id = self.next_root.fetch_add(1, Ordering::Relaxed);
        self.extra_roots.lock().push((id, roots));
        RootGuard {
            roots: Arc::clone(&self.extra_roots),
            id: Some(id),
        }
    }

    /// 登记环境中已初始化的值.
    pub(crate) fn root_env_scope(&self, env: &Env) -> RootGuard {
        self.root_scope(RootSet::from_env(env))
    }

    /// 设置自动收集阈值下限; 实际触发阈值随存活候选规模自适应上调.
    /// 0 表示关闭自动收集.
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
        self.coordinator.set_slice_budget(budget);
    }

    fn should_collect(&self) -> bool {
        let threshold = self.gc_threshold.load(Ordering::Relaxed);
        if threshold == 0 {
            return false;
        }
        // 阈值随存活候选规模上调: 标记成本正比于存活对象总数,
        // 按存活数比例触发, 使每个新候选分摊的标记成本保持为常数.
        let adaptive = (self.gc.live_candidates() / 4).max(threshold);
        self.gc.allocations_since_collection() >= adaptive
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
    /// 调度循环不在推进片段时, 全部机器都在任务表中; 在途 I/O 任务不携带可成环的值.
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
        self.coordinator.trace(marker);
    }

    /// 宿主额外根的标记; 任务根由协调器遍历.
    fn trace_host_roots(&self, marker: &mut Marker) {
        let roots = self.extra_roots.lock();
        for (_, values) in roots.iter() {
            marker.mark_roots(values);
        }
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
        types: &ksl_frontend::check::TypeTable,
    ) -> Result<Value, Error> {
        self.run_program_with_main(program, types, None, None)
    }

    /// 运行程序, 并在给出入口声明时求值入口并 spawn 根 Actor.
    ///
    /// `main` 是已从程序语句中抽出的入口声明; 入口体在程序执行后单独求值,
    /// 避免调度器再从程序尾部识别 `@Main`.
    ///
    /// # Errors
    ///
    /// 顶层 Prelude 的运行时错误, 入口求值错误或 `@Exit` 在此返回.
    pub fn run_program_with_main(
        &mut self,
        program: &Program,
        types: &ksl_frontend::check::TypeTable,
        main: Option<&Expr>,
        mailboxes: Option<usize>,
    ) -> Result<Value, Error> {
        let checkpoint = self.snapshot_globals();
        let mut result = self.run_program_in_root(program, types);
        if result.is_ok()
            && let Some(main) = main
        {
            let entry = Program {
                stmts: Vec::new(),
                tail: Some(main.clone()),
            };
            result = self.run_program_in_root(&entry, types);
            if let Ok(handler) = result {
                result = match mailboxes {
                    Some(mailboxes) => self.run_main(handler, mailboxes),
                    None => Err(Error::internal("@Main 缺少信箱数")),
                };
            }
        }
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
        types: &ksl_frontend::check::TypeTable,
    ) -> Result<Value, Error> {
        self.coordinator.start_prelude(program, types);
        self.loop_run()
    }

    /// 以 `@Main` 的 Handler 启动根 Actor, 运行到根结束.
    ///
    /// 根 Actor 走普通 Actor 任务路径.
    /// 它的正常返回是程序结果, `@Exit` 是程序退出, 其他运行时错误终止程序.
    /// 根的子 Actor 随根结束终止.
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
        let Value::Pid(_) = self.coordinator.spawn_root(mailboxes, handler, args) else {
            return Err(Error::internal("Main 的启动结果不是 Pid"));
        };
        self.loop_run()
    }

    /// 终止所有任务; 批处理结束或 REPL 退出时调用.
    pub fn shutdown(&mut self) {
        self.coordinator.clear_prelude();
        self.coordinator.kill_all();
        self.handle.clear_runtime_state();
        // 终止任务时丢弃的值可能已排入释放任务; 在调度线程执行完毕.
        self.drain_releases();
    }

    /// 调度线程释放队列; 模块执行构造外来值时随句柄一起携带.
    #[must_use]
    pub(crate) fn releases(&self) -> &Arc<crate::ffi::ReleaseQueue> {
        &self.releases
    }

    /// 事件泵注册表; 模块加载时注册.
    #[must_use]
    pub(crate) fn pumps(&self) -> &Arc<PumpRegistry> {
        &self.pumps
    }

    /// 交互会话不注册事件泵: 它会把控制权交给插件, 与终端输入冲突.
    pub(crate) fn set_pumps_enabled(&mut self, enabled: bool) {
        self.pumps.set_enabled(enabled);
    }

    /// 取走本轮到目前收集到的未观察 Actor 运行时错误; 宿主负责渲染.
    ///
    /// Actor 的错误不会终止 Prelude, 因此单独收集而不是混入 Prelude 结果.
    /// 已由 `@Join` 或 `@Monitor` 观察的失败不再重复报告.
    /// 观察标记与句柄共享, 句柄释放后仍能正确过滤.
    #[must_use]
    pub fn take_actor_errors(&mut self) -> Vec<Error> {
        self.handle.take_actor_errors()
    }

    /// 调度器主循环: Prelude 与 Actor 按预算轮转.
    fn loop_run(&mut self) -> Result<Value, Error> {
        loop {
            self.coordinator.drain_events();
            self.drain_releases();
            self.coordinator.fire_timers();
            self.coordinator.fire_send_timers();
            self.pumps.tick();

            if self.coordinator.program_finished() {
                return self
                    .coordinator
                    .take_program_result()
                    .unwrap_or_else(|| Err(Error::internal("程序结局丢失")));
            }

            self.maybe_collect_cycles();

            if self.coordinator.prelude_ready() {
                self.coordinator.run_prelude();
                continue;
            }

            if self.coordinator.run_next() {
                continue;
            }

            let has_io = self.coordinator.has_io_waiters();
            let has_timers =
                self.coordinator.has_timers() || self.coordinator.has_send_timers();
            let has_events =
                self.coordinator.has_pending_events() || !self.release_drain.is_empty();
            if !has_io && !has_timers && !has_events && !self.pumps.is_active() {
                return Err(Error::runtime(ErrorKind::Deadlock, None));
            }
            self.coordinator.wait_for_next_event(self.pump_wait());
        }
    }

    /// 排空调度线程释放队列; 任务只在调度线程执行.
    fn drain_releases(&mut self) {
        for job in self.release_drain.drain() {
            job();
        }
    }

    /// 事件泵存在时的等待上限; 没有事件泵时不限制.
    fn pump_wait(&self) -> Option<std::time::Duration> {
        self.pumps.is_active().then_some(super::pump::INTERVAL)
    }
}

/// Prelude 全局环境快照; 持有回滚用的环境与对应的根作用域.
pub struct GlobalsCheckpoint {
    env: Env,
    /// 快照值在回滚前只由本对象持有; 守卫保证收集时不回收其闭包捕获.
    root: RootGuard,
}

/// 一组额外根的守卫; 释放时自动注销.
pub(crate) struct RootGuard {
    roots: Arc<Mutex<RootEntries>>,
    /// 登记编号; 空根集合不登记.
    id: Option<u64>,
}

impl Drop for RootGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.roots.lock().retain(|(entry, _)| *entry != id);
        }
    }
}

#[cfg(test)]
mod tests;
