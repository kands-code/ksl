//! 执行线性 CPS IR 的抽象机.
//!
//! 控制状态是基本块, 指令下标, 槽位与捕获; 函数返回走续延帧,
//! Actor 与 I/O 挂起即保存该状态. 列表与 Option 的高阶操作沿用续延帧,
//! 不依赖 Rust 原生栈.

use std::sync::Arc;

use super::gc::{GcRegistry, Marker};
use super::ir::PrimOp;
use super::linear::{BlockId, Function as LinearFunction, Functions as LinearFunctions};
use super::scheduler::SchedulerHandle;

mod actor;
mod collection;
mod eval;
mod linear;
mod list;
mod monitor;
mod option;
mod registry;
mod timer;

use eval::describe;

use crate::ast::{ListOpKind, OptionOpKind};
use crate::builtin;
use crate::error::{ErrorKind, RuntimeError as Error};
use crate::source::Span;
use crate::value::{
    ActorHandle, Capture, Env, ExitReason, Key, Value, join_result_value, option_none,
    option_some, result_ok,
};

/// 调用/原语的结果续延帧.
struct LinearFrame {
    function: Arc<LinearFunction>,
    target: BlockId,
    /// 目标块除结果参数外的其余参数值.
    live: Vec<Value>,
    captures: Arc<[Capture]>,
    functions: Arc<LinearFunctions>,
}

/// 执行状态: 当前函数, 块, 指令下标, 槽位与捕获.
struct LinearState {
    function: Arc<LinearFunction>,
    block: BlockId,
    instr: usize,
    locals: Vec<Option<Value>>,
    captures: Arc<[Capture]>,
    functions: Arc<LinearFunctions>,
}

/// 一次调度片段的结局.
pub(crate) enum RunOutcome {
    Finished(Value),
    Suspended(Wait),
    ActorExit(ExitReason),
    Yield,
}

/// Actor 原语的挂起原因.
pub(crate) enum Wait {
    Receive {
        index: usize,
        timeout: Option<u64>,
        span: Span,
    },
    Join {
        target: Arc<ActorHandle>,
    },
    Sleep {
        millis: u64,
        span: Span,
    },
    Io {
        op_id: u64,
    },
}

enum StepResult {
    Continue,
    Yield,
    Finished(Value),
    Suspended(Wait),
    ActorExit(ExitReason),
}

/// 原语实参游标: 统一参数形态检查与错误报告.
struct PrimArgs {
    args: std::vec::IntoIter<Value>,
    span: Span,
}

impl PrimArgs {
    fn new(args: Vec<Value>, span: Span) -> Self {
        Self {
            args: args.into_iter(),
            span,
        }
    }

    fn next(&mut self) -> Result<Value, Error> {
        self.args
            .next()
            .ok_or_else(|| Error::internal("内置操作参数不足"))
    }

    fn list(&mut self) -> Result<Arc<crate::value::List>, Error> {
        match self.next()? {
            Value::List(list) => Ok(list),
            _ => Err(Error::internal("列表操作的实参不是列表")),
        }
    }

    fn count(&mut self) -> Result<usize, Error> {
        match self.next()? {
            Value::U64(value) => usize::try_from(value)
                .map_err(|_| Error::runtime(ErrorKind::CountOutOfRange, Some(self.span))),
            _ => Err(Error::internal("计数的实参不是 u64")),
        }
    }

    fn u8_count(&mut self) -> Result<usize, Error> {
        match self.next()? {
            Value::U8(value) => Ok(usize::from(value)),
            _ => Err(Error::internal("信箱计数的实参不是 u8")),
        }
    }

    fn pid(&mut self, what: &str) -> Result<Arc<ActorHandle>, Error> {
        match self.next()? {
            Value::Pid(pid) => Ok(pid),
            _ => Err(Error::internal(format!("{what} 的目标不是 Pid"))),
        }
    }

    fn remaining(self) -> Vec<Value> {
        self.args.collect()
    }
}

