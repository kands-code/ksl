//! 线性 IR 解释: 槽位, 捕获与基本块.
//!
//! 线性代码已完成闭包转换: 捕获下标取捕获值, 递归定义共享写一次单元,
//! 变量读取按槽位或捕获下标取值, 不查名字环境.
//! 续延是目标基本块, 跳转时显式传入参数值.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_plan::{
    BlockId, FStrPart, Function as LinearFunction, Functions as LinearFunctions, GlobalTarget,
    Instr, Operand, PatternIr, RecBinding, Terminator, VarRef,
};
use ksl_value::value::{
    Capture, DataFields, Env, ExitReason, FixCell, List, Value, data_value,
};

use super::eval::closure_linear;
use super::{Control, Kont, LinearFrame, LinearState, Machine};
use crate::builtin;
use crate::runtime::gc::GcRegistry;

/// 槽位缓冲池的上限; 超出上限的缓冲直接释放.
const LOCALS_POOL_CAP: usize = 16;
/// 参数求值缓冲池的上限与可复用容量.
const VALUE_BUFFER_POOL_CAP: usize = 16;
const VALUE_BUFFER_CAPACITY: usize = 64;

impl Machine {
    /// 进入基本块: 参数占据前若干连续槽位; 槽位缓冲优先从池中复用.
    pub(super) fn enter_block(
        &mut self,
        function: &Arc<LinearFunction>,
        block: BlockId,
        args: &mut Vec<Value>,
        captures: Arc<[Capture]>,
        functions: Arc<LinearFunctions>,
    ) -> LinearState {
        let target = &function.blocks[block.index() as usize];
        debug_assert_eq!(args.len(), target.param_count, "基本块参数个数不符");
        let mut locals = self.take_locals(target.slot_count);
        for (slot, value) in args.drain(..).enumerate() {
            locals[slot] = value;
        }
        #[cfg(debug_assertions)]
        let defined = {
            let mut defined = vec![false; target.slot_count];
            defined[..target.param_count].fill(true);
            defined
        };
        LinearState {
            block,
            instr: 0,
            locals,
            #[cfg(debug_assertions)]
            defined,
            captures,
            functions,
        }
    }

    /// 进入续延目标块: 结果占据第一个参数槽, 其余活跃值随后.
    pub(super) fn enter_block_with_result(
        &mut self,
        function: &Arc<LinearFunction>,
        block: BlockId,
        result: Value,
        live: &mut Vec<Value>,
        captures: Arc<[Capture]>,
        functions: Arc<LinearFunctions>,
    ) -> LinearState {
        let target = &function.blocks[block.index() as usize];
        debug_assert_eq!(live.len() + 1, target.param_count, "续延参数个数不符");
        let mut locals = self.take_locals(target.slot_count);
        locals[0] = result;
        for (slot, value) in live.drain(..).enumerate() {
            locals[slot + 1] = value;
        }
        #[cfg(debug_assertions)]
        let defined = {
            let mut defined = vec![false; target.slot_count];
            defined[..target.param_count].fill(true);
            defined
        };
        LinearState {
            block,
            instr: 0,
            locals,
            #[cfg(debug_assertions)]
            defined,
            captures,
            functions,
        }
    }

    fn take_locals(&mut self, slots: usize) -> Vec<Value> {
        if let Some(mut locals) = self.locals_pool.pop() {
            locals.resize(slots, Value::Nil);
            locals
        } else {
            vec![Value::Nil; slots]
        }
    }

    /// 回收状态持有的槽位缓冲; 入池前已清空, 池中缓冲不含存活值.
    fn recycle_locals(&mut self, state: &mut LinearState) {
        let mut locals = std::mem::take(&mut state.locals);
        locals.clear();
        if self.locals_pool.len() < LOCALS_POOL_CAP {
            self.locals_pool.push(locals);
        }
    }

