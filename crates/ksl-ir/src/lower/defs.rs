use super::support::{goto, wrap_join};
use super::*;

impl Lowerer {
    pub(super) fn stmt(&mut self, expr: &CheckedExpr, body: Cps, top_level: bool) -> Cps {
        match &expr.kind {
            ExprKind::Let { target, value } => {
                if top_level {
                    self.lower_global_let(value, target.name.clone(), target.ty.clone(), body)
                } else {
                    self.lower_to(value, Target::Bind(Self::binder_for(target), Arc::new(body)))
                }
            },
            ExprKind::Def(def) => self.stmt_def(def, body, top_level),
            ExprKind::Fix(defs) => self.stmt_fix(defs, body, top_level),
            _ => self.lower_to(
                expr,
                Target::Bind(Self::binder(Binding::Discard, expr.ty.clone()), Arc::new(body)),
            ),
        }
    }

    pub(super) fn stmt_def(&mut self, def: &CheckedDef, body: Cps, top_level: bool) -> Cps {
        let function = self.lower_def(def);
        let bindings = vec![(def.name.clone(), function)];
        let body = Arc::new(body);
        if top_level {
            Cps::GlobalLetRec { bindings, body }
        } else {
            Cps::LetRec { bindings, body }
        }
    }

    pub(super) fn stmt_fix(&mut self, defs: &[CheckedDef], body: Cps, top_level: bool) -> Cps {
        let bindings = defs
            .iter()
            .map(|def| (def.name.clone(), self.lower_def(def)))
            .collect();
        let body = Arc::new(body);
        if top_level {
            Cps::GlobalLetRec { bindings, body }
        } else {
            Cps::LetRec { bindings, body }
        }
    }

    pub(super) fn lower_def(&mut self, def: &CheckedDef) -> Name {
        let name = self.fresh_function(def.name.clone());
        self.lower_function_named(
            name,
            &def.params,
            def.ret.clone(),
            &def.body,
            FunctionKind::Fn,
            Some(def.name.clone()),
        )
    }

    pub(super) fn lower_function_named(
        &mut self,
        name: Name,
        params: &[CheckedBinder],
        ret: Type,
        body: &CheckedExpr,
        kind: FunctionKind,
        display_name: Option<Name>,
    ) -> Name {
        let function = self.build_function(name.clone(), params, ret, body, kind, display_name);
        self.functions.push(function);
        name
    }

    pub(super) fn build_function(
        &mut self,
        name: Name,
        params: &[CheckedBinder],
        ret: Type,
        body: &CheckedExpr,
        kind: FunctionKind,
        display_name: Option<Name>,
    ) -> Function {
        let params = params.iter().map(Self::binder_for).collect();
        let body = self.lower_to(body, Target::Cont(ContId::RETURN));
        Function {
            name,
            kind,
            params,
            ret,
            body: Arc::new(body),
            display_name,
        }
    }

    pub(super) fn lower_block(&mut self, stmts: &[CheckedExpr], target: Target) -> Cps {
        let Some((last, init)) = stmts.split_last() else {
            return goto(target, Value::Nil);
        };
        match target {
            Target::Cont(cont) => self.sequence(init, Some(last), Target::Cont(cont), false),
            Target::Bind(binder, body) => {
                let join = self.fresh_cont();
                let cont = Arc::new(Cont {
                    id: join,
                    params: vec![binder],
                    body,
                });
                let inner = self.sequence(init, Some(last), Target::Cont(join), false);
                Cps::LetCont {
                    cont,
                    body: Arc::new(inner),
                }
            },
        }
    }

    pub(super) fn lower_match(
        &mut self,
        value: &CheckedExpr,
        arms: &[CheckedMatchArm],
        target: Target,
    ) -> Cps {
        let value_name = self.fresh_temp();
        let value_ty = value.ty.clone();
        let (branch, join) = self.join_target(target);
        let arms = arms
            .iter()
            .map(|arm| MatchArmIr {
                pattern: Self::lower_pattern(&arm.pattern),
                body: Arc::new(self.lower_to(&arm.body, Target::Cont(branch))),
            })
            .collect();
        let match_cps = Cps::Match {
            value: Value::Temp(value_name),
            arms,
        };
        let match_cps = wrap_join(join, match_cps);
        self.lower_to(
            value,
            Target::Bind(
                Self::binder(Binding::Temp(value_name), value_ty),
                Arc::new(match_cps),
            ),
        )
    }

    pub(super) fn join_target(&mut self, target: Target) -> (ContId, Option<Arc<Cont>>) {
        match target {
            Target::Cont(cont) => (cont, None),
            Target::Bind(binder, body) => {
                let join = self.fresh_cont();
                (
                    join,
                    Some(Arc::new(Cont {
                        id: join,
                        params: vec![binder],
                        body,
                    })),
                )
            },
        }
    }

    pub(super) fn lower_pattern(pattern: &CheckedPattern) -> PatternIr {
        match pattern {
            Pattern::Wildcard => PatternIr::Wildcard,
            Pattern::Variant {
                qualifier,
                name,
                fields,
            } => PatternIr::Variant {
                qualifier: qualifier.clone(),
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|field| field.as_ref().map(Self::binder_for))
                    .collect(),
            },
        }
    }

    pub(super) fn lower_data(decl: &CheckedDataDecl) -> DataDecl {
        DataDecl {
            name: decl.name.clone(),
            variants: decl
                .variants
                .iter()
                .map(|variant| VariantDecl {
                    name: variant.name.clone(),
                    fields: variant.fields.clone(),
                })
                .collect(),
        }
    }

    pub(super) fn lower_global_let(
        &mut self,
        value: &CheckedExpr,
        name: Option<Name>,
        ty: Type,
        body: Cps,
    ) -> Cps {
        let temp = self.fresh_temp();
        let global = Cps::GlobalLet {
            name,
            ty: ty.clone(),
            value: Value::Temp(temp),
            body: Arc::new(body),
        };
        self.lower_to(
            value,
            Target::Bind(Self::binder(Binding::Temp(temp), ty), Arc::new(global)),
        )
    }
}
