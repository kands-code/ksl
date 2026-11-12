//! CPS IR 降级器.

use std::collections::HashSet;
use std::sync::Arc;

use super::model::{
    Binder, Binding, Cont, ContId, Cps, DataDecl, FStrPartIr, Function, FunctionKind,
    MatchArmIr, Module, PatternIr, PrimOp, TempId, Value, VariantDecl,
};
use crate::ast::{
    BindingTarget, DataVariant, DictOpKind, Expr, ExprKind, FStrAstPart, FailKind, MatchArm,
    OptionOpKind, Param, Pattern, PatternField, Special, TypeExpr,
};
use crate::builtin;
use crate::check::{Type, TypeTable};
use crate::source::Span;
use crate::symbol::{Symbol, intern};

/// 把顶层程序降级为 CPS IR.
#[must_use]
pub fn lower_program(program: &crate::ast::Program, types: &TypeTable) -> Module {
    let mut lowerer = Lowerer::new(types);
    let body = lowerer.lower_sequence(
        &program.stmts,
        program.tail.as_ref(),
        Target::Cont(ContId::RETURN),
        true,
    );
    let ret = program
        .tail
        .as_ref()
        .and_then(|tail| types.get(&tail.key()).cloned())
        .unwrap_or(Type::Nil);
    let main = Function {
        // 源码标识符不会包含 `$`; 入口函数用该前缀避免与用户函数同名.
        name: intern("$main"),
        kind: FunctionKind::Fn,
        params: Vec::new(),
        ret,
        cont: ContId::RETURN,
        body: Arc::new(body),
        display_name: None,
    };
    Module {
        data: lowerer.data,
        functions: lowerer.functions,
        main,
    }
}

/// 降级目标: 已有的续延, 或需要绑定结果后继续.
#[derive(Debug, Clone)]
enum Target {
    Cont(ContId),
    Bind(Binder, Arc<Cps>),
}

/// 待回卷的语句包装步骤.
///
/// `Rec` 与 `Skip` 在下降阶段已经处理完副作用, 回卷时只负责构造节点;
/// `Let` 与 `Expr` 的值在回卷时才降级, 以保持临时编号与续延编号的顺序.
enum SeqStep<'e> {
    Let {
        target: &'e BindingTarget,
        value: &'e Expr,
    },
    Rec(Vec<(Symbol, Symbol)>),
    Expr(&'e Expr),
    Skip,
}

struct Lowerer<'a> {
    types: &'a TypeTable,
    data: Vec<DataDecl>,
    functions: Vec<Function>,
    next_temp: u32,
    next_cont: u32,
    next_function: u32,
    used_functions: HashSet<Symbol>,
}

impl<'a> Lowerer<'a> {
    fn new(types: &'a TypeTable) -> Self {
        Self {
            types,
            data: Vec::new(),
            functions: Vec::new(),
            next_temp: 0,
            next_cont: 0,
            next_function: 0,
            used_functions: HashSet::new(),
        }
    }

    /// 取节点类型; 类型表由检查器完整填充, 缺失属于实现缺陷.
    fn type_at(&self, key: crate::ast::NodeKey) -> Type {
        self.types.get(&key).cloned().expect("类型表缺少节点类型")
    }

    fn type_of(&self, expr: &Expr) -> Type {
        self.type_at(expr.key())
    }

    /// 绑定目标的类型; `_` 取值的类型.
    fn binding_ty(&self, target: &crate::ast::BindingTarget, value: &Expr) -> Type {
        target
            .name
            .as_ref()
            .map_or_else(|| self.type_of(value), |name| self.type_at(name.key()))
    }

    fn binder(target: Binding, ty: Type) -> Binder {
        Binder { target, ty }
    }

    fn fresh_temp(&mut self) -> TempId {
        let id = TempId(self.next_temp);
        self.next_temp += 1;
        id
    }

    fn fresh_cont(&mut self) -> ContId {
        // 0 保留给函数返回续延 `$k`.
        self.next_cont += 1;
        ContId(self.next_cont)
    }

    fn fresh_function(&mut self, base: Symbol) -> Symbol {
        if self.used_functions.insert(base) {
            return base;
        }
        loop {
            let candidate = intern(&format!("{base}#{}", self.next_function));
            self.next_function += 1;
            if self.used_functions.insert(candidate) {
                return candidate;
            }
        }
    }

    /// 合成匿名函数与 Handler; 前缀后接单调序号, 与源码名不会冲突.
    fn fresh_synthetic_function(&mut self, prefix: &str) -> Symbol {
        let base = intern(&format!("{prefix}{}", self.next_function));
        self.next_function += 1;
        self.fresh_function(base)
    }