/// 高阶列表操作的迭代状态.
struct ListLoop {
    op: ListOpKind,
    function: Value,
    list: crate::value::List,
    /// 第一个输入列表的当前下标.
    pos: usize,
    /// `ZipWith` 的第二个输入列表; 其余操作为 None.
    second: Option<crate::value::List>,
    /// `ZipWith` 第二个输入列表的当前下标.
    pos2: usize,
    current: Option<Value>,
    /// `ZipWith` 的第二个当前元素.
    current2: Option<Value>,
    /// `MapIndexed` 的当前下标.
    index: u64,
    kept: crate::value::List,
    acc: Value,
    span: Span,
}

struct ActorRuntime {
    pid: Arc<ActorHandle>,
    handler: Value,
    base_kont: usize,
}

struct DictFilterLoop {
    entries: Vec<(Key, Value)>,
    index: usize,
    predicate: Value,
    kept: imbl::OrdMap<Key, Value>,
    span: Span,
}

struct SetFilterLoop {
    items: Vec<Key>,
    index: usize,
    predicate: Value,
    kept: imbl::OrdSet<Key>,
    span: Span,
}

enum Kont {
    /// 调用/原语的结果续延.
    Linear(Box<LinearFrame>),
    ListLoop(Box<ListLoop>),
    DictFilter(Box<DictFilterLoop>),
    SetFilter(Box<SetFilterLoop>),
    OptionMap {
        op: OptionOpKind,
    },
}

enum Control {
    Run(LinearState),
    Return(Value),
    Suspend(Wait),
    Fail(Error),
    ActorExit(ExitReason),
    Halt,
}

/// 线性 IR 抽象机: 持久化全局环境加当前执行状态.
pub struct Machine {
    globals: Env,
    args: Vec<String>,
    control: Control,
    kont: Vec<Kont>,
    actor: Option<ActorRuntime>,
    scheduler: Option<SchedulerHandle>,
    gc: Arc<GcRegistry>,
    /// 槽位缓冲池: 基本块切换时回收复用, 避免每次进入块都重新分配.
    locals_pool: Vec<Vec<Option<Value>>>,
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}

