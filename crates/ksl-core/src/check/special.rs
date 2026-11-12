//! 特殊形式的检查规则.

use std::collections::HashSet;

use super::data::check_bindable;
use super::infer::InferTy;
use super::types::{HandlerType, PidType, Type};
use super::{ActorContext, Checker};
use crate::ast::{BindingTarget, Def, Expr, ExprKind, Special, TypeExpr};
use crate::error::{ErrorKind, KslError};
use crate::source::Span;
use crate::symbol::{Symbol, intern, resolve};

impl Checker {
    pub(super) fn check_special_expect(
        &mut self,
        form: &Special,
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        match form {
            // `@Data` 在收集阶段已校验, 语句级不产生值.
            Special::Data { .. } => Ok(InferTy::Nil),
            Special::Export { .. } | Special::Use { .. } => {
                Err(KslError::new(ErrorKind::ModuleFormNotResolved, span))
            },
            Special::Let { target, value } => {
                self.check_let(target, value)?;
                // 绑定语句的值不被使用, 语句级语义由绑定承担.
                Ok(InferTy::Nil)
            },
            Special::Block(stmts) => {
                self.env.push();
                let result = (|| {
                    let Some((last, init)) = stmts.split_last() else {
                        return Ok(InferTy::Nil);
                    };
                    for stmt in init {
                        let ty = self.check_expr_expect(stmt, None)?;
                        if !matches!(self.infer.shallow(&ty), InferTy::Nil | InferTy::Never) {
                            let found = self.infer.finalize(&ty)?;
                            return Err(KslError::with_subject(
                                ErrorKind::UnusedValue { found },
                                stmt.span,
                                "语句值",
                            ));
                        }
                    }
                    self.check_expr_expect(last, expected)
                })();
                self.env.pop();
                result
            },
            Special::If { cond, then, els } => {
                self.expect_type(cond, &Type::Bool)?;
                let result = expected
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@If 结果"));
                let then_ty = self.check_expr_expect(then, Some(&result))?;
                self.infer.unify(&result, &then_ty, then.span)?;
                let els_ty = self.check_expr_expect(els, Some(&result))?;
                self.infer.unify(&result, &els_ty, els.span)?;
                let then_never = matches!(self.infer.shallow(&then_ty), InferTy::Never);
                let els_never = matches!(self.infer.shallow(&els_ty), InferTy::Never);
                if then_never && els_never {
                    Ok(InferTy::Never)
                } else {
                    Ok(result)
                }
            },
            Special::Fn { params, ret, body } => self.check_fn(params, ret, body),
            Special::Def(def) => self.check_def(def),
            Special::Exit { code } => {
                self.expect_type(code, &Type::U8)?;
                Ok(InferTy::Never)
            },
            Special::Handler { params, ret, body } => self.check_handler(params, ret, body),
            Special::Continue { args } => self.check_continue(args, span),
            Special::Spawn {
                count,
                handler,
                args,
                detached: _,
            } => self.check_spawn(count, handler, args, span),
            Special::Send {
                pid,
                index,
                message,
            } => self.check_send(pid, index, message),
            Special::Receive { index, timeout } => self.check_receive(index, timeout, span),
            Special::Join { pid } => self.check_join(pid),
            Special::Kill { pid } => self.check_kill(pid),
            Special::Monitor { pid } => self.check_monitor(pid, span),
            Special::Unmonitor { id } => self.check_unmonitor(id, span),
            Special::PidSerial { pid } => self.check_pid_serial(pid),
            Special::SetTimer {
                pid,
                index,
                message,
                millis,
            } => self.check_set_timer(pid, index, message, millis),
            Special::CancelTimer { id } => self.check_cancel_timer(id),
            Special::Halt => self.check_halt(span),
            Special::Sleep { millis } => self.check_sleep(millis, span),
            Special::Fix(defs) => self.check_fix(defs),
            Special::Match { value, arms } => self.check_match_expect(value, arms, expected),
            Special::ListOp { op, args } => self.check_list_op(*op, args, span, expected),
            Special::PairOp { op, args } => self.check_pair_op(*op, args, span, expected),
            Special::Dict { entries } => self.check_dict_literal(entries, span, expected),
            Special::Set { items } => self.check_set_literal(items, span, expected),
            Special::DictOp { op, args } => self.check_dict_op(*op, args, span, expected),
            Special::SetOp { op, args } => self.check_set_op(*op, args, span, expected),
            Special::Registry { op, args } => self.check_registry(*op, args, span, expected),
            Special::OptionOp { op, args } => self.check_option_op(*op, args, span, expected),
            Special::Assert { .. }
            | Special::Panic { .. }
            | Special::Todo { .. }
            | Special::Unreachable { .. } => self.check_fail(form),
        }
    }