    /// 降级语句序列; `tail` 是可选尾表达式, 无尾表达式时值为 nil.
    ///
    /// 语句从右到左包装: 先得到尾部 CPS, 再让前面的语句逐层包住它.
    /// 待包装步骤存为显式列表, 长语句序列不会随语句数量加深调用栈.
    fn lower_sequence(
        &mut self,
        stmts: &[Expr],
        tail: Option<&Expr>,
        target: Target,
        top_level: bool,
    ) -> Cps {
        // 定义与数据声明在下降时按源码顺序处理, 其余语句留到回卷时处理;
        // 合成名字与临时编号因此保持源码顺序.
        let mut steps = Vec::with_capacity(stmts.len());
        let mut base = None;
        for expr in stmts {
            match &expr.node {
                ExprKind::Special(Special::Def(def)) => {
                    let function = self.lower_def(def);
                    steps.push(SeqStep::Rec(vec![(def.name.node, function)]));
                },
                ExprKind::Special(Special::Fix(defs)) => {
                    let bindings = defs
                        .iter()
                        .map(|def| (def.name.node, self.lower_def(def)))
                        .collect();
                    steps.push(SeqStep::Rec(bindings));
                },
                ExprKind::Special(Special::Data { name, variants }) => {
                    let variants = variants.iter().map(|v| self.data_variant_ir(v)).collect();
                    self.data.push(DataDecl {
                        name: name.node,
                        variants,
                    });
                    steps.push(SeqStep::Skip);
                },
                ExprKind::Special(Special::Export { .. } | Special::Use { .. }) => {
                    steps.push(SeqStep::Skip);
                },
                ExprKind::Special(Special::Exit { code }) => {
                    // `@Exit` 之后的语句不可达, 不降级它们.
                    base = Some(self.lower_exit(code));
                    break;
                },
                ExprKind::Special(Special::Let { target, value }) => {
                    steps.push(SeqStep::Let {
                        target,
                        value: value.as_ref(),
                    });
                },
                _ => steps.push(SeqStep::Expr(expr)),
            }
        }

        let mut body = match base {
            Some(cps) => cps,
            None => match tail {
                Some(expr) if top_level => self.lower_top_expr(expr, target),
                Some(expr) => self.lower_to(expr, target),
                None => goto(target, Value::Nil),
            },
        };
        for step in steps.into_iter().rev() {
            body = self.wrap_stmt(step, body, top_level);
        }
        body
    }

    /// 把一条语句包到已降级的后续语句上.
    fn wrap_stmt(&mut self, step: SeqStep<'_>, body: Cps, top_level: bool) -> Cps {
        match step {
            SeqStep::Let {
                target: binding,
                value,
            } => {
                let ty = self.binding_ty(binding, value);
                let target = binding_target(binding);
                if top_level {
                    let name = match target {
                        Binding::Named(name) => Some(name),
                        Binding::Temp(_) | Binding::Discard => None,
                    };
                    self.lower_global_let(value, name, ty, Arc::new(body))
                } else {
                    self.lower_to(value, Target::Bind(Self::binder(target, ty), Arc::new(body)))
                }
            },
            SeqStep::Rec(bindings) => Self::let_rec(bindings, Arc::new(body), top_level),
            SeqStep::Expr(expr) => {
                let ty = self.type_of(expr);
                let discard = self.fresh_temp();
                self.lower_to(
                    expr,
                    Target::Bind(Self::binder(Binding::Temp(discard), ty), Arc::new(body)),
                )
            },
            SeqStep::Skip => body,
        }
    }

    /// 顶层尾表达式: `@Let`/`@Def`/`@Fix` 在尾位置也要写入全局环境.
    fn lower_top_expr(&mut self, expr: &Expr, target: Target) -> Cps {
        match &expr.node {
            ExprKind::Special(Special::Let {
                target: binding,
                value,
            }) => {
                let name = binding.name.as_ref().map(|name| name.node);
                let ty = self.binding_ty(binding, value);
                self.lower_global_let(value, name, ty, Arc::new(goto(target, Value::Nil)))
            },
            ExprKind::Special(Special::Def(def)) => {
                let function = self.lower_def(def);
                Self::let_rec(
                    vec![(def.name.node, function)],
                    Arc::new(goto(target, Value::Nil)),
                    true,
                )
            },
            ExprKind::Special(Special::Fix(defs)) => {
                let bindings = defs
                    .iter()
                    .map(|def| (def.name.node, self.lower_def(def)))
                    .collect();
                Self::let_rec(bindings, Arc::new(goto(target, Value::Nil)), true)
            },
            ExprKind::Special(Special::Data { name, variants }) => {
                let variants = variants.iter().map(|v| self.data_variant_ir(v)).collect();
                self.data.push(DataDecl {
                    name: name.node,
                    variants,
                });
                goto(target, Value::Nil)
            },
            _ => self.lower_to(expr, target),
        }
    }