impl Machine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            globals: Env::new(),
            args: Vec::new(),
            control: Control::Halt,
            kont: Vec::new(),
            actor: None,
            scheduler: None,
            gc: Arc::new(GcRegistry::new()),
            locals_pool: Vec::new(),
        }
    }

    pub(crate) fn set_scheduler(&mut self, scheduler: SchedulerHandle) {
        self.gc = Arc::clone(scheduler.registry());
        self.scheduler = Some(scheduler);
    }

    /// 恢复挂起的机器: 把挂起原语的结果交回续延.
    pub(crate) fn resume(&mut self, value: Value) {
        self.control = Control::Return(value);
    }

    /// 让挂起的机器以运行时错误结束.
    pub(crate) fn resume_error(&mut self, error: Error) {
        self.control = Control::Fail(error);
    }

    /// 由 Handler 蓝图创建 Actor 机器.
    pub(crate) fn start_actor(
        handler: Value,
        pid: Arc<ActorHandle>,
        args: Vec<Value>,
        cli_args: Vec<String>,
        scheduler: SchedulerHandle,
    ) -> Result<Self, Error> {
        let Value::Closure(_) = &handler else {
            return Err(Error::internal("Actor 的 handler 不是 IR 闭包"));
        };
        let mut machine = Self::new();
        machine.args = cli_args;
        machine.set_scheduler(scheduler);
        machine.actor = Some(ActorRuntime {
            pid: pid.clone(),
            handler: handler.clone(),
            base_kont: 0,
        });
        let mut call_args = Vec::with_capacity(args.len() + 1);
        call_args.push(Value::Pid(pid));
        call_args.extend(args);
        machine.apply_value(handler, call_args, None)?;
        Ok(machine)
    }

    /// 注入命令行参数, `Args` 内置函数读取.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.args = args;
    }

    /// 替换主任务环境; 模块加载与 REPL 导出重绑定使用.
    pub(crate) fn set_globals(&mut self, env: Env) {
        self.globals = env;
    }

    #[must_use]
    pub(crate) fn globals(&self) -> &Env {
        &self.globals
    }

    /// 把顶层程序降级为线性 IR 并装入控制状态.
    pub(crate) fn start_program(
        &mut self,
        program: &crate::ast::Program,
        types: &crate::check::TypeTable,
    ) {
        self.control = Control::Halt;
        self.kont.clear();
        let module = super::ir::lower_program(program, types);
        let linear = super::linear::linearize(&module);
        let (functions, main) = linear.into_parts();
        let captures = linear::capture_values(&self.globals, &main.free_vars);
        let entry = main.entry;
        self.control =
            Control::Run(self.enter_block(main, entry, Vec::new(), captures, functions));
    }

    /// 按指令预算推进; 预算耗尽时保留控制状态供下次继续.
    ///
    /// 预算按 IR 指令与终止指令计数, 不按基本块计数; 超长基本块也会在
    /// 预算边界让出, Actor 与定时器因此不会被单个大块长期饿死.
    pub(crate) fn run_budget(&mut self, steps: usize) -> Result<RunOutcome, Error> {
        let gc = Arc::clone(&self.gc);
        let mut budget = steps;
        while budget > 0 {
            match self.step(&gc, &mut budget)? {
                StepResult::Finished(value) => {
                    return Ok(RunOutcome::Finished(value));
                },
                StepResult::Suspended(wait) => return Ok(RunOutcome::Suspended(wait)),
                StepResult::ActorExit(reason) => return Ok(RunOutcome::ActorExit(reason)),
                StepResult::Yield | StepResult::Continue => {},
            }
        }
        Ok(RunOutcome::Yield)
    }

    fn step(&mut self, gc: &GcRegistry, budget: &mut usize) -> Result<StepResult, Error> {
        let control = std::mem::replace(&mut self.control, Control::Halt);
        match control {
            Control::Run(state) => {
                if self.run_linear(state, gc, budget)? {
                    return Ok(StepResult::Yield);
                }
            },
            Control::Return(value) => {
                // 返回续延帧也是一次控制推进, 计入预算.
                *budget = budget.saturating_sub(1);
                let Some(frame) = self.kont.pop() else {
                    return Ok(StepResult::Finished(value));
                };
                self.return_to(frame, value)?;
            },
            Control::Suspend(wait) => return Ok(StepResult::Suspended(wait)),
            Control::Fail(error) => return Err(error),
            Control::ActorExit(reason) => return Ok(StepResult::ActorExit(reason)),
            Control::Halt => return Err(Error::internal("抽象机已停止")),
        }
        Ok(StepResult::Continue)
    }

    fn return_to(&mut self, frame: Kont, value: Value) -> Result<(), Error> {
        match frame {
            Kont::Linear(frame) => {
                let frame = *frame;
                let mut live = Vec::with_capacity(frame.live.len() + 1);
                live.push(value);
                live.extend(frame.live);
                self.control = Control::Run(self.enter_block(
                    frame.function,
                    frame.target,
                    live,
                    frame.captures,
                    frame.functions,
                ));
            },
            Kont::ListLoop(frame) => self.list_loop_return(*frame, value)?,
            Kont::DictFilter(frame) => self.dict_filter_return(*frame, &value)?,
            Kont::SetFilter(frame) => self.set_filter_return(*frame, &value)?,
            Kont::OptionMap { op } => {
                let result = match op {
                    OptionOpKind::OptionMap => option_some(value),
                    OptionOpKind::OptionAndThen | OptionOpKind::ResultAndThen => value,
                    OptionOpKind::ResultMap => result_ok(value),
                    _ => return Err(Error::internal("非法的 Option 映射操作")),
                };
                self.control = Control::Return(result);
            },
        }
        Ok(())
    }

    fn apply_value(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        span: Option<Span>,
    ) -> Result<(), Error> {
        match callee {
            Value::Builtin(id) => {
                let Some(kind) = builtin::resolve(id) else {
                    return Err(Error::internal(format!(
                        "内置函数未注册: {}",
                        builtin::name(id)
                    )));
                };
                match kind {
                    builtin::Builtin::Io(builder) => {
                        let task = builder(&args, span)?;
                        let Some(scheduler) = self.scheduler.clone() else {
                            return Err(Error::internal("I/O 内置需要调度器"));
                        };
                        let owner = self.actor.as_ref().map(|actor| actor.pid.id);
                        let op_id = scheduler.submit_io(task, owner);
                        self.control = Control::Suspend(Wait::Io { op_id });
                    },
                    builtin::Builtin::Pure(function) => {
                        let value = function(&self.args, &args, span)?;
                        self.control = Control::Return(value);
                    },
                }
            },
            Value::Closure(closure) => {
                if args.len() != closure.params.len() {
                    return Err(Error::internal("实参个数不符"));
                }
                let state = self.enter_block(
                    Arc::clone(&closure.function),
                    closure.function.entry,
                    args,
                    closure.captures(),
                    Arc::clone(&closure.functions),
                );
                self.control = Control::Run(state);
            },
            other => {
                return Err(Error::internal(format!(
                    "调用了不可调用的值: {}",
                    describe(&other)
                )));
            },
        }
        Ok(())
    }

    fn run_prim(
        &mut self,
        op: PrimOp,
        args: Vec<Value>,
        arg_types: &[crate::check::Type],
        ty: &crate::check::Type,
        span: Span,
    ) -> Result<(), Error> {
        match op {
            PrimOp::List(op) => self.start_list_op(op, args, ty, span),
            PrimOp::Pair(op) => self.start_pair_op(op, args, span),
            PrimOp::Dict(op) => self.start_dict_op(op, &args, ty, span),
            PrimOp::Set(op) => self.start_set_op(op, &args, ty, span),
            PrimOp::DictLiteral => self.start_dict_literal(args, span),
            PrimOp::SetLiteral => self.start_set_literal(args, span),
            PrimOp::Registry(op) => self.start_registry_op(op, &args, arg_types, ty, span),
            PrimOp::Option(op) => self.start_option_op(op, args, span),
            PrimOp::Spawn {
                detached,
                mailboxes,
            } => {
                let mut args = PrimArgs::new(args, span);
                let handler = args.next()?;
                self.spawn_actor(handler, mailboxes, args.remaining(), detached, span)
            },
            PrimOp::Send => self.start_send(args, span),
            PrimOp::Join => {
                let mut args = PrimArgs::new(args, span);
                let target = args.pid("Join")?;
                if self
                    .actor
                    .as_ref()
                    .is_some_and(|actor| actor.pid.id == target.id)
                {
                    return Err(Error::runtime(ErrorKind::JoinSelf, Some(span)));
                }
                // Join 是观察失败的契约; 之后的失败不再作为未处理错误重复诊断.
                target.mark_observed();
                if let Some(result) = target.result() {
                    self.control = Control::Return(join_result_value(&result));
                } else {
                    self.control = Control::Suspend(Wait::Join { target });
                }
                Ok(())
            },
            PrimOp::Kill => {
                let mut args = PrimArgs::new(args, span);
                let target = args.pid("Kill")?;
                let alive = !target.is_finished();
                if alive {
                    target.request_kill();
                    if let Some(scheduler) = &self.scheduler {
                        scheduler.wake_kill(target.id);
                    }
                }
                self.control = Control::Return(Value::Bool(alive));
                Ok(())
            },
            PrimOp::Monitor => {
                let mut args = PrimArgs::new(args, span);
                let target = args.pid("Monitor")?;
                self.start_monitor(&target, span)
            },
            PrimOp::Unmonitor => {
                let mut args = PrimArgs::new(args, span);
                let Value::U64(id) = args.next()? else {
                    return Err(Error::internal("Unmonitor 的编号不是 u64"));
                };
                self.start_unmonitor(id, span)
            },
            PrimOp::PidSerial => {
                let mut args = PrimArgs::new(args, span);
                let target = args.pid("PidSerial")?;
                self.pid_serial(&target);
                Ok(())
            },
            PrimOp::SetTimer => self.start_set_timer(args, span),
            PrimOp::CancelTimer => {
                let mut args = PrimArgs::new(args, span);
                let Value::U64(id) = args.next()? else {
                    return Err(Error::internal("CancelTimer 的编号不是 u64"));
                };
                self.start_cancel_timer(id, span)
            },
            PrimOp::Sleep => {
                let mut args = PrimArgs::new(args, span);
                let Value::U64(millis) = args.next()? else {
                    return Err(Error::internal("Sleep 的时长不是 u64"));
                };
                if millis == 0 {
                    self.control = Control::Return(Value::Nil);
                } else {
                    self.control = Control::Suspend(Wait::Sleep { millis, span });
                }
                Ok(())
            },
        }
    }

    fn receive(&mut self, index: usize, timeout: &Value, span: Span) -> Result<(), Error> {
        let Some(actor) = self.actor.as_ref() else {
            return Err(Error::internal("@Receive 出现在 Handler 外"));
        };
        let timeout = Self::parse_timeout(timeout)?;
        if let Some(message) = actor.pid.try_recv(index) {
            self.control = Control::Return(option_some(message));
        } else if timeout == Some(0) {
            self.control = Control::Return(option_none());
        } else {
            self.control = Control::Suspend(Wait::Receive {
                index,
                timeout,
                span,
            });
        }
        Ok(())
    }
}