    /// 用户主动中断: 断言与三种主动中止.
    ///
    /// 断言通过时给出 nil, 其余三者类型为 never; 信息字符串一律为 string.
    fn check_fail(&mut self, form: &Special) -> Result<InferTy, KslError> {
        match form {
            Special::Assert { cond, message } => {
                self.expect_type(cond, &Type::Bool)?;
                self.expect_type(message, &Type::Str)?;
                Ok(InferTy::Nil)
            },
            Special::Panic { message }
            | Special::Todo { message }
            | Special::Unreachable { message } => {
                self.expect_type(message, &Type::Str)?;
                Ok(InferTy::Never)
            },
            _ => unreachable!("非用户主动中断的形式"),
        }
    }

    /// 绑定目标: 有标注时以标注为准校验值, 无标注时取值的类型.
    pub(super) fn bind_target(
        &mut self,
        target: &BindingTarget,
        value: &Expr,
    ) -> Result<Type, KslError> {
        if let Some(declared_expr) = &target.ty {
            let declared = self.type_from_expr(declared_expr)?;
            let expected = InferTy::from_concrete(&declared);
            self.check_expr_expect(value, Some(&expected))?;
            Ok(declared)
        } else {
            let ty = self.check_expr(value)?;
            self.infer.finalize(&ty)
        }
    }

    pub(super) fn check_let(
        &mut self,
        target: &BindingTarget,
        value: &Expr,
    ) -> Result<InferTy, KslError> {
        let ty = self.bind_target(target, value)?;
        if let Some(name) = &target.name {
            check_bindable(name)?;
            self.infer_types.insert(name.key(), InferTy::from_concrete(&ty));
            self.env.insert(name.node, ty);
        }
        Ok(InferTy::Nil)
    }

    /// 解析函数签名: 参数类型表与返回类型.
    pub(super) fn fn_signature(
        &mut self,
        params: &[crate::ast::Param],
        ret: &TypeExpr,
    ) -> Result<(Vec<Type>, Type), KslError> {
        let mut param_types = Vec::with_capacity(params.len());
        for param in params {
            param_types.push(self.type_from_expr(&param.ty)?);
        }
        let ret_ty = self.type_from_expr(ret)?;
        Ok((param_types, ret_ty))
    }

    /// 在参数作用域内检查函数体; 普通函数与 Handler 共用.
    pub(super) fn check_fn_body_inner(
        &mut self,
        params: &[crate::ast::Param],
        param_types: &[Type],
        body: &Expr,
        ret_ty: &Type,
    ) -> Result<(), KslError> {
        self.env.push();
        let result = (|| {
            let mut seen = HashSet::new();
            for (param, ty) in params.iter().zip(param_types) {
                check_bindable(&param.name)?;
                if !seen.insert(param.name.node) {
                    return Err(KslError::with_subject(
                        ErrorKind::DuplicateParam,
                        param.name.span,
                        resolve(param.name.node).to_string(),
                    ));
                }
                self.infer_types
                    .insert(param.name.key(), InferTy::from_concrete(ty));
                self.env.insert(param.name.node, ty.clone());
            }
            let body_ty =
                self.check_expr_expect(body, Some(&InferTy::from_concrete(ret_ty)))?;
            // 期望统一已在 check_expr_expect 内完成; 这里只需解析剩余类型变量,
            // 未定类型仍按原位置报告.
            self.infer.finalize(&body_ty)?;
            Ok(())
        })();
        self.env.pop();
        result
    }