    fn lower_to(&mut self, expr: &Expr, target: Target) -> Cps {
        match &expr.node {
            ExprKind::I64(value) => goto(target, Value::I64(*value)),
            ExprKind::U8(value) => goto(target, Value::U8(*value)),
            ExprKind::U64(value) => goto(target, Value::U64(*value)),
            ExprKind::F64(value) => goto(target, Value::F64(*value)),
            ExprKind::Char(value) => goto(target, Value::Char(*value)),
            ExprKind::Str(value) => goto(target, Value::Str(Arc::from(value.as_str()))),
            ExprKind::Bool(value) => goto(target, Value::Bool(*value)),
            ExprKind::Nil => goto(target, Value::Nil),
            ExprKind::Ident(name) => match builtin::id(*name) {
                Some(id) => goto(target, Value::Builtin(id)),
                None => goto(target, Value::Var(*name)),
            },
            ExprKind::FStr(parts) => self.lower_fstr(parts, &target),
            ExprKind::List(items) => {
                let elem = match self.type_of(expr) {
                    Type::List(elem) => *elem,
                    other => unreachable!("列表字面量类型应为 List, 实际 {other}"),
                };
                self.lower_values(items, 0, Vec::new(), &mut |values| {
                    goto(
                        target.clone(),
                        Value::List {
                            elem: elem.clone(),
                            items: values,
                        },
                    )
                })
            },
            ExprKind::Pair(first, second) => {
                let ty = self.type_of(expr);
                let exprs = [first.as_ref().clone(), second.as_ref().clone()];
                self.lower_values(&exprs, 0, Vec::new(), &mut |mut values| {
                    let second = values.pop().expect("配对有两个元素");
                    let first = values.pop().expect("配对有两个元素");
                    goto(
                        target.clone(),
                        Value::Pair {
                            ty: ty.clone(),
                            first: Box::new(first),
                            second: Box::new(second),
                        },
                    )
                })
            },
            ExprKind::Call { head, args } => {
                if let Some(qualifier) = &head.qualifier {
                    let ty = qualifier.node;
                    let variant = head.name.node;
                    self.lower_values(args, 0, Vec::new(), &mut |fields| {
                        goto(
                            target.clone(),
                            Value::Data {
                                ty,
                                variant,
                                fields,
                            },
                        )
                    })
                } else {
                    let callee = Value::Var(head.name.node);
                    let result_ty = self.type_of(expr);
                    self.lower_call(&callee, args, target, expr.span, result_ty)
                }
            },
            ExprKind::Special(special) => {
                let result_ty = self.type_of(expr);
                self.lower_special(special, target, expr.span, result_ty)
            },
        }
    }