    /// 取一个空的参数求值缓冲; 池为空时新建.
    #[inline]
    pub(super) fn take_values(&mut self) -> Vec<Value> {
        self.value_buffers.pop().unwrap_or_default()
    }

    /// 求值一组操作数并返回缓冲; 调用方用完后回收.
    fn take_live(
        &mut self,
        operands: &[Operand],
        state: &LinearState,
        gc: &GcRegistry,
    ) -> Result<Vec<Value>, Error> {
        let mut values = self.take_values();
        eval_into(operands, state, gc, &mut values)?;
        Ok(values)
    }

    /// 回收参数求值缓冲; 超出池上限或容量过大的缓冲直接释放.
    #[inline]
    pub(super) fn recycle_values(&mut self, mut values: Vec<Value>) {
        values.clear();
        if self.value_buffers.len() < VALUE_BUFFER_POOL_CAP
            && values.capacity() <= VALUE_BUFFER_CAPACITY
        {
            self.value_buffers.push(values);
        }
    }

    pub(super) fn run_linear(
        &mut self,
        mut function: Arc<LinearFunction>,
        mut state: LinearState,
        gc: &GcRegistry,
        budget: &mut usize,
    ) -> Result<bool, Error> {
        // 当前函数随控制状态持有, 推进期间只借用.
        // 预算耗尽时停在当前指令之前, 把控制状态原样留给下一次调度.
        // 不要求以基本块为让出单位.
        loop {
            if *budget == 0 {
                self.control = Control::Run(function, state);
                return Ok(true);
            }
            let block = &function.blocks[state.block.index() as usize];
            self.run_instrs(&mut state, &block.instrs, budget, gc)?;
            if *budget == 0 {
                continue;
            }
            *budget -= 1;
            // 同函数块切换原地复用槽位缓冲, 不经控制状态往返.
            match &block.term {
                Terminator::Jump { target, args } => {
                    let mut values = self.take_values();
                    eval_into(args, &state, gc, &mut values)?;
                    Self::switch_block(&function, &mut state, *target, &mut values);
                    self.recycle_values(values);
                },
                Terminator::If { cond, then, els } => {
                    let cond = eval_operand(cond, &state, gc)?;
                    let Value::Bool(cond) = cond else {
                        return Err(Error::internal("条件不是 bool"));
                    };
                    let target = if cond { then } else { els };
                    let mut values = self.take_values();
                    eval_into(&target.args, &state, gc, &mut values)?;
                    Self::switch_block(&function, &mut state, target.block, &mut values);
                    self.recycle_values(values);
                },
                Terminator::Match { value, arms } => {
                    let value = eval_operand(value, &state, gc)?;
                    let mut values = self.take_values();
                    let mut found = None;
                    for arm in arms {
                        if match_linear(&arm.pattern, &value, &mut values) {
                            found = Some(arm);
                            break;
                        }
                    }
                    let Some(arm) = found else {
                        return Err(Error::internal("模式匹配没有命中分支"));
                    };
                    eval_into(&arm.live, &state, gc, &mut values)?;
                    Self::switch_block(&function, &mut state, arm.target, &mut values);
                    self.recycle_values(values);
                },
                // 尾调用与返回在抽象机内就地完成, 不经控制状态.
                Terminator::TailCall { callee, args, span } => {
                    if let Some((next_function, next_state)) =
                        self.start_tail_call(callee, args, &mut state, gc, *span)?
                    {
                        function = next_function;
                        state = next_state;
                        continue;
                    }
                    return Ok(false);
                },
                Terminator::Return { value } => {
                    if let Some((next_function, next_state)) =
                        self.start_return(value, &mut state, budget, gc)?
                    {
                        function = next_function;
                        state = next_state;
                        continue;
                    }
                    return Ok(false);
                },
                // 被调与续延走常规路径; 同步内置与同步原语已在降级期成为块内指令.
                _ => {
                    self.step_term(&block.term, &function, state, gc)?;
                    return Ok(false);
                },
            }
        }
    }