impl Machine {
    /// 遍历抽象机持有的运行时值; 环回收根标记用.
    pub(crate) fn trace(&self, marker: &mut Marker) {
        marker.mark_env(&self.globals);
        self.control.trace(marker);
        for frame in &self.kont {
            frame.trace(marker);
        }
        if let Some(actor) = &self.actor {
            marker.mark_actor(&actor.pid);
            marker.mark_value(&actor.handler);
        }
    }
}

impl Control {
    fn trace(&self, marker: &mut Marker) {
        match self {
            Self::Run(state) => {
                for value in state.locals.iter().flatten() {
                    marker.mark_value(value);
                }
                marker.mark_captures(&state.captures);
            },
            Self::Return(value) => marker.mark_value(value),
            Self::Suspend(wait) => wait.trace(marker),
            Self::Fail(_) | Self::ActorExit(_) | Self::Halt => {},
        }
    }
}

impl Wait {
    pub(crate) fn trace(&self, marker: &mut Marker) {
        if let Self::Join { target } = self {
            marker.mark_actor(target);
        }
    }
}

impl Kont {
    fn trace(&self, marker: &mut Marker) {
        match self {
            Self::Linear(frame) => {
                for value in &frame.live {
                    marker.mark_value(value);
                }
                marker.mark_captures(&frame.captures);
            },
            Self::DictFilter(frame) => {
                marker.mark_value(&frame.predicate);
                for (_, value) in &frame.entries {
                    marker.mark_value(value);
                }
                for value in frame.kept.values() {
                    marker.mark_value(value);
                }
            },
            Self::SetFilter(frame) => marker.mark_value(&frame.predicate),
            Self::ListLoop(frame) => {
                marker.mark_value(&frame.function);
                if let crate::value::List::Values(items) = &frame.list {
                    for item in items {
                        marker.mark_value(item);
                    }
                }
                if let Some(second) = &frame.second
                    && let crate::value::List::Values(items) = second
                {
                    for item in items {
                        marker.mark_value(item);
                    }
                }
                if let Some(current) = &frame.current {
                    marker.mark_value(current);
                }
                if let Some(current) = &frame.current2 {
                    marker.mark_value(current);
                }
                if let crate::value::List::Values(items) = &frame.kept {
                    for item in items {
                        marker.mark_value(item);
                    }
                }
                marker.mark_value(&frame.acc);
            },
            Self::OptionMap { .. } => {},
        }
    }
}

#[cfg(test)]
mod tests;
