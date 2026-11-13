//! 命名注册表的检查规则.

use super::Checker;
use super::infer::InferTy;
use crate::ast::{Expr, RegistryOp};
use crate::error::{ErrorKind, KslError};
use crate::source::Span;

impl Checker {
    pub(super) fn check_registry(
        &mut self,
        op: RegistryOp,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        if self.actor.is_none() {
            return Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span));
        }
        Self::check_arity(args, op.arity(), span)?;
        match op {
            RegistryOp::Register => {
                self.check_expr_expect(&args[0], Some(&InferTy::Str))?;
                let pid = self.check_expr(&args[1])?;
                let pid = self.infer.finalize(&pid)?;
                if !pid.is_pid() {
                    return Err(KslError::new(ErrorKind::NotPid { found: pid }, args[1].span));
                }
                Ok(InferTy::Bool)
            },
            RegistryOp::WhereIs => {
                self.check_expr_expect(&args[0], Some(&InferTy::Str))?;
                let Some(expected) = expected else {
                    return Err(KslError::new(
                        ErrorKind::UndeterminedType {
                            detail: "@WhereIs 需要 Option[Pid[...]] 期望类型".to_string(),
                        },
                        span,
                    ));
                };
                let shallow = self.infer.shallow(expected);
                let Some(inner) = shallow.as_option() else {
                    return Err(KslError::new(
                        ErrorKind::UndeterminedType {
                            detail: "@WhereIs 需要 Option[Pid[...]] 期望类型".to_string(),
                        },
                        span,
                    ));
                };
                if !self.infer.shallow(inner).is_pid() {
                    return Err(KslError::new(
                        ErrorKind::UndeterminedType {
                            detail: "@WhereIs 需要 Option[Pid[...]] 期望类型".to_string(),
                        },
                        span,
                    ));
                }
                Ok(shallow)
            },
            RegistryOp::Unregister => {
                self.check_expr_expect(&args[0], Some(&InferTy::Str))?;
                Ok(InferTy::Bool)
            },
        }
    }
}
