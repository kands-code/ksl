use super::*;

impl Lowerer {
    pub(super) fn result_cont(&mut self, target: Target, ty: Type) -> Arc<Cont> {
        match target {
            Target::Cont(cont) => {
                let temp = self.fresh_temp();
                Arc::new(Cont {
                    id: self.fresh_cont(),
                    params: vec![Self::binder(Binding::Temp(temp), ty)],
                    body: Arc::new(Cps::Jump {
                        cont,
                        value: Value::Temp(temp),
                    }),
                })
            },
            Target::Bind(binder, body) => Arc::new(Cont {
                id: self.fresh_cont(),
                params: vec![binder],
                body,
            }),
        }
    }

    /// 同步内置调用的结果绑定; 续延目标需要临时值承接结果.
    pub(super) fn eval_target(&mut self, target: Target, ty: Type) -> (Binder, Arc<Cps>) {
        match target {
            Target::Cont(cont) => {
                let temp = self.fresh_temp();
                (
                    Self::binder(Binding::Temp(temp), ty),
                    Arc::new(Cps::Jump {
                        cont,
                        value: Value::Temp(temp),
                    }),
                )
            },
            Target::Bind(binder, body) => (binder, body),
        }
    }

    /// 求值一组实参; 非原子表达式先绑定到临时值.
    pub(super) fn lower_values<'a>(
        &mut self,
        args: impl IntoIterator<Item = &'a CheckedExpr>,
        done: &mut dyn FnMut(Vec<Value>) -> Cps,
    ) -> Cps {
        let args = args.into_iter();
        let mut values = Vec::with_capacity(args.size_hint().0);
        let mut pending = Vec::new();
        for arg in args {
            if let Some(value) = Self::atomic_value(arg) {
                values.push(value);
            } else {
                let temp = self.fresh_temp();
                values.push(Value::Temp(temp));
                pending.push((arg, arg.ty.clone(), temp));
            }
        }
        let mut cps = done(values);
        for (arg, ty, temp) in pending.into_iter().rev() {
            cps = self.lower_to(
                arg,
                Target::Bind(Self::binder(Binding::Temp(temp), ty), Arc::new(cps)),
            );
        }
        cps
    }

    /// 不需要临时绑定的原子表达式.
    pub(super) fn atomic_value(expr: &CheckedExpr) -> Option<Value> {
        Some(match &expr.kind {
            ExprKind::I64(value) => Value::I64(*value),
            ExprKind::I32(value) => Value::I32(*value),
            ExprKind::U8(value) => Value::U8(*value),
            ExprKind::U32(value) => Value::U32(*value),
            ExprKind::U64(value) => Value::U64(*value),
            ExprKind::F32(value) => Value::F32(*value),
            ExprKind::F64(value) => Value::F64(*value),
            ExprKind::Char(value) => Value::Char(*value),
            ExprKind::Str(value) => Value::Str(value.as_str().into()),
            ExprKind::Bool(value) => Value::Bool(*value),
            ExprKind::Nil => Value::Nil,
            ExprKind::Ident(name) => Value::Var(name.clone()),
            _ => return None,
        })
    }

    pub(super) fn binder(target: Binding, ty: Type) -> Binder {
        Binder { target, ty }
    }

    pub(super) fn binder_for(binder: &CheckedBinder) -> Binder {
        Binder {
            target: binder.name.clone().map_or(Binding::Discard, Binding::Named),
            ty: binder.ty.clone(),
        }
    }
}

pub(super) fn goto(target: Target, value: Value) -> Cps {
    match target {
        Target::Cont(cont) => Cps::Jump { cont, value },
        Target::Bind(binder, body) => Cps::Let {
            target: binder,
            value,
            body,
        },
    }
}

pub(super) fn wrap_join(join: Option<Arc<Cont>>, body: Cps) -> Cps {
    match join {
        Some(cont) => Cps::LetCont {
            cont,
            body: Arc::new(body),
        },
        None => body,
    }
}
