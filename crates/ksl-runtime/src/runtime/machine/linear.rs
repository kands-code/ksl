//! 线性 IR 解释: 槽位, 捕获与基本块.
//!
//! 线性代码已经完成闭包转换: 捕获下标取捕获值, 递归定义共享写一次单元,
//! 变量读取按槽位或捕获下标取值, 不查名字环境.
//! 续延是目标基本块, 跳转时显式传入参数值.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use ksl_base::source::Span;
use ksl_base::symbol::Symbol;
use ksl_frontend::check::Type;
use ksl_frontend::error::RuntimeError as Error;

use super::eval::closure_linear;
use super::{Control, Kont, LinearFrame, LinearState, Machine};
use crate::builtin;
use crate::cps::ir::{PatternIr, PrimOp};
use crate::cps::linear::{
    BlockId, FStrPart, Function as LinearFunction, Functions as LinearFunctions, Instr,
    Operand, Terminator, VarRef,
};
use crate::runtime::gc::GcRegistry;
use crate::value::{Capture, Env, ExitReason, FixCell, Value, data_value};

/// 槽位缓冲池的上限. 超出上限的缓冲直接释放, 避免深调用后长期占用大缓冲.
const LOCALS_POOL_CAP: usize = 16;
/// 参数求值缓冲池的上限与可复用容量.
const VALUE_BUFFER_POOL_CAP: usize = 16;
const VALUE_BUFFER_CAPACITY: usize = 64;

impl Machine {
    /// 进入基本块: 参数占据前若干连续槽位; 槽位缓冲优先从池中复用.
    pub(super) fn enter_block(
        &mut self,
        function: Arc<LinearFunction>,
        block: BlockId,
        args: &mut Vec<Value>,
        captures: Arc<[Capture]>,
        functions: Arc<LinearFunctions>,
    ) -> LinearState {
        let target = &function.blocks[block.index() as usize];
        debug_assert_eq!(args.len(), target.param_count, "基本块参数个数不符");
        let mut locals = self.take_locals(target.slots.len());
        for (slot, value) in args.drain(..).enumerate() {
            locals[slot] = value;
        }
        #[cfg(debug_assertions)]
        let defined = {
            let mut defined = vec![false; target.slots.len()];
            defined[..target.param_count].fill(true);
            defined
        };
        LinearState {
            function,
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
    fn take_values(&mut self) -> Vec<Value> {
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
        mut state: LinearState,
        gc: &GcRegistry,
        budget: &mut usize,
    ) -> Result<bool, Error> {
        // 局部持有函数 Arc, 使指令与终止指令可直接借用, 避免每步克隆.
        let function = Arc::clone(&state.function);
        // 指令在预算内连续执行.
        // 预算耗尽时停在当前指令之前, 把控制状态原样留给下一次调度.
        // 不要求以基本块为让出单位.
        loop {
            if *budget == 0 {
                self.control = Control::Run(state);
                return Ok(true);
            }
            let block = &function.blocks[state.block.index() as usize];
            if let Some(instr) = block.instrs.get(state.instr) {
                *budget -= 1;
                state.instr += 1;
                self.step_instr(instr, &mut state, gc)?;
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
                        return Err(Error::internal("条件不是布尔值"));
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
                // 被调静态已知为内置纯函数时直接完成调用: 不建续延帧,
                // 结果直接送入目标块; I/O 内置与其余被调走常规路径.
                Terminator::Call {
                    callee,
                    args,
                    target,
                    live,
                    span,
                } => {
                    if let Some(pure) = Self::pure_builtin(callee) {
                        let value = self.eval_builtin(args, &state, gc, *span, pure)?;
                        let mut values = self.take_values();
                        values.push(value);
                        eval_into(live, &state, gc, &mut values)?;
                        Self::switch_block(&function, &mut state, *target, &mut values);
                        self.recycle_values(values);
                    } else {
                        self.step_term(&block.term, &mut state, gc)?;
                        return Ok(false);
                    }
                },
                Terminator::TailCall { callee, args, span } => {
                    if let Some(pure) = Self::pure_builtin(callee) {
                        let value = self.eval_builtin(args, &state, gc, *span, pure)?;
                        self.recycle_locals(&mut state);
                        self.control = Control::Return(value);
                        return Ok(false);
                    }
                    self.step_term(&block.term, &mut state, gc)?;
                    return Ok(false);
                },
                _ => {
                    if self.step_term(&block.term, &mut state, gc)? {
                        continue;
                    }
                    return Ok(false);
                },
            }
        }
    }

    /// 被调为内置纯函数时返回其函数指针; I/O 内置与其余被调返回 None.
    fn pure_builtin(callee: &Operand) -> Option<builtin::BuiltinFn> {
        let Operand::Builtin(id) = callee else {
            return None;
        };
        builtin::pure(*id)
    }

    /// 在内置纯函数实参缓冲中求值并直接调用.
    ///
    /// 内置纯函数不回调抽象机, 因此缓冲可以在调用间复用.
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
        let value = pure(&self.args, &self.builtin_args, Some(span));
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
        let slots = block.slots.len();
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
pub(super) fn capture_values(globals: &Env, names: &[Symbol]) -> Arc<[Capture]> {
    names
        .iter()
        .map(|name| {
            globals
                .get(name)
                .cloned()
                .map_or(Capture::Missing(*name), Capture::Value)
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
                let Some(builtin::Builtin::Pure(pure)) = builtin::resolve(*callee) else {
                    return Err(Error::internal("融合调用不是内置纯函数"));
                };
                let value = self.eval_builtin(args, state, gc, *span, pure)?;
                set_local(state, *slot, value);
            },
            Instr::Prim {
                slot,
                op,
                args,
                arg_types,
                ty,
                span,
                dead,
            } => {
                let args = eval_values(args, state, gc)?;
                for slot in dead {
                    clear_local(state, *slot);
                }
                let value = self.run_prim_value(*op, args, arg_types, ty, *span)?;
                set_local(state, *slot, value);
            },
            Instr::GlobalLet {
                slot,
                name,
                ty: _,
                value,
            } => {
                let value = eval_operand(value, state, gc)?;
                if let Some(slot) = slot {
                    set_local(state, *slot, value.clone());
                    if let Some(name) = name {
                        self.globals.insert(*name, value);
                    }
                }
            },
            Instr::LetRec { bindings } => self.bind_rec_linear(bindings, state, false, gc)?,
            Instr::GlobalLetRec { bindings } => {
                self.bind_rec_linear(bindings, state, true, gc)?;
            },
        }
        Ok(())
    }

    /// 执行同步原语并取出结果; 原语必须直接产出值.
    fn run_prim_value(
        &mut self,
        op: PrimOp,
        args: Vec<Value>,
        arg_types: &[Type],
        ty: &Type,
        span: Span,
    ) -> Result<Value, Error> {
        self.run_prim(op, args, arg_types, ty, span)?;
        match std::mem::replace(&mut self.control, Control::Halt) {
            Control::Return(value) => Ok(value),
            other => {
                self.control = other;
                Err(Error::internal("同步原语未直接产出值"))
            },
        }
    }

    fn bind_rec_linear(
        &mut self,
        bindings: &[crate::cps::linear::RecBinding],
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
                self.globals.insert(binding.name, closure);
            }
        }
        Ok(())
    }