    /// 在预算内连续执行当前基本块的指令, 不逐条检查预算.
    fn run_instrs(
        &mut self,
        state: &mut LinearState,
        instrs: &[Instr],
        budget: &mut usize,
        gc: &GcRegistry,
    ) -> Result<(), Error> {
        let start = state.instr;
        if start == instrs.len() {
            return Ok(());
        }
        let end = start.saturating_add(*budget).min(instrs.len());
        for instr in &instrs[start..end] {
            self.step_instr(instr, state, gc)?;
        }
        state.instr = end;
        *budget -= end - start;
        Ok(())
    }

    /// 尾调用: 闭包就地进入目标函数; 内置与外来函数交回控制层.
    fn start_tail_call(
        &mut self,
        callee: &Operand,
        args: &[Operand],
        state: &mut LinearState,
        gc: &GcRegistry,
        span: Span,
    ) -> Result<Option<(Arc<LinearFunction>, LinearState)>, Error> {
        let callee = eval_operand(callee, state, gc)?;
        let mut values = self.take_values();
        eval_into(args, state, gc, &mut values)?;
        let Value::Closure(closure) = callee else {
            self.recycle_locals(state);
            self.apply_value(callee, values, Some(span))?;
            return Ok(None);
        };
        if values.len() != closure.arity {
            return Err(Error::internal("实参个数不符"));
        }
        self.recycle_locals(state);
        let next = self.enter_block(
            &closure.function,
            closure.function.entry,
            &mut values,
            closure.captures(),
            Arc::clone(&closure.functions),
        );
        self.recycle_values(values);
        Ok(Some((Arc::clone(&closure.function), next)))
    }

    /// 返回: 线性续延就地进入目标块; 其余续延交回控制层.
    fn start_return(
        &mut self,
        value: &Operand,
        state: &mut LinearState,
        budget: &mut usize,
        gc: &GcRegistry,
    ) -> Result<Option<(Arc<LinearFunction>, LinearState)>, Error> {
        let value = eval_operand(value, state, gc)?;
        self.recycle_locals(state);
        let Some(frame) = self.kont.pop() else {
            self.control = Control::Return(value);
            return Ok(None);
        };
        let Kont::Linear = frame else {
            self.kont.push(frame);
            self.control = Control::Return(value);
            return Ok(None);
        };
        // 与经控制态的返回一致, 返回续延帧计一步.
        *budget = budget.saturating_sub(1);
        let frame = self.linear_frames.pop().expect("续延帧栈不一致");
        let LinearFrame {
            function,
            target,
            mut live,
            captures,
            functions,
        } = frame;
        let next = self.enter_block_with_result(
            &function,
            target,
            value,
            &mut live,
            captures,
            functions,
        );
        self.recycle_values(live);
        Ok(Some((function, next)))
    }

    /// 在内置纯函数实参缓冲中求值并直接调用.
    ///
    /// 内置纯函数不回调抽象机, 缓冲可在调用间复用.
    fn eval_builtin(
        &mut self,
        operands: &[Operand],
        state: &LinearState,
        gc: &GcRegistry,
        span: Span,
        pure: builtin::BuiltinFn,
    ) -> Result<Value, Error> {
        self.builtin_args.clear();
        for operand in operands {
            match eval_operand(operand, state, gc) {
                Ok(value) => self.builtin_args.push(value),
                Err(error) => {
                    self.builtin_args.clear();
                    return Err(error);
                },
            }
        }
        let value = pure(&self.builtin_args, Some(span));
        self.builtin_args.clear();
        value
    }

