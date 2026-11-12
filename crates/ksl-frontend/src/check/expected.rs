//! 期望类型与结果类型的公共处理.

use ksl_base::source::Span;
use ksl_diagnostics::error::KslError;

use super::Checker;
use super::infer::InferTy;

impl Checker {
    /// 有外层期望时统一到实际类型.
    pub(super) fn unify_expected(
        &mut self,
        expected: Option<&InferTy>,
        actual: &InferTy,
        span: Span,
    ) -> Result<(), KslError> {
        if let Some(expected) = expected {
            self.infer.unify(expected, actual, span)?;
        }
        Ok(())
    }

    /// 外层期望给出结果类型时取之, 否则新建类型变量.
    pub(super) fn expected_or_fresh(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        context: &'static str,
    ) -> InferTy {
        expected
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, context))
    }
}
