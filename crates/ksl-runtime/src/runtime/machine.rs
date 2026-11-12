//! 执行线性 CPS IR 的抽象机.
//!
//! 控制状态是基本块, 指令下标, 槽位与捕获; 函数返回走续延帧,
//! Actor 与 I/O 挂起即保存该状态. 列表与 Option 的高阶操作沿用续延帧,
//! 不依赖 Rust 原生栈.

use std::sync::Arc;

use crossbeam_channel::Sender;

use super::event::Event;
use super::gc::{GcRegistry, Marker};
use crate::cps::ir::{OptionPrim, PrimOp};
use crate::cps::linear::{BlockId, Function as LinearFunction, Functions as LinearFunctions};
use crate::ffi::ReleaseQueue;
use crate::io::IoRuntime;
use crate::root::RootSet;

mod actor;
mod collection;
mod effect;
mod eval;
mod linear;
mod list;
mod monitor;
mod option;
mod registry;
mod timer;

pub(crate) use effect::{Effect, EffectResult};
use eval::describe;
use ksl_base::source::Span;
use ksl_frontend::ast::ListOpKind;
use ksl_frontend::error::{ErrorKind, RuntimeError as Error};

use crate::builtin;
use crate::value::{
    ActorHandle, Capture, Env, ExitReason, Key, Value, option_none, option_some, result_ok,
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
    /// 槽位值; 线性化保证读取前已绑定.
    locals: Vec<Value>,
    /// 槽位绑定标记; 仅调试构建维护, 用于抓线性化缺陷.
    #[cfg(debug_assertions)]
    defined: Vec<bool>,
    captures: Arc<[Capture]>,
    functions: Arc<LinearFunctions>,
}

/// 一次调度片段的结局.
pub(crate) enum RunOutcome {
    Finished(Value),
    Suspended(Wait),
    /// 交回一个需要协调器执行的效果; 执行结果经 `resume` 交回.
    Effect(Effect),
    ActorExit(ExitReason),
    /// 预算耗尽, 保留执行状态后重新排队.
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
        span: Span,
    },
    Sleep {
        millis: u64,
        span: Span,
    },
    Io {
        op_id: u64,
        /// 线程池实参与网络操作的目标句柄; 在操作完成前作为环回收根.
        ///
        /// 这些值只被提交到线程池的闭包或网络引擎持有, 不登记就会在标记中不可见.
        roots: RootSet,
    },
}

enum StepResult {
    Continue,
    Yield,
    Finished(Value),
    Suspended(Wait),
    Effect(Effect),
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
        op: OptionPrim,
    },
}