    /// 同函数基本块切换: 参数求值完毕后原地重置槽位与指令下标.
    #[inline]
    fn switch_block(
        function: &Arc<LinearFunction>,
        state: &mut LinearState,
        target: BlockId,
        args: &mut Vec<Value>,
    ) {
        let block = &function.blocks[target.index() as usize];
        debug_assert_eq!(args.len(), block.param_count, "基本块参数个数不符");
        // 非参数槽位在块内先写后读, 旧值最迟到该次写入时释放, 无需全量清空.
        let slots = block.slot_count;
        if state.locals.len() < slots {
            state.locals.resize(slots, Value::Nil);
        } else {
            state.locals.truncate(slots);
        }
        #[cfg(debug_assertions)]
        {
            state.defined.clear();
            state.defined.resize(slots, false);
        }
        for (slot, value) in args.drain(..).enumerate() {
            state.locals[slot] = value;
            #[cfg(debug_assertions)]
            {
                state.defined[slot] = true;
            }
        }
        state.block = target;
        state.instr = 0;
    }
}

/// 由全局环境取主函数捕获值; 缺失的绑定留空, 首次读取时报错.
pub(super) fn capture_values(globals: &Env, names: &[Name]) -> Arc<[Capture]> {
    names
        .iter()
        .map(|name| {
            globals
                .get(name)
                .cloned()
                .map_or(Capture::Missing(name.clone()), Capture::Value)
        })
        .collect()
}

impl Machine {
    fn step_instr(
        &mut self,
        instr: &Instr,
        state: &mut LinearState,
        gc: &GcRegistry,
    ) -> Result<(), Error> {
        match instr {
            Instr::Let { slot, value } => {
                let value = eval_operand(value, state, gc)?;
                set_local(state, *slot, value);
            },
            Instr::Builtin {
                slot,
                callee,
                args,
                span,
            } => {
                let value = match builtin::resolve(*callee) {
                    builtin::Builtin::Pure(pure) => {
                        self.eval_builtin(args, state, gc, *span, pure)?
                    },
                    builtin::Builtin::Args => self.args.clone(),
                    _ => return Err(Error::internal("同步求值不是内置纯函数")),
                };
                set_local(state, *slot, value);
            },
            Instr::Prim {
                slot,
                op,
                args,
                ty,
                span,
                dead,
            } => {
                // 实参缓冲从池中取用, 原语按游标取走; 无论成败都回收.
                let mut values = self.take_values();
                let result = eval_into(args, state, gc, &mut values).and_then(|()| {
                    for slot in dead {
                        clear_local(state, *slot);
                    }
                    self.run_sync_prim(op.clone(), &mut values, ty, *span)
                });
                self.recycle_values(values);
                let value = result?;
                set_local(state, *slot, value);
            },
            Instr::GlobalLet {
                target,
                ty: _,
                value,
            } => {
                let value = eval_operand(value, state, gc)?;
                if let GlobalTarget::Named { slot, name } = target {
                    set_local(state, *slot, value.clone());
                    self.globals.insert(name.clone(), value);
                }
            },
            Instr::LetRec { bindings } => self.bind_rec_linear(bindings, state, false, gc)?,
            Instr::GlobalLetRec { bindings } => {
                self.bind_rec_linear(bindings, state, true, gc)?;
            },
        }
        Ok(())
    }

    fn bind_rec_linear(
        &mut self,
        bindings: &[RecBinding],
        state: &mut LinearState,
        global: bool,
        gc: &GcRegistry,
    ) -> Result<(), Error> {
        // 递归组先建写一次单元, 让组内函数可以捕获尚未完成的同伴.
        // 回填后再把槽位与全局环境换成普通值.
        let mut fixes = HashMap::with_capacity(bindings.len());
        let mut cells = Vec::with_capacity(bindings.len());
        for binding in bindings {
            let cell: FixCell = Arc::new(OnceLock::new());
            fixes.insert(binding.slot, Arc::clone(&cell));
            cells.push(cell);
        }
        let mut closures = Vec::with_capacity(bindings.len());
        for (binding, cell) in bindings.iter().zip(&cells) {
            let function =
                state.functions.get(&binding.function).cloned().ok_or_else(|| {
                    Error::internal(format!("线性函数未注册: {}", binding.function))
                })?;
            let captures = eval_captures(&binding.captures, state, gc, Some(&fixes))?;
            let closure = closure_linear(&state.functions, &function, captures.into(), gc);
            if cell.set(closure.clone()).is_err() {
                return Err(Error::internal("递归单元被重复回填"));
            }
            closures.push(closure);
        }
        for (binding, closure) in bindings.iter().zip(closures) {
            set_local(state, binding.slot, closure.clone());
            if global {
                self.globals.insert(binding.name.clone(), closure);
            }
        }
        Ok(())
    }

