//! 线性 IR 解释: 槽位, 捕获与基本块.
//!
//! 线性代码已经完成闭包转换; 捕获下标取捕获值, 递归定义共享写一次单元,
//! 变量读取按槽位或捕获下标取值, 不查名字环境. 续延是目标基本块, 跳转时
//! 显式传入参数值.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use super::eval::closure_linear;
use super::{Control, Kont, LinearFrame, LinearState, Machine};
use crate::cps::gc::GcRegistry;
use crate::cps::ir::PatternIr;
use crate::cps::linear::{
    BlockId, FStrPart, Function as LinearFunction, Functions as LinearFunctions, Instr,
    Operand, Terminator, VarRef,
};
use crate::error::RuntimeError as Error;
use crate::symbol::Symbol;
use crate::value::{Capture, Env, ExitReason, FixCell, Value, data_value};

/// 进入基本块: 参数占据前若干连续槽位.
pub(super) fn enter_block(
    function: Arc<LinearFunction>,
    block: BlockId,
    args: Vec<Value>,
    captures: Arc<[Capture]>,
    functions: Arc<LinearFunctions>,
) -> LinearState {
    let target = &function.blocks[block.index() as usize];
    debug_assert_eq!(args.len(), target.param_count, "基本块参数个数不符");
    let mut locals = vec![None; target.slots.len()];
    for (slot, value) in args.into_iter().enumerate() {
        locals[slot] = Some(value);
    }
    LinearState {
        function,
        block,
        instr: 0,
        locals,
        captures,
        functions,
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
    pub(super) fn run_linear(
        &mut self,
        mut state: LinearState,
        gc: &GcRegistry,
        budget: &mut usize,
    ) -> Result<bool, Error> {
        // 局部持有函数 Arc, 使指令与终止指令可直接借用, 避免每步克隆.
        let function = Arc::clone(&state.function);
        // 指令在预算内连续执行; 预算耗尽时停在当前指令之前, 把控制状态
        // 原样留给下一次调度, 不要求以基本块为让出单位.
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
            self.step_term(&block.term, &mut state, gc)?;
            return Ok(false);
        }
    }

    fn step_instr(
        &mut self,
        instr: &Instr,
        state: &mut LinearState,
        gc: &GcRegistry,
    ) -> Result<(), Error> {
        match instr {
            Instr::Let { slot, value } => {
                let value = eval_operand(value, state, gc)?;
                state.locals[*slot as usize] = Some(value);
            },
            Instr::GlobalLet {
                slot,
                name,
                ty: _,
                value,
            } => {
                let value = eval_operand(value, state, gc)?;
                if let Some(slot) = slot {
                    state.locals[*slot as usize] = Some(value.clone());
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

    fn bind_rec_linear(
        &mut self,
        bindings: &[crate::cps::linear::RecBinding],
        state: &mut LinearState,
        global: bool,
        gc: &GcRegistry,
    ) -> Result<(), Error> {
        // 递归组先建写一次单元, 让组内函数可以捕获尚未完成的同伴;
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
            state.locals[binding.slot as usize] = Some(closure.clone());
            if global {
                self.globals.insert(binding.name, closure);
            }
        }
        Ok(())
    }

    // 终止指令分派; 分支与 Terminator 定义对应.
    #[allow(clippy::too_many_lines)]
    fn step_term(
        &mut self,
        term: &Terminator,
        state: &mut LinearState,
        gc: &GcRegistry,
    ) -> Result<(), Error> {
        match term {
            Terminator::Return { value } => {
                let value = eval_operand(value, state, gc)?;
                state.locals.clear();
                self.control = Control::Return(value);
            },
            Terminator::Jump { target, args } => {
                let args = eval_values(args, state, gc)?;
                state.locals.clear();
                let state = enter_block(
                    Arc::clone(&state.function),
                    *target,
                    args,
                    Arc::clone(&state.captures),
                    Arc::clone(&state.functions),
                );
                self.control = Control::Run(state);
            },
            Terminator::TailCall { callee, args, span } => {
                let callee = eval_operand(callee, state, gc)?;
                let args = eval_values(args, state, gc)?;
                state.locals.clear();
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
                let live = eval_values(live, state, gc)?;
                self.push_linear_frame(state, *target, live);
                state.locals.clear();
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
                let args = eval_values(args, state, gc)?;
                let live = eval_values(live, state, gc)?;
                self.push_linear_frame(state, *target, live);
                state.locals.clear();
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
                let live = eval_values(live, state, gc)?;
                self.push_linear_frame(state, *target, live);
                state.locals.clear();
                self.receive(*index, &timeout, *span)?;
            },
            Terminator::If { cond, then, els } => {
                let cond = eval_operand(cond, state, gc)?;
                let Value::Bool(cond) = cond else {
                    return Err(Error::internal("条件不是布尔值"));
                };
                let target = if cond { then } else { els };
                let args = eval_values(&target.args, state, gc)?;
                state.locals.clear();
                let state = enter_block(
                    Arc::clone(&state.function),
                    target.block,
                    args,
                    Arc::clone(&state.captures),
                    Arc::clone(&state.functions),
                );
                self.control = Control::Run(state);
            },
            Terminator::Match { value, arms } => {
                let value = eval_operand(value, state, gc)?;
                let mut found = None;
                for arm in arms {
                    if let Some(bound) = match_linear(&arm.pattern, &value) {
                        found = Some((arm, bound));
                        break;
                    }
                }
                let Some((arm, mut bound)) = found else {
                    return Err(Error::internal("模式匹配没有命中分支"));
                };
                for arg in &arm.live {
                    bound.push(eval_operand(arg, state, gc)?);
                }
                state.locals.clear();
                let state = enter_block(
                    Arc::clone(&state.function),
                    arm.target,
                    bound,
                    Arc::clone(&state.captures),
                    Arc::clone(&state.functions),
                );
                self.control = Control::Run(state);
            },
            Terminator::Continue { args } => {
                let Some((base, handler, pid)) = self.actor.as_ref().map(|actor| {
                    (actor.base_kont, actor.handler.clone(), Arc::clone(&actor.pid))
                }) else {
                    return Err(Error::internal("@Continue 出现在 Handler 外"));
                };
                let mut call_args = Vec::with_capacity(args.len() + 1);
                call_args.push(Value::Pid(pid));
                call_args.extend(eval_values(args, state, gc)?);
                state.locals.clear();
                self.kont.truncate(base);
                self.apply_value(handler, call_args, None)?;
            },
            Terminator::Exit { value } => {
                let value = eval_operand(value, state, gc)?;
                state.locals.clear();
                let Value::U8(code) = value else {
                    return Err(Error::internal("退出码不是 u8"));
                };
                return Err(Error::Exit(code));
            },
            Terminator::Halt => {
                state.locals.clear();
                self.control = Control::ActorExit(ExitReason::Halted);
            },
            Terminator::Fail {
                kind,
                message,
                span,
            } => {
                let message = eval_operand(message, state, gc)?;
                state.locals.clear();
                let Value::Str(text) = message else {
                    return Err(Error::internal("失败信息不是字符串"));
                };
                let kind = crate::error::ErrorKind::from_fail(*kind, text.to_string());
                return Err(Error::runtime(kind, Some(*span)));
            },
        }
        Ok(())
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
    operands
        .iter()
        .map(|operand| eval_operand(operand, state, gc))
        .collect()
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
            state.locals[*slot as usize]
                .clone()
                .map(Capture::Value)
                .ok_or_else(|| Error::internal(format!("槽位 {slot} 尚未绑定")))
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
        Operand::Str(value) => Ok(Value::Str(Arc::clone(value))),
        Operand::Bool(value) => Ok(Value::Bool(*value)),
        Operand::Nil => Ok(Value::Nil),
        Operand::Builtin(id) => Ok(Value::Builtin(*id)),
        Operand::Var(VarRef::Local(slot)) => state.locals[*slot as usize]
            .clone()
            .ok_or_else(|| Error::internal(format!("槽位 {slot} 尚未绑定"))),
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
            Ok(Value::Str(Arc::from(text)))
        },
    }
}

/// 按静态元素类型选择列表存储; 标量元素使用窄存储, 其余用 `Value`.
fn list_from_values(
    elem: &crate::check::Type,
    values: Vec<Value>,
) -> Result<crate::value::List, Error> {
    use crate::check::Type;
    use crate::value::List;
    match elem {
        Type::I64 => {
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let Value::I64(value) = value else {
                    return Err(Error::internal("i64 列表元素不是整数"));
                };
                items.push(value);
            }
            Ok(List::i64s(items))
        },
        Type::U8 => {
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let Value::U8(value) = value else {
                    return Err(Error::internal("u8 列表元素不是 u8"));
                };
                items.push(value);
            }
            Ok(List::u8s(items))
        },
        Type::U64 => {
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let Value::U64(value) = value else {
                    return Err(Error::internal("u64 列表元素不是 u64"));
                };
                items.push(value);
            }
            Ok(List::u64s(items))
        },
        Type::F64 => {
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let Value::F64(value) = value else {
                    return Err(Error::internal("f64 列表元素不是浮点数"));
                };
                items.push(value);
            }
            Ok(List::f64s(items))
        },
        Type::Char => {
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let Value::Char(value) = value else {
                    return Err(Error::internal("char 列表元素不是字符"));
                };
                items.push(value);
            }
            Ok(List::chars(items))
        },
        Type::Bool => {
            let mut items = Vec::with_capacity(values.len());
            for value in values {
                let Value::Bool(value) = value else {
                    return Err(Error::internal("bool 列表元素不是布尔值"));
                };
                items.push(value);
            }
            Ok(List::bools(items))
        },
        _ => Ok(List::from_values(values)),
    }
}

/// 线性模式的匹配结果与字段一一对应, 丢弃字段也占一个参数槽.
fn match_linear(pattern: &PatternIr, value: &Value) -> Option<Vec<Value>> {
    match pattern {
        PatternIr::Wildcard => Some(Vec::new()),
        PatternIr::Variant {
            qualifier,
            name,
            fields,
        } => {
            let Value::Data(data) = value else {
                return None;
            };
            if data.variant != *name {
                return None;
            }
            if let Some(qualifier) = qualifier
                && data.ty != *qualifier
            {
                return None;
            }
            if data.fields.len() != fields.len() {
                return None;
            }
            Some(data.fields.to_vec())
        },
    }
}