    /// 普通函数体: 不继承外层 Handler 上下文.
    pub(super) fn check_fn_body(
        &mut self,
        params: &[crate::ast::Param],
        param_types: &[Type],
        body: &Expr,
        ret_ty: &Type,
    ) -> Result<(), KslError> {
        let saved = self.actor.take();
        let result = self.check_fn_body_inner(params, param_types, body, ret_ty);
        self.actor = saved;
        result
    }

    /// `@Handler`: 首参数必须是 Pid, 返回类型与 Pid 的返回类型一致.
    pub(super) fn check_handler(
        &mut self,
        params: &[crate::ast::Param],
        ret: &TypeExpr,
        body: &Expr,
    ) -> Result<InferTy, KslError> {
        let (param_types, ret_ty) = self.fn_signature(params, ret)?;
        let Some(first) = param_types.first() else {
            return Err(KslError::new(ErrorKind::InvalidHandlerParam, body.span));
        };
        let Type::PidT(pid) = first else {
            return Err(KslError::new(ErrorKind::InvalidHandlerParam, body.span));
        };
        if *pid.ret != ret_ty {
            return Err(KslError::with_subject(
                ErrorKind::TypeMismatch {
                    expected: (*pid.ret).clone(),
                    found: ret_ty,
                },
                body.span,
                "Handler 返回值",
            ));
        }
        let handler = HandlerType {
            pid: Box::new(first.clone()),
            args: param_types[1..].to_vec(),
        };
        let saved = self.actor.replace(ActorContext {
            handler: handler.clone(),
        });
        let result = self.check_fn_body_inner(params, &param_types, body, &ret_ty);
        self.actor = saved;
        result?;
        Ok(InferTy::from_concrete(&Type::HandlerT(Box::new(handler))))
    }

