use super::prim::TimerOperands;
use super::support::{goto, wrap_join};
use super::*;
use crate::atom::{DataAtom, ListAtom};

impl Lowerer {
    /// 集中分派全部语法形态.
    pub(super) fn lower_to(&mut self, expr: &CheckedExpr, target: Target) -> Cps {
        match &expr.kind {
            ExprKind::I64(value) => goto(target, Value::I64(*value)),
            ExprKind::I32(value) => goto(target, Value::I32(*value)),
            ExprKind::U8(value) => goto(target, Value::U8(*value)),
            ExprKind::U32(value) => goto(target, Value::U32(*value)),
            ExprKind::U64(value) => goto(target, Value::U64(*value)),
            ExprKind::F32(value) => goto(target, Value::F32(*value)),
            ExprKind::F64(value) => goto(target, Value::F64(*value)),
            ExprKind::Char(value) => goto(target, Value::Char(*value)),
            ExprKind::Str(value) => goto(target, Value::Str(value.as_str().into())),
            ExprKind::Bool(value) => goto(target, Value::Bool(*value)),
            ExprKind::Nil | ExprKind::Link => goto(target, Value::Nil),
            ExprKind::Ident(name) => goto(target, Value::Var(name.clone())),
            ExprKind::List(_)
            | ExprKind::Pair(..)
            | ExprKind::ListOp { .. }
            | ExprKind::PairOp { .. }
            | ExprKind::DataGet { .. }
            | ExprKind::DictOp { .. }
            | ExprKind::SetOp { .. }
            | ExprKind::OptionOp { .. }
            | ExprKind::DictLiteral(_)
            | ExprKind::SetLiteral(_) => self.lower_collection_to(expr, target),
            ExprKind::FStr(_)
            | ExprKind::Which { .. }
            | ExprKind::Assert { .. }
            | ExprKind::Panic { .. }
            | ExprKind::Todo { .. }
            | ExprKind::Unreachable { .. } => self.lower_special_value_to(expr, target),
            ExprKind::Apply { .. } | ExprKind::Call { .. } => self.lower_call_to(expr, target),
            ExprKind::Fn { .. }
            | ExprKind::Def(_)
            | ExprKind::Fix(_)
            | ExprKind::Data(_)
            | ExprKind::Match { .. }
            | ExprKind::Handler { .. }
            | ExprKind::Main { .. } => self.lower_function_to(expr, target),
            ExprKind::Spawn { .. }
            | ExprKind::Send { .. }
            | ExprKind::Receive { .. }
            | ExprKind::Join { .. }
            | ExprKind::Kill { .. }
            | ExprKind::PidSerial { .. }
            | ExprKind::SetTimer { .. }
            | ExprKind::CancelTimer { .. }
            | ExprKind::Halt
            | ExprKind::Sleep { .. }
            | ExprKind::Continue { .. } => self.lower_actor_to(expr, target),
            ExprKind::Exit { .. }
            | ExprKind::Let { .. }
            | ExprKind::Block(_)
            | ExprKind::If { .. } => self.lower_control_to(expr, target),
        }
    }