    // 终止指令分派; 指令批, 同函数块切换, 尾调用与返回已在 `run_linear` 内联处理.
    // 状态在终止指令后不再使用, 按值传入并回收.
    fn step_term(
        &mut self,
        term: &Terminator,
        function: &Arc<LinearFunction>,
        mut state: LinearState,
        gc: &GcRegistry,
    ) -> Result<(), Error> {
        match term {
            Terminator::Call {
                callee,
                args,
                target,
                live,
                span,
            } => {
                let callee = eval_operand(callee, &state, gc)?;
                let mut call_args = self.take_values();
                eval_into(args, &state, gc, &mut call_args)?;
                let live_values = self.take_live(live, &state, gc)?;
                self.push_linear_frame(function, state, *target, live_values);
                self.apply_value(callee, call_args, Some(*span))?;
            },
            Terminator::Prim {
                op,
                args,
                ty,
                target,
                live,
                span,
                ..
            } => {
                let args = eval_values(args, &state, gc)?;
                let live_values = self.take_live(live, &state, gc)?;
                self.push_linear_frame(function, state, *target, live_values);
                self.run_prim(op.clone(), args, ty, *span)?;
            },
            Terminator::Receive {
                index,
                timeout,
                target,
                live,
                span,
            } => {
                let timeout = eval_operand(timeout, &state, gc)?;
                let live_values = self.take_live(live, &state, gc)?;
                self.push_linear_frame(function, state, *target, live_values);
                self.receive_value(*index, &timeout, *span)?;
            },
            Terminator::Continue { args } => {
                let Some((handler, pid)) = self
                    .actor
                    .as_ref()
                    .map(|actor| (actor.handler.clone(), Arc::clone(&actor.pid)))
                else {
                    return Err(Error::internal("@Continue 出现在 Handler 外"));
                };
                let mut call_args = self.take_values();
                call_args.push(Value::Pid(pid));
                eval_into(args, &state, gc, &mut call_args)?;
                self.recycle_locals(&mut state);
                self.kont.clear();
                self.linear_frames.clear();
                self.apply_value(handler, call_args, None)?;
            },
            Terminator::Exit { value } => {
                let value = eval_operand(value, &state, gc)?;
                self.recycle_locals(&mut state);
                let Value::U8(code) = value else {
                    return Err(Error::internal("退出码不是 u8"));
                };
                return Err(Error::Exit(code));
            },
            Terminator::Halt => {
                self.recycle_locals(&mut state);
                self.control = Control::ActorExit(ExitReason::Halted);
            },
            Terminator::Fail {
                kind,
                message,
                span,
            } => {
                let message = eval_operand(message, &state, gc)?;
                self.recycle_locals(&mut state);
                let Value::Str(text) = message else {
                    return Err(Error::internal("失败信息不是 string"));
                };
                let kind =
                    ksl_diagnostics::error::ErrorKind::from_fail(*kind, text.to_string());
                return Err(Error::runtime(kind, Some(*span)));
            },
            Terminator::Jump { .. }
            | Terminator::If { .. }
            | Terminator::Match { .. }
            | Terminator::TailCall { .. }
            | Terminator::Return { .. } => {
                unreachable!("块内切换, 尾调用与返回已在 run_linear 内联处理");
            },
        }
        Ok(())
    }