    // 终止指令分派; 同函数块切换已在 run_linear 内联处理.
    // 返回 true 表示控制已进入目标块, false 表示交回调度层.
    #[allow(clippy::too_many_lines)]
    fn step_term(
        &mut self,
        term: &Terminator,
        state: &mut LinearState,
        gc: &GcRegistry,
    ) -> Result<bool, Error> {
        match term {
            Terminator::Return { value } => {
                let value = eval_operand(value, state, gc)?;
                self.recycle_locals(state);
                self.control = Control::Return(value);
            },
            Terminator::TailCall { callee, args, span } => {
                let callee = eval_operand(callee, state, gc)?;
                let args = eval_values(args, state, gc)?;
                self.recycle_locals(state);
                self.apply_value(callee, args, Some(*span))?;
            },
            Terminator::Call {
                callee,
                args,
                target,
                live,
                span,
            } => {
                let callee = eval_operand(callee, state, gc)?;
                let args = eval_values(args, state, gc)?;
                let live_values = self.take_live(live, state, gc)?;
                self.push_linear_frame(state, *target, live_values);
                self.recycle_locals(state);
                self.apply_value(callee, args, Some(*span))?;
            },
            Terminator::Prim {
                op,
                args,
                arg_types,
                ty,
                target,
                live,
                span,
                ..
            } => {
                if op.is_synchronous() {
                    let args = eval_values(args, state, gc)?;
                    let mut values = self.take_live(live, state, gc)?;
                    // 释放槽位引用, 使实参持有的容器在同步原语内保持唯一,
                    // 原地修改路径不会退化为整树复制.
                    state.locals.clear();
                    let value = self.run_prim_value(*op, args, arg_types, ty, *span)?;
                    values.insert(0, value);
                    let function = Arc::clone(&state.function);
                    Self::switch_block(&function, state, *target, &mut values);
                    self.recycle_values(values);
                    return Ok(true);
                }
                let args = eval_values(args, state, gc)?;
                let live_values = self.take_live(live, state, gc)?;
                self.push_linear_frame(state, *target, live_values);
                self.recycle_locals(state);
                self.run_prim(*op, args, arg_types, ty, *span)?;
            },
            Terminator::Receive {
                index,
                timeout,
                target,
                live,
                span,
            } => {
                let timeout = eval_operand(timeout, state, gc)?;
                let live_values = self.take_live(live, state, gc)?;
                self.push_linear_frame(state, *target, live_values);
                self.recycle_locals(state);
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
                let mut call_args = Vec::with_capacity(args.len() + 1);
                call_args.push(Value::Pid(pid));
                call_args.extend(eval_values(args, state, gc)?);
                self.recycle_locals(state);
                self.kont.clear();
                self.apply_value(handler, call_args, None)?;
            },
            Terminator::Exit { value } => {
                let value = eval_operand(value, state, gc)?;
                self.recycle_locals(state);
                let Value::U8(code) = value else {
                    return Err(Error::internal("退出码不是 u8"));
                };
                return Err(Error::Exit(code));
            },
            Terminator::Halt => {
                self.recycle_locals(state);
                self.control = Control::ActorExit(ExitReason::Halted);
            },
            Terminator::Fail {
                kind,
                message,
                span,
            } => {
                let message = eval_operand(message, state, gc)?;
                self.recycle_locals(state);
                let Value::Str(text) = message else {
                    return Err(Error::internal("失败信息不是字符串"));
                };
                let kind = ksl_frontend::error::ErrorKind::from_fail(*kind, text.to_string());
                return Err(Error::runtime(kind, Some(*span)));
            },
            Terminator::Jump { .. } | Terminator::If { .. } | Terminator::Match { .. } => {
                unreachable!("同函数块切换已在 run_linear 内联处理");
            },
        }
        Ok(false)
    }

