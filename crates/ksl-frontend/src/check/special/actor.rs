//! Actor 原语与信箱索引检查.

use ksl_base::name::keywords;
use ksl_base::source::Span;
use ksl_base::types::PidType;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::super::infer::InferTy;
use super::super::{ActorContext, Checker, InferExpr, TypedKind};
use crate::ast::Expr;

impl Checker {
    /// 当前 Actor 上下文; 调用前已通过位置校验.
    pub(super) fn actor(&self) -> &ActorContext {
        self.actor
            .as_ref()
            .expect("validate 应拒绝 Handler/Main 之外的 Actor 原语")
    }

    /// `@Continue`: 只允许在 Handler 体内, 参数匹配 Handler 的状态参数.
    pub(super) fn check_continue(
        &mut self,
        args: &[Expr],
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let actor = self.actor().clone();
        let expected = actor.handler.args;
        Self::check_arity(args, expected.len(), span)?;
        let mut typed = Vec::with_capacity(args.len());
        for (arg, ty) in args.iter().zip(&expected) {
            typed.push(self.check_expr_expect(arg, Some(&InferTy::from_concrete(ty)))?);
        }
        Ok((InferTy::Never, TypedKind::Continue { args: typed }))
    }

    fn expect_pid_typed(&mut self, expr: &Expr) -> Result<(PidType, InferExpr), KslError> {
        let typed = self.check_expr(expr)?;
        let found = self.infer.finalize(&typed.ty)?;
        let pid = PidType::from_type(&found)
            .ok_or_else(|| KslError::new(ErrorKind::NotPid { found }, expr.span))?;
        Ok((pid, typed))
    }

    pub(super) fn check_spawn(
        &mut self,
        handler: &Expr,
        args: &[Expr],
        detached: bool,
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        self.actor();
        let handler = self.check_expr(handler)?;
        let handler_ty = self.infer.finalize(&handler.ty)?;
        let handler_span = handler.span;
        let Some((pid, handler_args)) = handler_ty.as_handler() else {
            return Err(KslError::new(
                ErrorKind::NotHandler { found: handler_ty },
                handler_span,
            ));
        };
        let (ty, args) = self.check_args(handler_args, pid, args, span)?;
        Ok((
            ty,
            TypedKind::Spawn {
                handler: Box::new(handler),
                args,
                detached,
            },
        ))
    }

    pub(super) fn check_send(
        &mut self,
        pid: &Expr,
        index: &Expr,
        message: &Expr,
        _span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        self.actor();
        let (pid, index, message) = self.check_send_message_typed(pid, index, message)?;
        Ok((
            InferTy::Bool,
            TypedKind::Send {
                pid: Box::new(pid),
                index,
                message: Box::new(message),
            },
        ))
    }

    fn check_send_message_typed(
        &mut self,
        pid: &Expr,
        index: &Expr,
        message: &Expr,
    ) -> Result<(InferExpr, u8, InferExpr), KslError> {
        let (pid_ty, pid) = self.expect_pid_typed(pid)?;
        let i = Self::literal_index(index, "信箱索引")?;
        let count = pid_ty.mailboxes.len();
        let Some(expected) = pid_ty.mailboxes.get(i) else {
            return Err(KslError::new(
                ErrorKind::MailboxIndexOutOfRange { index: i, count },
                index.span,
            ));
        };
        let expected = InferTy::from_concrete(expected);
        let message = self.check_expr_expect(message, Some(&expected))?;
        let index = u8::try_from(i).expect("信箱索引不超过 u8");
        Ok((pid, index, message))
    }

    pub(super) fn check_set_timer(
        &mut self,
        pid: &Expr,
        index: &Expr,
        message: &Expr,
        millis: &Expr,
        _span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        self.actor();
        let (pid, index, message) = self.check_send_message_typed(pid, index, message)?;
        let millis = self.check_expr_expect(millis, Some(&InferTy::U64))?;
        Ok((
            InferTy::option(InferTy::U64),
            TypedKind::SetTimer {
                pid: Box::new(pid),
                index,
                message: Box::new(message),
                millis: Box::new(millis),
            },
        ))
    }

    pub(super) fn check_cancel_timer(
        &mut self,
        id: &Expr,
        _span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        self.actor();
        let id = self.check_expr_expect(id, Some(&InferTy::U64))?;
        Ok((InferTy::Bool, TypedKind::CancelTimer { id: Box::new(id) }))
    }

    pub(super) fn check_receive(
        &mut self,
        index: &Expr,
        timeout: &Expr,
        _span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let actor = self.actor().clone();
        let Some((mailboxes, _)) = actor.handler.pid.as_pid() else {
            unreachable!("Handler 的 pid 字段已校验为 Pid 类型");
        };
        let i = Self::literal_index(index, "信箱索引")?;
        let count = mailboxes.len();
        let Some(elem) = mailboxes.get(i) else {
            return Err(KslError::new(
                ErrorKind::MailboxIndexOutOfRange { index: i, count },
                index.span,
            ));
        };
        let timeout = self.check_expr_expect(timeout, Some(&InferTy::option(InferTy::U64)))?;
        let ty = InferTy::option(InferTy::from_concrete(elem));
        Ok((
            ty,
            TypedKind::Receive {
                index: u8::try_from(i).expect("信箱索引不超过 u8"),
                timeout: Box::new(timeout),
            },
        ))
    }

    pub(super) fn check_join(&mut self, pid: &Expr) -> Result<(InferTy, TypedKind), KslError> {
        let (pid_ty, pid) = self.expect_pid_typed(pid)?;
        let ty = InferTy::result(
            InferTy::from_concrete(pid_ty.ret.as_ref()),
            InferTy::data(keywords().exit_reason_.clone()),
        );
        Ok((ty, TypedKind::Join { pid: Box::new(pid) }))
    }

    pub(super) fn check_kill(&mut self, pid: &Expr) -> Result<(InferTy, TypedKind), KslError> {
        let (_, pid) = self.expect_pid_typed(pid)?;
        Ok((InferTy::Bool, TypedKind::Kill { pid: Box::new(pid) }))
    }

    pub(super) fn check_pid_serial(
        &mut self,
        pid: &Expr,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let (_, pid) = self.expect_pid_typed(pid)?;
        Ok((InferTy::U64, TypedKind::PidSerial { pid: Box::new(pid) }))
    }

    pub(super) fn check_halt(&self) -> (InferTy, TypedKind) {
        self.actor();
        (InferTy::Never, TypedKind::Halt)
    }

    pub(super) fn check_sleep(
        &mut self,
        millis: &Expr,
        _span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        self.actor();
        let millis = self.check_expr_expect(millis, Some(&InferTy::U64))?;
        Ok((
            InferTy::Nil,
            TypedKind::Sleep {
                millis: Box::new(millis),
            },
        ))
    }
}
