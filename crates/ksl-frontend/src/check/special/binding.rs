//! 绑定, 条件分支与 Link 名字检查.

use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::super::data::check_bindable;
use super::super::infer::InferTy;
use super::super::{Checker, InferExpr};
use crate::ast::{BindingTarget, Expr, ExprKind};

/// `@Which` 的 typed 分支与 fallback.
type WhichTyped = (InferTy, Vec<(InferExpr, InferExpr)>, Box<InferExpr>);

impl Checker {
    pub(super) fn bind_target_typed(
        &mut self,
        target: &BindingTarget,
        value: &Expr,
    ) -> Result<(Type, InferExpr), KslError> {
        if let Some(declared_expr) = &target.ty {
            let declared = self.type_from_expr(declared_expr)?;
            let expected = InferTy::from_concrete(&declared);
            let value = self.check_expr_expect(value, Some(&expected))?;
            Ok((declared, value))
        } else {
            let value = self.check_expr(value)?;
            let ty = self.infer.finalize(&value.ty)?;
            Ok((ty, value))
        }
    }

    pub(super) fn check_which_typed(
        &mut self,
        branches: &[(Expr, Expr)],
        fallback: &Expr,
        expected: Option<&InferTy>,
        span: Span,
    ) -> Result<WhichTyped, KslError> {
        let result = expected
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, "@Which 结果"));
        let mut all_never = true;
        let mut typed = Vec::with_capacity(branches.len());
        for (cond, value) in branches {
            if let ExprKind::Bool(value) = &cond.node {
                return Err(KslError::new(
                    ErrorKind::WhichLiteralCondition { value: *value },
                    cond.span,
                ));
            }
            let cond = self.check_expr_expect(cond, Some(&InferTy::Bool))?;
            let value = self.check_expr_expect(value, Some(&result))?;
            self.infer.unify(&result, &value.ty, value.span)?;
            if !matches!(self.infer.shallow(&value.ty), InferTy::Never) {
                all_never = false;
            }
            typed.push((cond, value));
        }
        let fallback = self.check_expr_expect(fallback, Some(&result))?;
        self.infer.unify(&result, &fallback.ty, fallback.span)?;
        if !matches!(self.infer.shallow(&fallback.ty), InferTy::Never) {
            all_never = false;
        }
        let ty = if all_never { InferTy::Never } else { result };
        Ok((ty, typed, Box::new(fallback)))
    }

    /// 登记一个局部值; Link 名字不可被任何作用域遮蔽.
    pub(in crate::check) fn bind_value(
        &mut self,
        name: &crate::ast::Spanned<Name>,
        ty: Type,
    ) -> Result<(), KslError> {
        check_bindable(name)?;
        self.check_link_name(&name.node, name.span)?;
        self.env.insert(name.node.clone(), ty);
        Ok(())
    }

    /// Link 名字在模块内不可被其他绑定使用.
    pub(super) fn check_link_name(&self, name: &Name, span: Span) -> Result<(), KslError> {
        if self.context.link_values.contains(name) {
            return Err(KslError::link_name_conflict(name, span));
        }
        Ok(())
    }
}