    /// `@Continue`: 只允许在 Handler 体内, 参数匹配 Handler 的状态参数.
    pub(super) fn check_continue(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Result<InferTy, KslError> {
        let Some(actor) = self.actor.clone() else {
            return Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span));
        };
        let expected = actor.handler.args;
        Self::check_arity(args, expected.len(), span)?;
        for (arg, ty) in args.iter().zip(&expected) {
            self.expect_type(arg, ty)?;
        }
        Ok(InferTy::Never)
    }

    /// 从表达式取 u8 字面量; 信箱数与索引都必须静态可定, 且必须带 `u` 后缀.
    pub(super) fn literal_index(expr: &Expr, what: &'static str) -> Result<usize, KslError> {
        match &expr.node {
            ExprKind::U8(value) => Ok(usize::from(*value)),
            ExprKind::I64(_) => Err(KslError::with_subject(
                ErrorKind::TypeMismatch {
                    expected: Type::U8,
                    found: Type::I64,
                },
                expr.span,
                what,
            )),
            _ => Err(KslError::with_subject(
                ErrorKind::LiteralRequired,
                expr.span,
                what,
            )),
        }
    }

    /// 表达式须为 Pid 类型, 否则报 `NotPid` 错误.
    pub(super) fn expect_pid(&mut self, expr: &Expr) -> Result<PidType, KslError> {
        let ty = self.check_expr(expr)?;
        match self.infer.finalize(&ty)? {
            Type::PidT(pid) => Ok(*pid),
            found => Err(KslError::new(ErrorKind::NotPid { found }, expr.span)),
        }
    }

    pub(super) fn check_spawn(
        &mut self,
        count: &Expr,
        handler: &Expr,
        args: &[Expr],
        span: Span,
    ) -> Result<InferTy, KslError> {
        let n = Self::literal_index(count, "信箱数")?;
        self.infer_types.insert(count.key(), InferTy::U8);
        let handler_span = handler.span;
        let handler_ty = self.check_expr(handler)?;
        let handler_ty = self.infer.finalize(&handler_ty)?;
        let handler = match handler_ty {
            Type::HandlerT(handler) => handler,
            found => {
                return Err(KslError::new(ErrorKind::NotHandler { found }, handler_span));
            },
        };
        let Type::PidT(pid) = &*handler.pid else {
            return Err(KslError::new(ErrorKind::InvalidHandlerType, handler_span));
        };
        let expected = pid.mailboxes.len();
        if n != expected {
            return Err(KslError::new(
                ErrorKind::SpawnCountMismatch { expected, found: n },
                count.span,
            ));
        }
        self.check_args(&handler.args, &Type::PidT(pid.clone()), args, span)
    }

    pub(super) fn check_send(
        &mut self,
        pid: &Expr,
        index: &Expr,
        message: &Expr,
    ) -> Result<InferTy, KslError> {
        self.check_send_message(pid, index, message)?;
        Ok(InferTy::Bool)
    }

    fn check_send_message(
        &mut self,
        pid: &Expr,
        index: &Expr,
        message: &Expr,
    ) -> Result<(), KslError> {
        let pid = self.expect_pid(pid)?;
        let i = Self::literal_index(index, "信箱索引")?;
        self.infer_types.insert(index.key(), InferTy::U8);
        let count = pid.mailboxes.len();
        let Some(expected) = pid.mailboxes.get(i) else {
            return Err(KslError::new(
                ErrorKind::MailboxIndexOutOfRange { index: i, count },
                index.span,
            ));
        };
        let expected = InferTy::from_concrete(expected);
        self.check_expr_expect(message, Some(&expected))?;
        Ok(())
    }

    pub(super) fn check_set_timer(
        &mut self,
        pid: &Expr,
        index: &Expr,
        message: &Expr,
        millis: &Expr,
    ) -> Result<InferTy, KslError> {
        self.check_send_message(pid, index, message)?;
        self.expect_type(millis, &Type::U64)?;
        Ok(InferTy::OptionT(Box::new(InferTy::U64)))
    }

    pub(super) fn check_cancel_timer(&mut self, id: &Expr) -> Result<InferTy, KslError> {
        self.expect_type(id, &Type::U64)?;
        Ok(InferTy::Bool)
    }

    pub(super) fn check_receive(
        &mut self,
        index: &Expr,
        timeout: &Expr,
        span: Span,
    ) -> Result<InferTy, KslError> {
        let Some(actor) = self.actor.clone() else {
            return Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span));
        };
        let Type::PidT(pid) = &*actor.handler.pid else {
            unreachable!("Handler 的 pid 字段已校验为 Pid 类型");
        };
        let i = Self::literal_index(index, "信箱索引")?;
        self.infer_types.insert(index.key(), InferTy::U8);
        let count = pid.mailboxes.len();
        let Some(elem) = pid.mailboxes.get(i) else {
            return Err(KslError::new(
                ErrorKind::MailboxIndexOutOfRange { index: i, count },
                index.span,
            ));
        };
        self.expect_type(timeout, &Type::OptionT(Box::new(Type::U64)))?;
        Ok(InferTy::OptionT(Box::new(InferTy::from_concrete(elem))))
    }

    pub(super) fn check_join(&mut self, pid: &Expr) -> Result<InferTy, KslError> {
        let pid = self.expect_pid(pid)?;
        Ok(InferTy::ResultT(
            Box::new(InferTy::from_concrete(pid.ret.as_ref())),
            Box::new(InferTy::Data(intern("ExitReason"))),
        ))
    }

    pub(super) fn check_kill(&mut self, pid: &Expr) -> Result<InferTy, KslError> {
        self.expect_pid(pid)?;
        Ok(InferTy::Bool)
    }

    pub(super) fn check_pid_serial(&mut self, pid: &Expr) -> Result<InferTy, KslError> {
        self.expect_pid(pid)?;
        Ok(InferTy::U64)
    }

    pub(super) fn check_monitor(
        &mut self,
        pid: &Expr,
        span: Span,
    ) -> Result<InferTy, KslError> {
        let Some(actor) = self.actor.clone() else {
            return Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span));
        };
        self.expect_pid(pid)?;
        let Type::PidT(pid_ty) = &*actor.handler.pid else {
            unreachable!("Handler 的 pid 字段已校验为 Pid 类型");
        };
        let Some(mailbox) = pid_ty.mailboxes.last() else {
            unreachable!("Pid 类型至少包含一个信箱");
        };
        let notice =
            Type::Pair(Box::new(Type::U64), Box::new(Type::Data(intern("ExitReason"))));
        if *mailbox != notice {
            return Err(KslError::with_subject(
                ErrorKind::TypeMismatch {
                    expected: notice,
                    found: mailbox.clone(),
                },
                span,
                "监视信箱",
            ));
        }
        Ok(InferTy::OptionT(Box::new(InferTy::U64)))
    }

    pub(super) fn check_unmonitor(
        &mut self,
        id: &Expr,
        span: Span,
    ) -> Result<InferTy, KslError> {
        if self.actor.is_none() {
            return Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span));
        }
        self.expect_type(id, &Type::U64)?;
        Ok(InferTy::Bool)
    }

    pub(super) fn check_halt(&mut self, span: Span) -> Result<InferTy, KslError> {
        if self.actor.is_none() {
            return Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span));
        }
        Ok(InferTy::Never)
    }

    pub(super) fn check_sleep(
        &mut self,
        millis: &Expr,
        span: Span,
    ) -> Result<InferTy, KslError> {
        if self.actor.is_none() {
            return Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span));
        }
        self.expect_type(millis, &Type::U64)?;
        Ok(InferTy::Nil)
    }

    /// 匿名 `@Fn`: 类型完全由参数与返回标注决定.
    pub(super) fn check_fn(
        &mut self,
        params: &[crate::ast::Param],
        ret: &TypeExpr,
        body: &Expr,
    ) -> Result<InferTy, KslError> {
        let (param_types, ret_ty) = self.fn_signature(params, ret)?;
        self.check_fn_body(params, &param_types, body, &ret_ty)?;
        Ok(InferTy::from_concrete(&Type::Fn(param_types, Box::new(ret_ty))))
    }

    /// 具名 `@Def`: 先把签名放进当前作用域, 再检查体, 以支持自递归.
    pub(super) fn check_def(&mut self, def: &Def) -> Result<InferTy, KslError> {
        check_bindable(&def.name)?;
        let (param_types, ret_ty) = self.fn_signature(&def.params, &def.ret)?;
        let fn_ty = Type::Fn(param_types.clone(), Box::new(ret_ty.clone()));
        self.infer_types
            .insert(def.name.key(), InferTy::from_concrete(&fn_ty));
        self.env.insert(def.name.node, fn_ty);
        self.check_fn_body(&def.params, &param_types, &def.body, &ret_ty)?;
        Ok(InferTy::Nil)
    }

    /// `@Fix`: 先登记组内全部签名, 再逐个检查体, 以支持互递归.
    pub(super) fn check_fix(&mut self, defs: &[Def]) -> Result<InferTy, KslError> {
        let mut prebound: Vec<(Symbol, Type, Vec<Type>, Type)> = Vec::new();
        for def in defs {
            check_bindable(&def.name)?;
            if prebound.iter().any(|(name, ..)| *name == def.name.node) {
                return Err(KslError::with_subject(
                    ErrorKind::DuplicateFixName,
                    def.name.span,
                    resolve(def.name.node).to_string(),
                ));
            }
            let (param_types, ret_ty) = self.fn_signature(&def.params, &def.ret)?;
            let fn_ty = Type::Fn(param_types.clone(), Box::new(ret_ty.clone()));
            prebound.push((def.name.node, fn_ty, param_types, ret_ty));
        }
        for (name, fn_ty, ..) in &prebound {
            self.env.insert(*name, fn_ty.clone());
        }
        for (def, (_, fn_ty, param_types, ret_ty)) in defs.iter().zip(&prebound) {
            self.infer_types
                .insert(def.name.key(), InferTy::from_concrete(fn_ty));
            self.check_fn_body(&def.params, param_types, &def.body, ret_ty)?;
        }
        Ok(InferTy::Nil)
    }
}
