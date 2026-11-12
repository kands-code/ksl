//! `@Handler` 与 `@Main` 的检查.

use ksl_base::source::Span;
use ksl_base::types::{HandlerType, Type};
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::super::infer::InferTy;
use super::super::{ActorContext, Checker, InferExpr, TypedKind};
use crate::ast::{Expr, TypeExpr};

impl Checker {
    /// `@Handler`: 首参数必须是 Pid, 返回类型与 Pid 的返回类型一致.
    pub(super) fn check_handler(
        &mut self,
        params: &[crate::ast::Param],
        ret: &TypeExpr,
        body: &Expr,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let (param_types, ret_ty) = self.fn_signature(params, ret)?;
        let Some(first) = param_types.first() else {
            return Err(KslError::new(ErrorKind::InvalidHandlerParam, body.span));
        };
        let Some(pid_ret) = first.as_pid().map(|(_, ret)| ret) else {
            return Err(KslError::new(ErrorKind::InvalidHandlerParam, body.span));
        };
        if pid_ret != &ret_ty {
            return Err(KslError::with_subject(
                ErrorKind::TypeMismatch {
                    expected: pid_ret.clone(),
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
        let body = self.with_actor(
            Some(ActorContext {
                handler: handler.clone(),
            }),
            |checker| checker.check_fn_body_inner(params, &param_types, body, &ret_ty),
        )?;
        Ok((
            InferTy::from_concrete(&handler.to_type()),
            TypedKind::Handler {
                params: Self::typed_params(params, &param_types),
                ret: InferTy::from_concrete(&ret_ty),
                body: Box::new(body),
            },
        ))
    }

    /// `@Main`: 仅一个 self 参数, 返回 nil; 体按 Actor 上下文检查.
    pub(in crate::check) fn check_main(
        &mut self,
        params: &[crate::ast::Param],
        ret: &TypeExpr,
        body: &Expr,
        span: Span,
    ) -> Result<(HandlerType, InferExpr), KslError> {
        let (param_types, ret_ty) = self.fn_signature(params, ret)?;
        if param_types.len() != 1 {
            return Err(KslError::new(ErrorKind::InvalidMainParam, span));
        }
        let Some(pid_ret) = param_types[0].as_pid().map(|(_, ret)| ret) else {
            return Err(KslError::new(ErrorKind::InvalidMainParam, span));
        };
        if ret_ty != Type::Nil || *pid_ret != Type::Nil {
            return Err(KslError::new(ErrorKind::InvalidMainReturn, span));
        }
        let handler = HandlerType {
            pid: Box::new(param_types[0].clone()),
            args: Vec::new(),
        };
        let body = self.with_actor(
            Some(ActorContext {
                handler: handler.clone(),
            }),
            |checker| checker.check_fn_body_inner(params, &param_types, body, &ret_ty),
        )?;
        Ok((handler, body))
    }

    /// 由 Main 签名与体构造 typed Main.
    pub(in crate::check) fn typed_main(
        handler: &HandlerType,
        params: &[crate::ast::Param],
        body: InferExpr,
    ) -> (InferTy, TypedKind) {
        let param_types =
            std::iter::once((*handler.pid).clone()).chain(handler.args.iter().cloned());
        let params = params
            .iter()
            .zip(param_types)
            .map(|(param, ty)| ksl_typed::Binder {
                name: Some(param.name.node.clone()),
                ty: InferTy::from_concrete(&ty),
            })
            .collect();
        (
            InferTy::from_concrete(&handler.to_type()),
            TypedKind::Main {
                params,
                ret: InferTy::Nil,
                body: Box::new(body),
            },
        )
    }
}