    /// 压入续延帧; 状态随终止指令结束, 捕获按值移入帧.
    fn push_linear_frame(
        &mut self,
        function: &Arc<LinearFunction>,
        mut state: LinearState,
        target: BlockId,
        live: Vec<Value>,
    ) {
        self.recycle_locals(&mut state);
        let LinearState {
            captures,
            functions,
            ..
        } = state;
        self.linear_frames.push(LinearFrame {
            target,
            live,
            function: Arc::clone(function),
            captures,
            functions,
        });
        self.kont.push(Kont::Linear);
    }
}

fn eval_values(
    operands: &[Operand],
    state: &LinearState,
    gc: &GcRegistry,
) -> Result<Vec<Value>, Error> {
    let mut values = Vec::with_capacity(operands.len());
    eval_into(operands, state, gc, &mut values)?;
    Ok(values)
}

/// 把一组操作数求值到已有缓冲; 缓冲由调用方负责回收.
#[inline]
fn eval_into(
    operands: &[Operand],
    state: &LinearState,
    gc: &GcRegistry,
    values: &mut Vec<Value>,
) -> Result<(), Error> {
    for operand in operands {
        values.push(eval_operand(operand, state, gc)?);
    }
    Ok(())
}

/// 读取槽位值; 线性化保证读取前已绑定, 调试构建校验该不变量.
#[inline]
fn local_value(state: &LinearState, slot: u32) -> Value {
    #[cfg(debug_assertions)]
    debug_assert!(
        state.defined.get(slot as usize).copied().unwrap_or(false),
        "槽位 {slot} 尚未绑定"
    );
    state.locals[slot as usize].clone()
}

/// 写入槽位值并更新调试构建的绑定标记.
#[inline]
fn set_local(state: &mut LinearState, slot: u32, value: Value) {
    state.locals[slot as usize] = value;
    #[cfg(debug_assertions)]
    {
        state.defined[slot as usize] = true;
    }
}

/// 清空已结束生命周期的槽位; 调试构建同时撤销绑定标记.
#[inline]
fn clear_local(state: &mut LinearState, slot: u32) {
    state.locals[slot as usize] = Value::Nil;
    #[cfg(debug_assertions)]
    {
        state.defined[slot as usize] = false;
    }
}

fn eval_captures(
    operands: &[Operand],
    state: &LinearState,
    gc: &GcRegistry,
    fixes: Option<&HashMap<u32, FixCell>>,
) -> Result<Vec<Capture>, Error> {
    operands
        .iter()
        .map(|operand| eval_capture(operand, state, gc, fixes))
        .collect()
}

/// 求值闭包捕获; 递归组内的槽位共享写一次单元, 其余按值捕获.
fn eval_capture(
    operand: &Operand,
    state: &LinearState,
    gc: &GcRegistry,
    fixes: Option<&HashMap<u32, FixCell>>,
) -> Result<Capture, Error> {
    match operand {
        Operand::Var(VarRef::Local(slot)) => {
            if let Some(cell) = fixes.and_then(|fixes| fixes.get(slot)) {
                return Ok(Capture::Fix(Arc::clone(cell)));
            }
            Ok(Capture::Value(local_value(state, *slot)))
        },
        Operand::Var(VarRef::Capture(index)) => state
            .captures
            .get(*index as usize)
            .cloned()
            .ok_or_else(|| Error::internal(format!("捕获 {index} 不存在"))),
        _ => Ok(Capture::Value(eval_operand(operand, state, gc)?)),
    }
}