enum Control {
    Run(LinearState),
    Return(Value),
    Suspend(Wait),
    /// 重新检查信箱; 信箱由任务自身消费, 唤醒只通知而不交付消息.
    RetryReceive {
        index: usize,
        timeout: Option<u64>,
        span: Span,
    },
    Effect(Effect),
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
    gc: Arc<GcRegistry>,
    /// 槽位缓冲池: 基本块切换时回收复用, 避免每次进入块都重新分配.
    locals_pool: Vec<Vec<Value>>,
    /// 参数求值缓冲池: 块跳转与续延参数就地复用.
    value_buffers: Vec<Vec<Value>>,
    /// 内置纯函数实参缓冲; 调用前后复用, 避免每次求值都分配.
    builtin_args: Vec<Value>,
    /// 输出服务; 缺失时 Print 回退为效果.
    io: Option<Arc<IoRuntime>>,
    /// 协调器收件箱; 内联 Send 唤醒与效果提交使用.
    events: Option<Sender<Event>>,
    /// 调度线程释放队列; 内联当前调用使用.
    releases: Arc<ReleaseQueue>,
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
            gc: Arc::new(GcRegistry::new()),
            locals_pool: Vec::new(),
            value_buffers: Vec::new(),
            builtin_args: Vec::new(),
            io: None,
            events: None,
            releases: Arc::new(ReleaseQueue::detached()),
        }
    }

    pub(crate) fn set_gc(&mut self, gc: Arc<GcRegistry>) {
        self.gc = gc;
    }

    /// 附着机器可直接使用的运行时服务.
    pub(crate) fn set_runtime(
        &mut self,
        io: Arc<IoRuntime>,
        events: Sender<Event>,
        releases: Arc<ReleaseQueue>,
    ) {
        self.io = Some(io);
        self.events = Some(events);
        self.releases = releases;
    }

    fn notify(&self, event: Event) {
        if let Some(events) = &self.events {
            let _ = events.send(event);
        }
    }

    /// 恢复挂起的机器: 把挂起原语的结果交回续延.
    pub(crate) fn resume(&mut self, value: Value) {
        self.control = Control::Return(value);
    }

    /// 让挂起的机器以运行时错误结束.
    pub(crate) fn resume_error(&mut self, error: Error) {
        self.control = Control::Fail(error);
    }

    /// 保存挂起原因; 协调器登记定时器或 Join 等待后调用.
    pub(crate) fn park(&mut self, wait: Wait) {
        self.control = Control::Suspend(wait);
    }

    /// 当前挂起原因; 未挂起时为 None.
    #[must_use]
    pub(crate) fn wait(&self) -> Option<&Wait> {
        match &self.control {
            Control::Suspend(wait) => Some(wait),
            _ => None,
        }
    }

    /// 把接收挂起改为重新检查信箱; 返回是否转换成功.
    pub(crate) fn retry_receive(&mut self) -> bool {
        let control = std::mem::replace(&mut self.control, Control::Halt);
        match control {
            Control::Suspend(Wait::Receive {
                index,
                timeout,
                span,
            }) => {
                self.control = Control::RetryReceive {
                    index,
                    timeout,
                    span,
                };
                true
            },
            other => {
                self.control = other;
                false
            },
        }
    }

    /// 由 Handler 蓝图创建 Actor 机器.
    pub(crate) fn start_actor(
        handler: Value,
        pid: Arc<ActorHandle>,
        args: Vec<Value>,
        cli_args: Vec<String>,
        gc: Arc<GcRegistry>,
    ) -> Result<Self, Error> {
        let Value::Closure(_) = &handler else {
            return Err(Error::internal("Actor 的 handler 不是 IR 闭包"));
        };
        let mut machine = Self::new();
        machine.args = cli_args;
        machine.set_gc(gc);
        machine.actor = Some(ActorRuntime {
            pid: pid.clone(),
            handler: handler.clone(),
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
        program: &ksl_frontend::ast::Program,
        types: &ksl_frontend::check::TypeTable,
    ) {
        self.control = Control::Halt;
        self.kont.clear();
        let module = crate::cps::ir::lower_program(program, types);
        let linear = crate::cps::linear::linearize(&module);
        let (functions, main) = linear.into_parts();
        let captures = linear::capture_values(&self.globals, &main.free_vars);
        let entry = main.entry;
        let mut args = Vec::new();
        self.control =
            Control::Run(self.enter_block(main, entry, &mut args, captures, functions));
    }

    /// 按指令预算推进; 预算耗尽时保留控制状态供下次继续.
    ///
    /// 预算按 IR 指令与终止指令计数, 不按基本块计数.
    /// 超长基本块也会在预算边界让出, Actor 与定时器因此不会被单个大块长期饿死.
    pub(crate) fn run_budget(&mut self, steps: usize) -> Result<RunOutcome, Error> {
        let gc = Arc::clone(&self.gc);
        let mut budget = steps;
        while budget > 0 {
            let result = self.step(&gc, &mut budget)?;
            match result {
                StepResult::Finished(value) => {
                    return Ok(RunOutcome::Finished(value));
                },
                StepResult::Suspended(wait) => return Ok(RunOutcome::Suspended(wait)),
                // 效果统一交回协调器, 当前片段到此结束.
                StepResult::Effect(effect) => return Ok(RunOutcome::Effect(effect)),
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
            Control::RetryReceive {
                index,
                timeout,
                span,
            } => self.receive(index, timeout, span)?,
            Control::Effect(effect) => return Ok(StepResult::Effect(effect)),
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
                let mut live = frame.live;
                live.insert(0, value);
                let state = self.enter_block(
                    frame.function,
                    frame.target,
                    &mut live,
                    frame.captures,
                    frame.functions,
                );
                self.recycle_values(live);
                self.control = Control::Run(state);
            },
            Kont::ListLoop(frame) => self.list_loop_return(frame, value)?,
            Kont::DictFilter(frame) => self.dict_filter_return(*frame, &value)?,
            Kont::SetFilter(frame) => self.set_filter_return(*frame, &value)?,
            Kont::OptionMap { op } => {
                let result = match op {
                    OptionPrim::OptionMap => option_some(value),
                    OptionPrim::OptionAndThen | OptionPrim::ResultAndThen => value,
                    OptionPrim::ResultMap => result_ok(value),
                    OptionPrim::IsSome
                    | OptionPrim::IsNone
                    | OptionPrim::IsOk
                    | OptionPrim::IsErr
                    | OptionPrim::ResultOrElse => {
                        return Err(Error::internal("非法的 Option 映射操作"));
                    },
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
                        self.control = Control::Effect(Effect::SubmitIo { task });
                    },
                    builtin::Builtin::Pure(function) => {
                        let value = function(&self.args, &args, span)?;
                        self.control = Control::Return(value);
                    },
                    builtin::Builtin::Print { error } => {
                        let Some(Value::Str(text)) = args.first() else {
                            return Err(Error::internal("Print 的实参形态不符"));
                        };
                        let Some(io) = &self.io else {
                            return Err(Error::internal("Print 缺少输出服务"));
                        };
                        let result = if error {
                            io.print_error(text)
                        } else {
                            io.print(text)
                        };
                        self.control = match result {
                            Ok(()) => Control::Return(Value::Nil),
                            Err(error) => Control::Fail(error),
                        };
                    },
                }
            },
            Value::ForeignFn(function) => {
                if function.execution() == crate::ffi::Execution::Current {
                    let releases = Arc::downgrade(&self.releases);
                    let result = function.call(&args, span.unwrap_or_default(), &releases);
                    self.control = match result {
                        Ok(value) => Control::Return(value),
                        Err(error) => Control::Fail(error),
                    };
                } else {
                    self.control = Control::Effect(Effect::ForeignCall {
                        function,
                        args,
                        span: span.unwrap_or_default(),
                    });
                }
            },
            Value::Closure(closure) => {
                if args.len() != closure.arity {
                    return Err(Error::internal("实参个数不符"));
                }
                let mut args = args;
                let state = self.enter_block(
                    Arc::clone(&closure.function),
                    closure.function.entry,
                    &mut args,
                    closure.captures(),
                    Arc::clone(&closure.functions),
                );
                self.recycle_values(args);
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
        arg_types: &[ksl_frontend::check::Type],
        ty: &ksl_frontend::check::Type,
        span: Span,
    ) -> Result<(), Error> {
        match op {
            PrimOp::List(op) => self.start_list_op(op, args, ty, span),
            PrimOp::Pair(op) => self.start_pair_op(op, args, span),
            PrimOp::Dict(op) => self.start_dict_op(op, args, ty, span),
            PrimOp::Set(op) => self.start_set_op(op, args, ty, span),
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
                self.control = Control::Effect(Effect::Spawn {
                    handler,
                    mailboxes,
                    detached,
                    args: args.remaining(),
                });
                Ok(())
            },
            PrimOp::Send => self.start_send(args, span),
            PrimOp::Join => {
                let mut args = PrimArgs::new(args, span);
                let target = args.pid("Join")?;
                self.control = Control::Suspend(Wait::Join { target, span });
                Ok(())
            },
            PrimOp::Kill => {
                let mut args = PrimArgs::new(args, span);
                let target = args.pid("Kill")?;
                self.control = Control::Effect(Effect::Kill { target });
                Ok(())
            },
            PrimOp::Monitor => self.start_monitor(args, span),
            PrimOp::Unmonitor => self.start_unmonitor(args, span),
            PrimOp::PidSerial => {
                let mut args = PrimArgs::new(args, span);
                let target = args.pid("PidSerial")?;
                self.pid_serial(&target);
                Ok(())
            },
            PrimOp::SetTimer => self.start_set_timer(args, span),
            PrimOp::CancelTimer => self.start_cancel_timer(args, span),
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

    fn receive_value(
        &mut self,
        index: usize,
        timeout: &Value,
        span: Span,
    ) -> Result<(), Error> {
        let timeout = Self::parse_timeout(timeout)?;
        self.receive(index, timeout, span)
    }

    fn receive(&mut self, index: usize, timeout: Option<u64>, span: Span) -> Result<(), Error> {
        let Some(actor) = self.actor.as_ref() else {
            return Err(Error::internal("@Receive 出现在 Handler 外"));
        };
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
                for value in &state.locals {
                    marker.mark_value(value);
                }
                marker.mark_captures(&state.captures);
            },
            Self::Return(value) => marker.mark_value(value),
            Self::Suspend(wait) => wait.trace(marker),
            Self::Effect(effect) => effect.trace(marker),
            Self::RetryReceive { .. } | Self::Fail(_) | Self::ActorExit(_) | Self::Halt => {},
        }
    }
}

impl Wait {
    pub(crate) fn trace(&self, marker: &mut Marker) {
        match self {
            Self::Join { target, .. } => marker.mark_actor(target),
            Self::Io { roots, .. } => marker.mark_roots(roots),
            Self::Receive { .. } | Self::Sleep { .. } => {},
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