    fn push_linear_frame(&mut self, state: &LinearState, target: BlockId, live: Vec<Value>) {
        self.kont.push(Kont::Linear(Box::new(LinearFrame {
            target,
            live,
            function: Arc::clone(&state.function),
            captures: Arc::clone(&state.captures),
            functions: Arc::clone(&state.functions),
        })));
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
        Operand::U8(value) => Ok(Value::U8(*value)),
        Operand::U64(value) => Ok(Value::U64(*value)),
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
        Operand::Func { name, captures } => {
            let function = state
                .functions
                .get(name)
                .cloned()
                .ok_or_else(|| Error::internal(format!("线性函数未注册: {name}")))?;
            let captures = eval_captures(captures, state, gc, None)?;
            Ok(closure_linear(&state.functions, &function, captures.into(), gc))
        },
        Operand::List { elem, items } => {
            let values: Vec<Value> = items
                .iter()
                .map(|item| eval_operand(item, state, gc))
                .collect::<Result<_, _>>()?;
            Ok(Value::List(Arc::new(list_from_values(elem, values)?)))
        },
        Operand::Pair { first, second, .. } => Ok(Value::Pair(Arc::new((
            eval_operand(first, state, gc)?,
            eval_operand(second, state, gc)?,
        )))),
        Operand::Data {
            ty,
            variant,
            fields,
        } => Ok(data_value(
            *ty,
            *variant,
            fields
                .iter()
                .map(|field| eval_operand(field, state, gc))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Operand::FStr(parts) => {
            let mut text = String::new();
            for part in parts {
                match part {
                    FStrPart::Text(piece) => text.push_str(piece),
                    FStrPart::Interp { value, spec } => {
                        let value = eval_operand(value, state, gc)?;
                        text.push_str(&crate::fmtstr::format_value(&value, spec.as_deref())?);
                    },
                }
            }
            Ok(Value::Str(text.into()))
        },
    }
}

/// 按静态元素类型选择列表存储; 标量元素使用紧凑表示, 其余用 `Value`.
fn list_from_values(
    elem: &ksl_frontend::check::Type,
    values: Vec<Value>,
) -> Result<crate::value::List, Error> {
    use ksl_frontend::check::Type;

    use crate::value::List;

    macro_rules! narrow {
        ($variant:ident, $ctor:ident, $message:literal) => {{
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let Value::$variant(item) = value else {
                    return Err(Error::internal($message));
                };
                items.push(item);
            }
            Ok(List::$ctor(items))
        }};
    }

    match elem {
        Type::I64 => narrow!(I64, i64s, "i64 列表元素不是整数"),
        Type::U8 => narrow!(U8, u8s, "u8 列表元素不是 u8"),
        Type::U64 => narrow!(U64, u64s, "u64 列表元素不是 u64"),
        Type::F64 => narrow!(F64, f64s, "f64 列表元素不是浮点数"),
        Type::Char => narrow!(Char, chars, "char 列表元素不是字符"),
        Type::Bool => narrow!(Bool, bools, "bool 列表元素不是布尔值"),
        _ => Ok(List::from_values(values)),
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