fn eval_operand(
    operand: &Operand,
    state: &LinearState,
    gc: &GcRegistry,
) -> Result<Value, Error> {
    match operand {
        Operand::I64(value) => Ok(Value::I64(*value)),
        Operand::I32(value) => Ok(Value::I32(*value)),
        Operand::U8(value) => Ok(Value::U8(*value)),
        Operand::U32(value) => Ok(Value::U32(*value)),
        Operand::U64(value) => Ok(Value::U64(*value)),
        Operand::F32(value) => Ok(Value::F32(*value)),
        Operand::F64(value) => Ok(Value::F64(*value)),
        Operand::Char(value) => Ok(Value::Char(*value)),
        Operand::Str(value) => Ok(Value::Str(value.clone())),
        Operand::Bool(value) => Ok(Value::Bool(*value)),
        Operand::Nil => Ok(Value::Nil),
        Operand::Builtin(id) => Ok(Value::Builtin(*id)),
        Operand::Var(VarRef::Local(slot)) => Ok(local_value(state, *slot)),
        Operand::Var(VarRef::Capture(index)) => {
            let capture = state
                .captures
                .get(*index as usize)
                .ok_or_else(|| Error::internal(format!("捕获 {index} 不存在")))?;
            capture.get().ok_or_else(|| match capture {
                Capture::Missing(name) => Error::internal(format!("全局名字 {name} 未绑定")),
                Capture::Value(_) | Capture::Fix(_) => {
                    Error::internal(format!("捕获 {index} 尚未绑定"))
                },
            })
        },
        Operand::Var(VarRef::Id(var)) => {
            Err(Error::internal(format!("线性 IR 未完成解析: {var}")))
        },
        Operand::Var(VarRef::Free(name)) => {
            Err(Error::internal(format!("线性 IR 未完成解析: {name}")))
        },
        Operand::Func(func) => {
            let function = state
                .functions
                .get(&func.name)
                .cloned()
                .ok_or_else(|| Error::internal(format!("线性函数未注册: {}", func.name)))?;
            let captures = eval_captures(&func.captures, state, gc, None)?;
            Ok(closure_linear(&state.functions, &function, captures.into(), gc))
        },
        Operand::List(list) => {
            let values: Vec<Value> = list
                .items
                .iter()
                .map(|item| eval_operand(item, state, gc))
                .collect::<Result<_, _>>()?;
            let list = List::from_values_typed(&list.elem, values)
                .ok_or_else(|| Error::internal("列表元素与静态类型不符"))?;
            Ok(Value::List(Arc::new(list)))
        },
        Operand::Pair { first, second, .. } => Ok(Value::Pair(Arc::new((
            eval_operand(first, state, gc)?,
            eval_operand(second, state, gc)?,
        )))),
        Operand::Data(data) => {
            let fields = match data.fields.as_slice() {
                [] => DataFields::Zero,
                [field] => DataFields::One([eval_operand(field, state, gc)?]),
                [first, second] => DataFields::Two([
                    eval_operand(first, state, gc)?,
                    eval_operand(second, state, gc)?,
                ]),
                fields => {
                    let mut values = Vec::with_capacity(fields.len());
                    for field in fields {
                        values.push(eval_operand(field, state, gc)?);
                    }
                    DataFields::Many(values.into_boxed_slice())
                },
            };
            Ok(data_value(data.ty.clone(), data.variant.clone(), fields))
        },
        Operand::FStr(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    FStrPart::Text(piece) => text.push_str(piece),
                    FStrPart::Interp { value, spec } => {
                        let value = eval_operand(value, state, gc)?;
                        text.push_str(&ksl_value::fmtstr::format_value(
                            &value,
                            spec.as_deref(),
                        )?);
                    },
                }
            }
            Ok(Value::Str(text.into()))
        },
    }
}

/// 线性模式的匹配结果写入参数缓冲, 丢弃字段也占一个参数槽.
fn match_linear(pattern: &PatternIr, value: &Value, bound: &mut Vec<Value>) -> bool {
    match pattern {
        PatternIr::Wildcard => true,
        PatternIr::Variant {
            qualifier,
            name,
            fields,
        } => {
            let Value::Data(data) = value else {
                return false;
            };
            if data.variant != *name {
                return false;
            }
            if let Some(qualifier) = qualifier
                && data.ty != *qualifier
            {
                return false;
            }
            if data.fields.len() != fields.len() {
                return false;
            }
            bound.extend(data.fields.iter().cloned());
            true
        },
    }
}