    fn lower_collection_to(&mut self, expr: &CheckedExpr, target: Target) -> Cps {
        match &expr.kind {
            ExprKind::List(items) => {
                let Some(elem) = expr.ty.as_list().cloned() else {
                    unreachable!("列表字面量类型应为 List, 实际 {}", expr.ty);
                };
                self.lower_values(items, &mut |values| {
                    goto(
                        target.clone(),
                        Value::List(Box::new(ListAtom {
                            elem: elem.clone(),
                            items: values,
                        })),
                    )
                })
            },
            ExprKind::Pair(first, second) => {
                self.lower_values([first.as_ref(), second.as_ref()], &mut |mut values| {
                    let second = values.pop().expect("配对有两个元素");
                    let first = values.pop().expect("配对有两个元素");
                    goto(
                        target.clone(),
                        Value::Pair {
                            first: Box::new(first),
                            second: Box::new(second),
                        },
                    )
                })
            },
            ExprKind::ListOp { op, args } => self.lower_prim_exprs(
                &PrimOp::List(*op),
                &args.iter().collect::<Vec<_>>(),
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::PairOp { op, args } => self.lower_prim_exprs(
                &PrimOp::Pair(*op),
                &args.iter().collect::<Vec<_>>(),
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::DataGet { value, index } => self.lower_prim_exprs(
                &PrimOp::DataGet { index: *index },
                &[value],
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::DictOp { op, args } => match op {
                DictOpKind::Get => self.lower_dict_get(args, target, expr.span, &expr.ty),
                DictOpKind::Update => self.lower_dict_update(args, target, expr.span),
                _ => self.lower_prim_exprs(
                    &PrimOp::Dict(*op),
                    &args.iter().collect::<Vec<_>>(),
                    target,
                    expr.span,
                    expr.ty.clone(),
                ),
            },
            ExprKind::SetOp { op, args } => self.lower_prim_exprs(
                &PrimOp::Set(*op),
                &args.iter().collect::<Vec<_>>(),
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::OptionOp { op, args } => match op {
                OptionOpKind::OptionGet | OptionOpKind::ResultGet => {
                    self.lower_extract(*op, args, target, expr.span, expr.ty.clone())
                },
                _ => {
                    let op = OptionPrim::from_kind(*op).expect("取值操作已由降级分支处理");
                    self.lower_prim_exprs(
                        &PrimOp::Option(op),
                        &args.iter().collect::<Vec<_>>(),
                        target,
                        expr.span,
                        expr.ty.clone(),
                    )
                },
            },
            ExprKind::DictLiteral(entries) => self.lower_prim_exprs(
                &PrimOp::DictLiteral,
                &entries.iter().collect::<Vec<_>>(),
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::SetLiteral(items) => self.lower_prim_exprs(
                &PrimOp::SetLiteral,
                &items.iter().collect::<Vec<_>>(),
                target,
                expr.span,
                expr.ty.clone(),
            ),
            _ => unreachable!("降级分组不匹配"),
        }
    }

    fn lower_special_value_to(&mut self, expr: &CheckedExpr, target: Target) -> Cps {
        match &expr.kind {
            ExprKind::FStr(parts) => self.lower_fstr(parts, &target),
            ExprKind::Which { branches, fallback } => {
                self.lower_which(branches, fallback, target)
            },
            ExprKind::Assert { cond, message } => {
                self.lower_assert(cond, message, target, expr.span)
            },
            ExprKind::Panic { message } => self.lower_fail(FailKind::Panic, message, expr.span),
            ExprKind::Todo { message } => {
                self.lower_fail(FailKind::Unimplemented, message, expr.span)
            },
            ExprKind::Unreachable { message } => {
                self.lower_fail(FailKind::Unreachable, message, expr.span)
            },
            _ => unreachable!("降级分组不匹配"),
        }
    }

    fn lower_call_to(&mut self, expr: &CheckedExpr, target: Target) -> Cps {
        match &expr.kind {
            ExprKind::Apply { callee, args } => {
                let callee_temp = self.fresh_temp();
                let callee_ty = callee.ty.clone();
                let result_ty = expr.ty.clone();
                let call = self.lower_named_call(
                    &Value::Temp(callee_temp),
                    args,
                    target,
                    expr.span,
                    result_ty,
                );
                self.lower_to(
                    callee,
                    Target::Bind(
                        Self::binder(Binding::Temp(callee_temp), callee_ty),
                        Arc::new(call),
                    ),
                )
            },
            ExprKind::Call { callee, args } => {
                let result_ty = expr.ty.clone();
                match callee {
                    Callee::Builtin(id) if is_pure(*id) => {
                        self.lower_builtin_call(*id, args, target, expr.span, result_ty)
                    },
                    Callee::Builtin(id) => self.lower_named_call(
                        &Value::Builtin(*id),
                        args,
                        target,
                        expr.span,
                        result_ty,
                    ),
                    Callee::Named(name) => self.lower_named_call(
                        &Value::Var(name.clone()),
                        args,
                        target,
                        expr.span,
                        result_ty,
                    ),
                    Callee::Data { ty, variant } => self.lower_values(args, &mut |fields| {
                        goto(
                            target.clone(),
                            Value::Data(Box::new(DataAtom {
                                ty: ty.clone(),
                                variant: variant.clone(),
                                fields,
                            })),
                        )
                    }),
                }
            },
            _ => unreachable!("降级分组不匹配"),
        }
    }

    fn lower_function_to(&mut self, expr: &CheckedExpr, target: Target) -> Cps {
        match &expr.kind {
            ExprKind::Fn { params, ret, body } => {
                let name = self.fresh_synthetic_function("$fn");
                self.lower_function_named(
                    name.clone(),
                    params,
                    ret.clone(),
                    body,
                    FunctionKind::Fn,
                    None,
                );
                goto(target, Value::Func(name))
            },
            ExprKind::Def(def) => self.stmt_def(def, goto(target, Value::Nil), false),
            ExprKind::Fix(defs) => self.stmt_fix(defs, goto(target, Value::Nil), false),
            ExprKind::Data(decl) => {
                self.data.push(Self::lower_data(decl));
                goto(target, Value::Nil)
            },
            ExprKind::Match { value, arms } => self.lower_match(value, arms, target),
            ExprKind::Handler { params, ret, body } | ExprKind::Main { params, ret, body } => {
                let name = self.fresh_synthetic_function("$handler");
                self.lower_function_named(
                    name.clone(),
                    params,
                    ret.clone(),
                    body,
                    FunctionKind::Handler,
                    None,
                );
                goto(target, Value::Func(name))
            },
            _ => unreachable!("降级分组不匹配"),
        }
    }

    fn lower_actor_to(&mut self, expr: &CheckedExpr, target: Target) -> Cps {
        match &expr.kind {
            ExprKind::Spawn {
                handler,
                args,
                detached,
            } => {
                let mailboxes = expr.ty.as_pid().map_or(0, |(mailboxes, _)| mailboxes.len());
                let args: Vec<&CheckedExpr> =
                    std::iter::once(handler.as_ref()).chain(args.iter()).collect();
                self.lower_prim_exprs(
                    &PrimOp::Spawn {
                        detached: *detached,
                        mailboxes,
                    },
                    &args,
                    target,
                    expr.span,
                    expr.ty.clone(),
                )
            },
            ExprKind::Send {
                pid,
                index,
                message,
            } => self.lower_send(pid, *index, message, target, expr.span, expr.ty.clone()),
            ExprKind::Receive { index, timeout } => {
                self.lower_receive(*index, timeout, target, expr.span, expr.ty.clone())
            },
            ExprKind::Join { pid } => {
                self.lower_prim_exprs(&PrimOp::Join, &[pid], target, expr.span, expr.ty.clone())
            },
            ExprKind::Kill { pid } => {
                self.lower_prim_exprs(&PrimOp::Kill, &[pid], target, expr.span, expr.ty.clone())
            },
            ExprKind::PidSerial { pid } => self.lower_prim_exprs(
                &PrimOp::PidSerial,
                &[pid],
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::SetTimer {
                pid,
                index,
                message,
                millis,
            } => self.lower_set_timer(
                TimerOperands {
                    pid,
                    index: *index,
                    message,
                    millis,
                },
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::CancelTimer { id } => self.lower_prim_exprs(
                &PrimOp::CancelTimer,
                &[id],
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::Halt => Cps::Halt,
            ExprKind::Sleep { millis } => self.lower_prim_exprs(
                &PrimOp::Sleep,
                &[millis],
                target,
                expr.span,
                expr.ty.clone(),
            ),
            ExprKind::Continue { args } => {
                self.lower_values(args, &mut |values| Cps::Continue { args: values })
            },
            _ => unreachable!("降级分组不匹配"),
        }
    }

    fn lower_control_to(&mut self, expr: &CheckedExpr, target: Target) -> Cps {
        match &expr.kind {
            ExprKind::Exit { code } => {
                let temp = self.fresh_temp();
                self.lower_to(
                    code,
                    Target::Bind(
                        Self::binder(Binding::Temp(temp), Type::U8),
                        Arc::new(Cps::Exit(Value::Temp(temp))),
                    ),
                )
            },
            ExprKind::Let {
                target: binder,
                value,
            } => {
                let body = goto(target, Value::Nil);
                self.lower_to(value, Target::Bind(Self::binder_for(binder), Arc::new(body)))
            },
            ExprKind::Block(stmts) => self.lower_block(stmts, target),
            ExprKind::If { cond, then, els } => {
                let temp = self.fresh_temp();
                let body = Cps::If {
                    cond: Value::Temp(temp),
                    then: Arc::new(self.lower_to(then, target.clone())),
                    els: Arc::new(self.lower_to(els, target)),
                };
                self.lower_to(
                    cond,
                    Target::Bind(Self::binder(Binding::Temp(temp), Type::Bool), Arc::new(body)),
                )
            },
            _ => unreachable!("降级分组不匹配"),
        }
    }

    /// 纯内置调用: 同步求值, 结果直接绑定.
    pub(super) fn lower_builtin_call(
        &mut self,
        callee: BuiltinId,
        args: &[CheckedExpr],
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        let (binder, body) = self.eval_target(target, result_ty);
        self.lower_values(args, &mut |values| Cps::Eval {
            op: EvalKind::Builtin(callee),
            args: values,
            target: binder.clone(),
            body: Arc::clone(&body),
            span,
        })
    }

    /// 用户函数调用: 尾调用复用当前续延, 其余建立续延帧.
    pub(super) fn lower_named_call(
        &mut self,
        callee: &Value,
        args: &[CheckedExpr],
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        match target {
            Target::Cont(cont) if cont == ContId::RETURN => {
                self.lower_values(args, &mut |values| Cps::TailCall {
                    callee: callee.clone(),
                    args: values,
                    span,
                })
            },
            Target::Cont(cont) => {
                let temp = self.fresh_temp();
                let binder = Self::binder(Binding::Temp(temp), result_ty);
                let call_cont = Arc::new(Cont {
                    id: self.fresh_cont(),
                    params: vec![binder],
                    body: Arc::new(Cps::Jump {
                        cont,
                        value: Value::Temp(temp),
                    }),
                });
                self.lower_values(args, &mut |values| Cps::Call {
                    callee: callee.clone(),
                    args: values,
                    cont: call_cont.clone(),
                    span,
                })
            },
            Target::Bind(binder, body) => {
                let cont = Arc::new(Cont {
                    id: self.fresh_cont(),
                    params: vec![binder],
                    body,
                });
                self.lower_values(args, &mut |values| Cps::Call {
                    callee: callee.clone(),
                    args: values,
                    cont: cont.clone(),
                    span,
                })
            },
        }
    }

    pub(super) fn lower_fstr(&mut self, parts: &[FStrPart<Type>], target: &Target) -> Cps {
        let exprs = parts.iter().filter_map(|part| match part {
            FStrPart::Interp { value, .. } => Some(value.as_ref()),
            FStrPart::Text(_) => None,
        });
        self.lower_values(exprs, &mut |values| {
            let mut values = values.into_iter();
            let mut out = Vec::with_capacity(parts.len());
            for part in parts {
                match part {
                    FStrPart::Text(text) => out.push(FStrPartIr::Text(text.clone())),
                    FStrPart::Interp { spec, .. } => {
                        let value = values.next().expect("插值与求值结果一一对应");
                        out.push(FStrPartIr::Interp {
                            value,
                            spec: spec.clone(),
                        });
                    },
                }
            }
            goto((*target).clone(), Value::FStr(out.into_boxed_slice()))
        })
    }

    pub(super) fn lower_which(
        &mut self,
        branches: &[(CheckedExpr, CheckedExpr)],
        fallback: &CheckedExpr,
        target: Target,
    ) -> Cps {
        if branches.is_empty() {
            return self.lower_to(fallback, target);
        }
        let (result_cont, join) = self.join_target(target);
        let mut els_cps = self.lower_to(fallback, Target::Cont(result_cont));
        for (cond, value) in branches.iter().rev() {
            let then_cps = self.lower_to(value, Target::Cont(result_cont));
            let cond_name = self.fresh_temp();
            let if_cps = Cps::If {
                cond: Value::Temp(cond_name),
                then: Arc::new(then_cps),
                els: Arc::new(els_cps),
            };
            els_cps = self.lower_to(
                cond,
                Target::Bind(
                    Self::binder(Binding::Temp(cond_name), Type::Bool),
                    Arc::new(if_cps),
                ),
            );
        }
        wrap_join(join, els_cps)
    }

    pub(super) fn lower_assert(
        &mut self,
        cond: &CheckedExpr,
        message: &CheckedExpr,
        target: Target,
        span: Span,
    ) -> Cps {
        let cond_name = self.fresh_temp();
        let (branch, join) = self.join_target(target);
        let then_cps = goto(Target::Cont(branch), Value::Nil);
        let els_cps = self.lower_fail(FailKind::Assertion, message, span);
        let if_cps = Cps::If {
            cond: Value::Temp(cond_name),
            then: Arc::new(then_cps),
            els: Arc::new(els_cps),
        };
        let if_cps = wrap_join(join, if_cps);
        self.lower_to(
            cond,
            Target::Bind(
                Self::binder(Binding::Temp(cond_name), Type::Bool),
                Arc::new(if_cps),
            ),
        )
    }

    pub(super) fn lower_fail(
        &mut self,
        kind: FailKind,
        message: &CheckedExpr,
        span: Span,
    ) -> Cps {
        let message_name = self.fresh_temp();
        let fail = Cps::Fail {
            kind,
            message: Value::Temp(message_name),
            span,
        };
        self.lower_to(
            message,
            Target::Bind(
                Self::binder(Binding::Temp(message_name), Type::Str),
                Arc::new(fail),
            ),
        )
    }

    pub(super) fn internal_failure(message: &str, span: Span) -> Cps {
        Cps::Fail {
            kind: FailKind::Unreachable,
            message: Value::Str(message.into()),
            span,
        }
    }
}