    fn lower_call(
        &mut self,
        callee: &Value,
        args: &[Expr],
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        match target {
            Target::Cont(cont) if cont == ContId::RETURN => {
                self.lower_values(args, 0, Vec::new(), &mut |values| Cps::TailCall {
                    callee: callee.clone(),
                    args: values,
                    span,
                })
            },
            Target::Cont(cont) => {
                let result = self.fresh_temp();
                let binder = Self::binder(Binding::Temp(result), result_ty);
                let call_cont = Arc::new(Cont {
                    id: self.fresh_cont(),
                    params: vec![binder],
                    body: Arc::new(Cps::Jump {
                        cont,
                        value: Value::Temp(result),
                    }),
                });
                self.lower_values(args, 0, Vec::new(), &mut |values| Cps::Call {
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
                self.lower_values(args, 0, Vec::new(), &mut |values| Cps::Call {
                    callee: callee.clone(),
                    args: values,
                    cont: cont.clone(),
                    span,
                })
            },
        }
    }

    fn lower_prim(
        &mut self,
        op: PrimOp,
        args: &[Expr],
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        let arg_types: Vec<Type> = args.iter().map(|arg| self.type_of(arg)).collect();
        let prim_ty = result_ty.clone();
        let cont = self.result_cont(target, result_ty);
        self.lower_values(args, 0, Vec::new(), &mut |values| Cps::Prim {
            op,
            args: values,
            arg_types: arg_types.clone(),
            ty: prim_ty.clone(),
            cont: cont.clone(),
            span,
        })
    }

    /// 构造 `DictPut` 的续延.
    #[allow(clippy::too_many_arguments)]
    fn dict_put_cps(
        &mut self,
        dict: Value,
        key: Value,
        value: Value,
        dict_ty: Type,
        key_ty: Type,
        value_ty: Type,
        target: Target,
        span: Span,
    ) -> Cps {
        let result_name = self.fresh_temp();
        let cont = Cont {
            id: self.fresh_cont(),
            params: vec![Self::binder(Binding::Temp(result_name), dict_ty.clone())],
            body: Arc::new(goto(target, Value::Temp(result_name))),
        };
        Cps::Prim {
            op: PrimOp::Dict(DictOpKind::Put),
            args: vec![dict, key, value],
            arg_types: vec![dict_ty.clone(), key_ty, value_ty],
            ty: dict_ty,
            cont: Arc::new(cont),
            span,
        }
    }

    /// `DictUpdate` 的惰性降级: 缺键时才降级默认值, 命中时才调用更新函数.
    fn lower_dict_update(&mut self, args: &[Expr], target: Target, span: Span) -> Cps {
        let [dict, key, function, fallback] = args else {
            return goto(target, Value::Nil);
        };
        let dict_ty = self.type_of(dict);
        let key_ty = self.type_of(key);
        let fn_ty = self.type_of(function);
        let Type::Dict(_, value_ty) = &dict_ty else {
            return goto(target, Value::Nil);
        };
        let value_ty = (**value_ty).clone();
        let dict_name = self.fresh_temp();
        let key_name = self.fresh_temp();
        let option_name = self.fresh_temp();
        let old_name = self.fresh_temp();
        let new_name = self.fresh_temp();
        let fn_name = self.fresh_temp();
        let default_name = self.fresh_temp();
        let option_ty = Type::OptionT(Box::new(value_ty.clone()));
        let dict_binder = Self::binder(Binding::Temp(dict_name), dict_ty.clone());
        let key_binder = Self::binder(Binding::Temp(key_name), key_ty.clone());
        let fn_binder = Self::binder(Binding::Temp(fn_name), fn_ty);

        let put_new = self.dict_put_cps(
            Value::Temp(dict_name),
            Value::Temp(key_name),
            Value::Temp(new_name),
            dict_ty.clone(),
            key_ty.clone(),
            value_ty.clone(),
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
        let some_body = self.lower_to(function, Target::Bind(fn_binder, Arc::new(call_cps)));
        let some_arm = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(intern("Option")),
                name: intern("Some"),
                fields: vec![Some(Self::binder(Binding::Temp(old_name), value_ty.clone()))],
            },
            body: Arc::new(some_body),
        };
        let put_default = self.dict_put_cps(
            Value::Temp(dict_name),
            Value::Temp(key_name),
            Value::Temp(default_name),
            dict_ty.clone(),
            key_ty.clone(),
            value_ty.clone(),
            target,
            span,
        );
        let none_body = self.lower_to(
            fallback,
            Target::Bind(
                Self::binder(Binding::Temp(default_name), value_ty.clone()),
                Arc::new(put_default),
            ),
        );
        let none_arm = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(intern("Option")),
                name: intern("None"),
                fields: Vec::new(),
            },
            body: Arc::new(none_body),
        };
        let match_cps = Cps::Match {
            value: Value::Temp(option_name),
            arms: vec![some_arm, none_arm],
        };
        let cont = Cont {
            id: self.fresh_cont(),
            params: vec![Self::binder(Binding::Temp(option_name), option_ty.clone())],
            body: Arc::new(match_cps),
        };
        let get_cps = Cps::Prim {
            op: PrimOp::Dict(DictOpKind::Lookup),
            args: vec![Value::Temp(dict_name), Value::Temp(key_name)],
            arg_types: vec![dict_ty, key_ty],
            ty: option_ty,
            cont: Arc::new(cont),
            span,
        };
        let key_cps = self.lower_to(key, Target::Bind(key_binder, Arc::new(get_cps)));
        self.lower_to(dict, Target::Bind(dict_binder, Arc::new(key_cps)))
    }

    /// `DictGet` 的惰性降级: 命中时取字典值, 缺键时才降级默认表达式.
    fn lower_dict_get(
        &mut self,
        args: &[Expr],
        target: Target,
        span: Span,
        result_ty: &Type,
    ) -> Cps {
        let [dict, key, fallback] = args else {
            return goto(target, Value::Nil);
        };
        let dict_ty = self.type_of(dict);
        let key_ty = self.type_of(key);
        let dict_name = self.fresh_temp();
        let key_name = self.fresh_temp();
        let option_name = self.fresh_temp();
        let value_name = self.fresh_temp();
        let option_ty = Type::OptionT(Box::new(result_ty.clone()));
        let dict_binder = Self::binder(Binding::Temp(dict_name), dict_ty.clone());
        let key_binder = Self::binder(Binding::Temp(key_name), key_ty.clone());
        let value_binder = Self::binder(Binding::Temp(value_name), result_ty.clone());
        let present = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(intern("Option")),
                name: intern("Some"),
                fields: vec![Some(value_binder)],
            },
            body: Arc::new(goto(target.clone(), Value::Temp(value_name))),
        };
        let absent = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(intern("Option")),
                name: intern("None"),
                fields: Vec::new(),
            },
            body: Arc::new(self.lower_to(fallback, target)),
        };
        let match_cps = Cps::Match {
            value: Value::Temp(option_name),
            arms: vec![present, absent],
        };
        let cont = Cont {
            id: self.fresh_cont(),
            params: vec![Self::binder(Binding::Temp(option_name), option_ty.clone())],
            body: Arc::new(match_cps),
        };
        let get_cps = Cps::Prim {
            op: PrimOp::Dict(DictOpKind::Lookup),
            args: vec![Value::Temp(dict_name), Value::Temp(key_name)],
            arg_types: vec![dict_ty, key_ty],
            ty: option_ty,
            cont: Arc::new(cont),
            span,
        };
        let key_cps = self.lower_to(key, Target::Bind(key_binder, Arc::new(get_cps)));
        self.lower_to(dict, Target::Bind(dict_binder, Arc::new(key_cps)))
    }

    /// `@OptionGet` 与 `@ResultGet` 降级为分支, 默认值只在该分支执行.
    ///
    /// 与 `@Match` 复用同一机制: 默认值的 IR 已存在, 只是位于未选中的分支,
    /// 运行时不会求值.
    fn lower_extract(
        &mut self,
        op: OptionOpKind,
        args: &[Expr],
        target: Target,
        result_ty: Type,
    ) -> Cps {
        let [input, fallback] = args else {
            // 实参个数已由检查器保证; 保持降级不 panic.
            return goto(target, Value::Nil);
        };
        let input_ty = self.type_of(input);
        let input_name = self.fresh_temp();
        let bound = self.fresh_temp();
        let (branch, join) = self.join_target(target);
        let (qualifier, present, absent, absent_fields) = match op {
            OptionOpKind::OptionGet => {
                (intern("Option"), intern("Some"), intern("None"), Vec::new())
            },
            _ => (intern("Result"), intern("Ok"), intern("Err"), vec![None]),
        };
        let present_arm = MatchArmIr {
            pattern: PatternIr::Variant {
                qualifier: Some(qualifier),
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
                Self::binder(Binding::Temp(input_name), input_ty),
                Arc::new(match_cps),
            ),
        )
    }

    #[allow(clippy::too_many_lines)]
    fn lower_special(
        &mut self,
        special: &Special,
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        match special {
            Special::Let {
                target: binding,
                value,
            } => {
                let ty = self.binding_ty(binding, value);
                let bound = Self::binder(binding_target(binding), ty);
                let body = goto(target, Value::Nil);
                self.lower_to(value, Target::Bind(bound, Arc::new(body)))
            },
            Special::Block(stmts) => self.lower_block(stmts, target),
            Special::If { cond, then, els } => {
                let cond_name = self.fresh_temp();
                let (branch, join) = self.join_target(target);
                let cond_value = Value::Temp(cond_name);
                let then_cps = self.lower_to(then, Target::Cont(branch));
                let els_cps = self.lower_to(els, Target::Cont(branch));
                let if_cps = Cps::If {
                    cond: cond_value,
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
            },
            Special::Fn { params, ret, body } => {
                let function =
                    self.lower_function("$lambda", params, ret, body, FunctionKind::Fn);
                goto(target, Value::Func(function))
            },
            Special::Def(def) => {
                let function = self.lower_def(def);
                let body = goto(target, Value::Nil);
                Cps::LetRec {
                    bindings: vec![(def.name.node, function)],
                    body: Arc::new(body),
                }
            },
            Special::Exit { code } => self.lower_exit(code),
            Special::Handler { params, ret, body } => {
                let function =
                    self.lower_function("$handler", params, ret, body, FunctionKind::Handler);
                goto(target, Value::Func(function))
            },
            Special::Continue { .. }
            | Special::Spawn { .. }
            | Special::Send { .. }
            | Special::Receive { .. }
            | Special::Join { .. }
            | Special::Kill { .. }
            | Special::Monitor { .. }
            | Special::Unmonitor { .. }
            | Special::Halt
            | Special::Sleep { .. } => self.lower_actor(special, target, span, result_ty),
            Special::Fix(defs) => self.lower_fix(defs, target),
            Special::Data { name, variants } => {
                let variants = variants.iter().map(|v| self.data_variant_ir(v)).collect();
                self.data.push(DataDecl {
                    name: name.node,
                    variants,
                });
                goto(target, Value::Nil)
            },
            // 模块声明由加载器在降级前移除; 直接降级时忽略.
            Special::Export { .. } | Special::Use { .. } => goto(target, Value::Nil),
            Special::Match { value, arms } => self.lower_match(value, arms, target),
            Special::ListOp { op, args } => {
                self.lower_prim(PrimOp::List(*op), args, target, span, result_ty)
            },
            Special::PairOp { op, args } => {
                self.lower_prim(PrimOp::Pair(*op), args, target, span, result_ty)
            },
            Special::Dict { entries } => {
                self.lower_prim(PrimOp::DictLiteral, entries, target, span, result_ty)
            },
            Special::Set { items } => {
                self.lower_prim(PrimOp::SetLiteral, items, target, span, result_ty)
            },
            Special::DictOp {
                op: DictOpKind::Get,
                args,
            } => self.lower_dict_get(args, target, span, &result_ty),
            Special::DictOp {
                op: DictOpKind::Update,
                args,
            } => self.lower_dict_update(args, target, span),
            Special::DictOp { op, args } => {
                self.lower_prim(PrimOp::Dict(*op), args, target, span, result_ty)
            },
            Special::SetOp { op, args } => {
                self.lower_prim(PrimOp::Set(*op), args, target, span, result_ty)
            },
            Special::Registry { op, args } => {
                self.lower_prim(PrimOp::Registry(*op), args, target, span, result_ty)
            },
            Special::PidSerial { pid } => self.lower_prim(
                PrimOp::PidSerial,
                std::slice::from_ref(pid),
                target,
                span,
                result_ty,
            ),
            Special::SetTimer {
                pid,
                index,
                message,
                millis,
            } => {
                let exprs = [
                    pid.as_ref().clone(),
                    index.as_ref().clone(),
                    message.as_ref().clone(),
                    millis.as_ref().clone(),
                ];
                self.lower_prim(PrimOp::SetTimer, &exprs, target, span, result_ty)
            },
            Special::CancelTimer { id } => self.lower_prim(
                PrimOp::CancelTimer,
                std::slice::from_ref(id),
                target,
                span,
                result_ty,
            ),
            Special::OptionOp { op, args } => match op {
                OptionOpKind::OptionGet | OptionOpKind::ResultGet => {
                    self.lower_extract(*op, args, target, result_ty)
                },
                _ => self.lower_prim(PrimOp::Option(*op), args, target, span, result_ty),
            },
            Special::Assert { cond, message } => self.lower_assert(cond, message, target, span),
            Special::Panic { message } => self.lower_fail(FailKind::Panic, message, span),
            Special::Todo { message } => {
                self.lower_fail(FailKind::Unimplemented, message, span)
            },
            Special::Unreachable { message } => {
                self.lower_fail(FailKind::Unreachable, message, span)
            },
        }
    }

    /// `@Assert[条件, 信息]`: 条件为真则继续并给出 nil, 为假则失败.
    ///
    /// 信息字符串只在失败路径求值, 通过时不付构造代价.
    fn lower_assert(&mut self, cond: &Expr, message: &Expr, target: Target, span: Span) -> Cps {
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

    /// 用户主动中断: 先求值信息字符串, 再失败.
    ///
    /// 该节点不返回, 调用方的续延随之丢弃, 与 `@Exit` 与 `@Halt` 一致.
    fn lower_fail(&mut self, kind: FailKind, message: &Expr, span: Span) -> Cps {
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

    /// Actor 原语: Continue, Spawn, Send, Receive, Join, Kill, Halt, Sleep.
    fn lower_actor(
        &mut self,
        special: &Special,
        target: Target,
        span: Span,
        result_ty: Type,
    ) -> Cps {
        match special {
            Special::Continue { args } => {
                self.lower_values(args, 0, Vec::new(), &mut |values| Cps::Continue {
                    args: values,
                })
            },
            Special::Spawn {
                count,
                handler,
                args,
                detached,
            } => {
                let mut exprs = Vec::with_capacity(args.len() + 2);
                exprs.push(count.as_ref().clone());
                exprs.push(handler.as_ref().clone());
                exprs.extend(args.iter().cloned());
                self.lower_prim(
                    PrimOp::Spawn {
                        detached: *detached,
                    },
                    &exprs,
                    target,
                    span,
                    result_ty,
                )
            },
            Special::Send {
                pid,
                index,
                message,
            } => {
                let exprs = [
                    pid.as_ref().clone(),
                    index.as_ref().clone(),
                    message.as_ref().clone(),
                ];
                self.lower_prim(PrimOp::Send, &exprs, target, span, result_ty)
            },
            Special::Receive { index, timeout } => {
                let index = literal_usize(index).expect("检查器已校验信箱索引为字面量");
                let timeout_name = self.fresh_temp();
                let timeout_ty = Type::OptionT(Box::new(Type::U64));
                let cont = self.result_cont(target, result_ty);
                let receive = Cps::Receive {
                    index,
                    timeout: Value::Temp(timeout_name),
                    cont,
                    span,
                };
                self.lower_to(
                    timeout,
                    Target::Bind(
                        Self::binder(Binding::Temp(timeout_name), timeout_ty),
                        Arc::new(receive),
                    ),
                )
            },
            Special::Join { pid } => self.lower_prim(
                PrimOp::Join,
                std::slice::from_ref(pid),
                target,
                span,
                result_ty,
            ),
            Special::Kill { pid } => self.lower_prim(
                PrimOp::Kill,
                std::slice::from_ref(pid),
                target,
                span,
                result_ty,
            ),
            Special::Monitor { pid } => self.lower_prim(
                PrimOp::Monitor,
                std::slice::from_ref(pid),
                target,
                span,
                result_ty,
            ),
            Special::Unmonitor { id } => self.lower_prim(
                PrimOp::Unmonitor,
                std::slice::from_ref(id),
                target,
                span,
                result_ty,
            ),
            Special::Halt => Cps::Halt,
            Special::Sleep { millis } => self.lower_prim(
                PrimOp::Sleep,
                std::slice::from_ref(millis),
                target,
                span,
                result_ty,
            ),
            _ => unreachable!("lower_actor 只接收 Actor 原语"),
        }
    }

    fn lower_global_let(
        &mut self,
        value: &Expr,
        name: Option<Symbol>,
        ty: Type,
        body: Arc<Cps>,
    ) -> Cps {
        let temp = self.fresh_temp();
        let global = Cps::GlobalLet {
            name,
            ty: ty.clone(),
            value: Value::Temp(temp),
            body,
        };
        self.lower_to(
            value,
            Target::Bind(Self::binder(Binding::Temp(temp), ty), Arc::new(global)),
        )
    }

    fn let_rec(bindings: Vec<(Symbol, Symbol)>, body: Arc<Cps>, top_level: bool) -> Cps {
        if top_level {
            Cps::GlobalLetRec { bindings, body }
        } else {
            Cps::LetRec { bindings, body }
        }
    }

    fn lower_fix(&mut self, defs: &[crate::ast::Def], target: Target) -> Cps {
        let bindings = defs
            .iter()
            .map(|def| (def.name.node, self.lower_def(def)))
            .collect();
        Cps::LetRec {
            bindings,
            body: Arc::new(goto(target, Value::Nil)),
        }
    }

    fn lower_exit(&mut self, code: &Expr) -> Cps {
        let value = self.fresh_temp();
        self.lower_to(
            code,
            Target::Bind(
                Self::binder(Binding::Temp(value), Type::U8),
                Arc::new(Cps::Exit(Value::Temp(value))),
            ),
        )
    }

    fn lower_block(&mut self, stmts: &[Expr], target: Target) -> Cps {
        let Some((last, init)) = stmts.split_last() else {
            return goto(target, Value::Nil);
        };
        match target {
            Target::Cont(cont) => {
                self.lower_sequence(init, Some(last), Target::Cont(cont), false)
            },
            Target::Bind(binder, body) => {
                let join = self.fresh_cont();
                let cont = Arc::new(Cont {
                    id: join,
                    params: vec![binder],
                    body,
                });
                let inner = self.lower_sequence(init, Some(last), Target::Cont(join), false);
                Cps::LetCont {
                    cont,
                    body: Arc::new(inner),
                }
            },
        }
    }

    fn lower_match(&mut self, value: &Expr, arms: &[MatchArm], target: Target) -> Cps {
        let value_name = self.fresh_temp();
        let value_ty = self.type_of(value);
        let (branch, join) = self.join_target(target);
        let mut lowered = Vec::with_capacity(arms.len());
        for arm in arms {
            lowered.push(MatchArmIr {
                pattern: self.pattern_ir(&arm.pattern),
                body: Arc::new(self.lower_to(&arm.body, Target::Cont(branch))),
            });
        }
        let match_cps = Cps::Match {
            value: Value::Temp(value_name),
            arms: lowered,
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

    fn lower_fstr(&mut self, parts: &[FStrAstPart], target: &Target) -> Cps {
        let mut exprs = Vec::new();
        for part in parts {
            if let FStrAstPart::Interp { expr, .. } = part {
                exprs.push(expr.as_ref().clone());
            }
        }
        let part_specs: Vec<Option<String>> = parts
            .iter()
            .map(|part| match part {
                FStrAstPart::Text(_) => None,
                FStrAstPart::Interp { spec, .. } => spec.clone(),
            })
            .collect();
        self.lower_values(&exprs, 0, Vec::new(), &mut |values| {
            let mut values = values.into_iter();
            let mut out = Vec::with_capacity(parts.len());
            for (part, spec) in parts.iter().zip(part_specs.iter()) {
                match part {
                    FStrAstPart::Text(text) => out.push(FStrPartIr::Text(text.clone())),
                    FStrAstPart::Interp { .. } => {
                        let value = values.next().expect("插值与求值结果一一对应");
                        out.push(FStrPartIr::Interp {
                            value,
                            spec: spec.clone(),
                        });
                    },
                }
            }
            goto(target.clone(), Value::FStr(out))
        })
    }

    /// 依序求值一组表达式, 每个结果绑定到新临时名, 再交给 `done`.
    ///
    /// 临时名按源码顺序分配, CPS 节点从后往前包装; 用循环代替按元素
    /// 递归, 长列表字面量与长参数列表不再受调用栈深度限制.
    fn lower_values(
        &mut self,
        exprs: &[Expr],
        index: usize,
        acc: Vec<Value>,
        done: &mut dyn FnMut(Vec<Value>) -> Cps,
    ) -> Cps {
        let mut acc = acc;
        let mut pending = Vec::with_capacity(exprs.len().saturating_sub(index));
        for expr in &exprs[index..] {
            let ty = self.type_of(expr);
            let temp = self.fresh_temp();
            acc.push(Value::Temp(temp));
            pending.push((expr, ty, temp));
        }
        let mut cps = done(acc);
        for (expr, ty, temp) in pending.into_iter().rev() {
            cps = self.lower_to(
                expr,
                Target::Bind(Self::binder(Binding::Temp(temp), ty), Arc::new(cps)),
            );
        }
        cps
    }

    fn result_cont(&mut self, target: Target, ty: Type) -> Arc<Cont> {
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

    fn join_target(&mut self, target: Target) -> (ContId, Option<Arc<Cont>>) {
        match target {
            Target::Cont(cont) => (cont, None),
            Target::Bind(binder, body) => {
                let join = self.fresh_cont();
                let cont = Arc::new(Cont {
                    id: join,
                    params: vec![binder],
                    body,
                });
                (join, Some(cont))
            },
        }
    }

    fn lower_def(&mut self, def: &crate::ast::Def) -> Symbol {
        let name = self.fresh_function(def.name.node);
        self.lower_function_named(
            name,
            &def.params,
            &def.ret,
            &def.body,
            FunctionKind::Fn,
            Some(def.name.node),
        )
    }

    fn lower_function(
        &mut self,
        prefix: &str,
        params: &[Param],
        ret: &TypeExpr,
        body: &Expr,
        kind: FunctionKind,
    ) -> Symbol {
        let name = self.fresh_synthetic_function(prefix);
        self.lower_function_named(name, params, ret, body, kind, None)
    }

    fn lower_function_named(
        &mut self,
        name: Symbol,
        params: &[Param],
        ret: &TypeExpr,
        body: &Expr,
        kind: FunctionKind,
        display_name: Option<Symbol>,
    ) -> Symbol {
        let params: Vec<Binder> = params
            .iter()
            .map(|param| {
                Self::binder(Binding::Named(param.name.node), self.type_at(param.name.key()))
            })
            .collect();
        let ret = self.type_at(ret.key());
        let body = self.lower_to(body, Target::Cont(ContId::RETURN));
        self.functions.push(Function {
            name,
            kind,
            params,
            ret,
            cont: ContId::RETURN,
            body: Arc::new(body),
            display_name,
        });
        name
    }

    fn data_variant_ir(&self, variant: &DataVariant) -> VariantDecl {
        VariantDecl {
            name: variant.name.node,
            fields: variant
                .fields
                .iter()
                .map(|field| self.type_at(field.key()))
                .collect(),
        }
    }

    fn pattern_ir(&self, pattern: &Pattern) -> PatternIr {
        let Some(variant) = &pattern.variant else {
            return PatternIr::Wildcard;
        };
        PatternIr::Variant {
            qualifier: pattern.qualifier.as_ref().map(|name| name.node),
            name: variant.node,
            fields: pattern
                .fields
                .iter()
                .map(|field| match field {
                    PatternField::Bind(name) => Some(Self::binder(
                        Binding::Named(name.node),
                        self.type_at(name.key()),
                    )),
                    PatternField::Wildcard => None,
                })
                .collect(),
        }
    }
}

/// 把值交给目标; 不依赖 lowering 状态.
fn goto(target: Target, value: Value) -> Cps {
    match target {
        Target::Cont(cont) => Cps::Jump { cont, value },
        Target::Bind(target, body) => Cps::Let {
            target,
            value,
            body,
        },
    }
}

fn wrap_join(join: Option<Arc<Cont>>, body: Cps) -> Cps {
    match join {
        Some(cont) => Cps::LetCont {
            cont,
            body: Arc::new(body),
        },
        None => body,
    }
}

/// 绑定目标的统一表示; `_` 不写入环境.
fn binding_target(target: &crate::ast::BindingTarget) -> Binding {
    target
        .name
        .as_ref()
        .map_or(Binding::Discard, |name| Binding::Named(name.node))
}

fn literal_usize(expr: &Expr) -> Option<usize> {
    match &expr.node {
        ExprKind::I64(value) if *value >= 0 => usize::try_from(*value).ok(),
        ExprKind::U8(value) => Some(usize::from(*value)),
        _ => None,
    }
}
