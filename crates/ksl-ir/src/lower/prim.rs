use super::*;

/// `@SetTimer` 的操作数; 与 IR 原语字段一一对应.
#[derive(Clone, Copy)]
pub(super) struct TimerOperands<'a> {
    pub pid: &'a CheckedExpr,
    pub index: u8,
    pub message: &'a CheckedExpr,
    pub millis: &'a CheckedExpr,
}

impl Lowerer {
    pub(super) fn lower_dict_get(
        &mut self,
        args: &[CheckedExpr],
        target: Target,
        span: Span,
        result_ty: &Type,
    ) -> Cps {
        let [dict, key, fallback] = args else {
            return Self::internal_failure("@DictGet 的实参个数不符", span);
        };
        let dict_name = self.fresh_temp();
        let key_name = self.fresh_temp();
        let option_name = self.fresh_temp();
        let value_name = self.fresh_temp();
        let option_ty = Type::option(result_ty.clone());
        let present = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(keywords().option_.clone()),
                name: keywords().some_.clone(),
                fields: vec![Some(Self::binder(Binding::Temp(value_name), result_ty.clone()))],
            },
            body: Arc::new(goto(target.clone(), Value::Temp(value_name))),
        };
        let absent = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(keywords().option_.clone()),
                name: keywords().none_.clone(),
                fields: Vec::new(),
            },
            body: Arc::new(self.lower_to(fallback, target)),
        };
        let match_cps = Cps::Match {
            value: Value::Temp(option_name),
            arms: vec![present, absent],
        };
        let get_cps = Cps::Eval {
            op: EvalKind::Prim {
                op: PrimOp::Dict(DictOpKind::Lookup),
                ty: option_ty.clone(),
            },
            args: vec![Value::Temp(dict_name), Value::Temp(key_name)],
            target: Self::binder(Binding::Temp(option_name), option_ty),
            body: Arc::new(match_cps),
            span,
        };
        let key_cps = self.lower_to(
            key,
            Target::Bind(
                Self::binder(Binding::Temp(key_name), key.ty.clone()),
                Arc::new(get_cps),
            ),
        );
        self.lower_to(
            dict,
            Target::Bind(
                Self::binder(Binding::Temp(dict_name), dict.ty.clone()),
                Arc::new(key_cps),
            ),
        )
    }

    pub(super) fn lower_dict_update(
        &mut self,
        args: &[CheckedExpr],
        target: Target,
        span: Span,
    ) -> Cps {
        let [dict, key, function, fallback] = args else {
            return Self::internal_failure("@DictUpdate 的实参个数不符", span);
        };
        let Some(value_ty) = dict.ty.as_dict().map(|(_, value)| value.clone()) else {
            return goto(target, Value::Nil);
        };
        let dict_name = self.fresh_temp();
        let key_name = self.fresh_temp();
        let option_name = self.fresh_temp();
        let old_name = self.fresh_temp();
        let new_name = self.fresh_temp();
        let fn_name = self.fresh_temp();
        let default_name = self.fresh_temp();
        let option_ty = Type::option(value_ty.clone());
        let dict_ty = dict.ty.clone();
        let put_new = self.dict_put_cps(
            Value::Temp(dict_name),
            Value::Temp(key_name),
            Value::Temp(new_name),
            dict_ty.clone(),
            target.clone(),
            span,
        );
        let call_cont = Cont {
            id: self.fresh_cont(),
            params: vec![Self::binder(Binding::Temp(new_name), value_ty.clone())],
            body: Arc::new(put_new),
        };
        let call_cps = Cps::Call {
            callee: Value::Temp(fn_name),
            args: vec![Value::Temp(old_name)],
            cont: Arc::new(call_cont),
            span,
        };
        let some_body = self.lower_to(
            function,
            Target::Bind(
                Self::binder(Binding::Temp(fn_name), function.ty.clone()),
                Arc::new(call_cps),
            ),
        );
        let some_arm = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(keywords().option_.clone()),
                name: keywords().some_.clone(),
                fields: vec![Some(Self::binder(Binding::Temp(old_name), value_ty.clone()))],
            },
            body: Arc::new(some_body),
        };
        let put_default = self.dict_put_cps(
            Value::Temp(dict_name),
            Value::Temp(key_name),
            Value::Temp(default_name),
            dict_ty,
            target,
            span,
        );
        let none_body = self.lower_to(
            fallback,
            Target::Bind(
                Self::binder(Binding::Temp(default_name), value_ty),
                Arc::new(put_default),
            ),
        );
        let none_arm = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(keywords().option_.clone()),
                name: keywords().none_.clone(),
                fields: Vec::new(),
            },
            body: Arc::new(none_body),
        };
        let match_cps = Cps::Match {
            value: Value::Temp(option_name),
            arms: vec![some_arm, none_arm],
        };
        let get_cps = Cps::Eval {
            op: EvalKind::Prim {
                op: PrimOp::Dict(DictOpKind::Lookup),
                ty: option_ty.clone(),
            },
            args: vec![Value::Temp(dict_name), Value::Temp(key_name)],
            target: Self::binder(Binding::Temp(option_name), option_ty),
            body: Arc::new(match_cps),
            span,
        };
        let key_cps = self.lower_to(
            key,
            Target::Bind(
                Self::binder(Binding::Temp(key_name), key.ty.clone()),
                Arc::new(get_cps),
            ),
        );
        self.lower_to(
            dict,
            Target::Bind(
                Self::binder(Binding::Temp(dict_name), dict.ty.clone()),
                Arc::new(key_cps),
            ),
        )
    }

    pub(super) fn dict_put_cps(
        &mut self,
        dict: Value,
        key: Value,
        value: Value,
        dict_ty: Type,
        target: Target,
        span: Span,
    ) -> Cps {
        let (binder, body) = self.eval_target(target, dict_ty.clone());
        Cps::Eval {
            op: EvalKind::Prim {
                op: PrimOp::Dict(DictOpKind::Put),
                ty: dict_ty,
            },
            args: vec![dict, key, value],
            target: binder,
            body,
            span,
        }
    }

    pub(super) fn lower_extract(
        &mut self,
        op: OptionOpKind,
        args: &[CheckedExpr],
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        let [input, fallback] = args else {
            return Self::internal_failure("取值操作的实参个数不符", span);
        };
        let input_name = self.fresh_temp();
        let bound = self.fresh_temp();
        let (branch, join) = self.join_target(target);
        let (qualifier, present, absent, absent_fields) = match op {
            OptionOpKind::OptionGet => (
                keywords().option_.clone(),
                keywords().some_.clone(),
                keywords().none_.clone(),
                Vec::new(),
            ),
            _ => (
                keywords().result_.clone(),
                keywords().ok_.clone(),
                keywords().err_.clone(),
                vec![None],
            ),
        };
        let present_arm = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(qualifier.clone()),
                name: present,
                fields: vec![Some(Self::binder(Binding::Temp(bound), result_ty))],
            },
            body: Arc::new(goto(Target::Cont(branch), Value::Temp(bound))),
        };
        let absent_body = Arc::new(self.lower_to(fallback, Target::Cont(branch)));
        let absent_arm = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(qualifier),
                name: absent,
                fields: absent_fields,
            },
            body: absent_body,
        };
        let match_cps = Cps::Match {
            value: Value::Temp(input_name),
            arms: vec![present_arm, absent_arm],
        };
        let match_cps = wrap_join(join, match_cps);
        self.lower_to(
            input,
            Target::Bind(
                Self::binder(Binding::Temp(input_name), input.ty.clone()),
                Arc::new(match_cps),
            ),
        )
    }

    /// 原语调用: 同步原语走 `Eval`, 需要挂起或回调的走 `Prim`.
    pub(super) fn lower_prim_exprs(
        &mut self,
        op: &PrimOp,
        args: &[&CheckedExpr],
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        if op.is_synchronous() {
            let (binder, body) = self.eval_target(target, result_ty.clone());
            let eval = EvalKind::Prim {
                op: op.clone(),
                ty: result_ty,
            };
            return self.lower_values(args.iter().copied(), &mut |values| Cps::Eval {
                op: eval.clone(),
                args: values,
                target: binder.clone(),
                body: Arc::clone(&body),
                span,
            });
        }
        let cont = self.result_cont(target, result_ty.clone());
        let prim_ty = result_ty;
        self.lower_values(args.iter().copied(), &mut |values| Cps::Prim {
            op: op.clone(),
            args: values,
            ty: prim_ty.clone(),
            cont: Arc::clone(&cont),
            span,
        })
    }

    pub(super) fn lower_send(
        &mut self,
        pid: &CheckedExpr,
        index: u8,
        message: &CheckedExpr,
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        let pid_temp = self.fresh_temp();
        let pid_ty = pid.ty.clone();
        let msg_temp = self.fresh_temp();
        let msg_ty = message.ty.clone();
        let call = self.lower_prim_values(
            PrimOp::Send,
            vec![Value::Temp(pid_temp), Value::U8(index), Value::Temp(msg_temp)],
            target,
            span,
            result_ty,
        );
        let with_msg = self.lower_to(
            message,
            Target::Bind(Self::binder(Binding::Temp(msg_temp), msg_ty), Arc::new(call)),
        );
        self.lower_to(
            pid,
            Target::Bind(
                Self::binder(Binding::Temp(pid_temp), pid_ty),
                Arc::new(with_msg),
            ),
        )
    }

    pub(super) fn lower_set_timer(
        &mut self,
        operands: TimerOperands<'_>,
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        let TimerOperands {
            pid,
            index,
            message,
            millis,
        } = operands;
        let pid_temp = self.fresh_temp();
        let pid_ty = pid.ty.clone();
        let msg_temp = self.fresh_temp();
        let msg_ty = message.ty.clone();
        let millis_temp = self.fresh_temp();
        let millis_ty = millis.ty.clone();
        let cont = self.result_cont(target, result_ty.clone());
        let prim = Cps::Prim {
            op: PrimOp::SetTimer,
            args: vec![
                Value::Temp(pid_temp),
                Value::U8(index),
                Value::Temp(msg_temp),
                Value::Temp(millis_temp),
            ],
            ty: result_ty,
            cont,
            span,
        };
        let with_millis = self.lower_to(
            millis,
            Target::Bind(
                Self::binder(Binding::Temp(millis_temp), millis_ty),
                Arc::new(prim),
            ),
        );
        let with_msg = self.lower_to(
            message,
            Target::Bind(
                Self::binder(Binding::Temp(msg_temp), msg_ty),
                Arc::new(with_millis),
            ),
        );
        self.lower_to(
            pid,
            Target::Bind(
                Self::binder(Binding::Temp(pid_temp), pid_ty),
                Arc::new(with_msg),
            ),
        )
    }

    pub(super) fn lower_receive(
        &mut self,
        index: u8,
        timeout: &CheckedExpr,
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        let timeout_temp = self.fresh_temp();
        let timeout_ty = timeout.ty.clone();
        let cont = self.result_cont(target, result_ty);
        let receive = Cps::Receive {
            index: usize::from(index),
            timeout: Value::Temp(timeout_temp),
            cont,
            span,
        };
        self.lower_to(
            timeout,
            Target::Bind(
                Self::binder(Binding::Temp(timeout_temp), timeout_ty),
                Arc::new(receive),
            ),
        )
    }

    pub(super) fn lower_prim_values(
        &mut self,
        op: PrimOp,
        values: Vec<Value>,
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        if op.is_synchronous() {
            let (binder, body) = self.eval_target(target, result_ty.clone());
            Cps::Eval {
                op: EvalKind::Prim { op, ty: result_ty },
                args: values,
                target: binder,
                body,
                span,
            }
        } else {
            let cont = self.result_cont(target, result_ty.clone());
            Cps::Prim {
                op,
                args: values,
                ty: result_ty,
                cont,
                span,
            }
        }
    }
}
