//! Option 与 Result 便捷操作的检查规则.

use ksl_base::source::Span;
use ksl_diagnostics::error::KslError;

use super::infer::InferTy;
use super::{Checker, InferExpr, TypedKind};
use crate::ast::Expr;

impl Checker {
    pub(super) fn check_option_op(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        use crate::ast::OptionOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        let mut typed_args = vec![None; args.len()];
        let ty = match op {
            K::IsSome | K::IsNone | K::IsOk | K::IsErr => {
                self.check_option_predicate(op, args, span, &mut typed_args)?
            },
            K::OptionMap | K::ResultMap | K::OptionAndThen | K::ResultAndThen => {
                self.check_option_map(op, args, span, expected, &mut typed_args)?
            },
            K::OptionGet | K::ResultGet => {
                self.check_option_extract(op, args, span, expected, &mut typed_args)?
            },
            K::ResultOrElse => {
                self.check_option_or_else(op, args, span, expected, &mut typed_args)?
            },
        };
        Ok((
            ty,
            TypedKind::OptionOp {
                op,
                args: Self::collect_typed_args(typed_args),
            },
        ))
    }

    /// 判定类操作: `@IsSome` / `@IsOk` 等, 结果恒为 bool.
    pub(super) fn check_option_predicate(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        match op {
            K::IsSome | K::IsNone => {
                let elem = self.infer.fresh(span, "Option 元素");
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::option(elem)))?;
                Ok(InferTy::Bool)
            },
            K::IsOk | K::IsErr => {
                let ok = self.infer.fresh(span, "Result 成功类型");
                let err = self.infer.fresh(span, "Result 错误类型");
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::result(ok, err)))?;
                Ok(InferTy::Bool)
            },
            _ => unreachable!("check_option_predicate 只接收判定类操作"),
        }
    }

    /// 映射类操作: 输入与结果由函数签名连接.
    pub(super) fn check_option_map(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        match op {
            K::OptionMap => {
                let out = self.expected_option_elem(expected, span, "@OptionMap 结果");
                let result = InferTy::option(out.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@OptionMap 输入");
                let function = InferTy::function(vec![input.clone()], out);
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::option(input)))?;
                Ok(result)
            },
            K::ResultMap => {
                let (out, err) = self.expected_result_parts(
                    expected,
                    span,
                    "@ResultMap 成功",
                    "@ResultMap 错误",
                );
                let result = InferTy::result(out.clone(), err.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ResultMap 输入");
                let function = InferTy::function(vec![input.clone()], out);
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::result(input, err)))?;
                Ok(result)
            },
            K::OptionAndThen => {
                let out = self.expected_option_elem(expected, span, "@OptionAndThen 结果");
                let result = InferTy::option(out.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@OptionAndThen 输入");
                let function = InferTy::function(vec![input.clone()], InferTy::option(out));
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::option(input)))?;
                Ok(result)
            },
            K::ResultAndThen => {
                let (out, err) = self.expected_result_parts(
                    expected,
                    span,
                    "@ResultAndThen 成功",
                    "@ResultAndThen 错误",
                );
                let result = InferTy::result(out.clone(), err.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ResultAndThen 输入");
                let function =
                    InferTy::function(vec![input.clone()], InferTy::result(out, err.clone()));
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::result(input, err)))?;
                Ok(result)
            },
            _ => unreachable!("check_option_map 只接收映射类操作"),
        }
    }

    /// 提取类操作: 第二个实参是默认值, 类型与被提取值相同.
    pub(super) fn check_option_extract(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        match op {
            K::OptionGet => {
                let result = self.expected_or_fresh(expected, span, "@OptionGet 结果");
                self.check_typed_arg(
                    typed_args,
                    args,
                    0,
                    Some(&InferTy::option(result.clone())),
                )?;
                self.check_typed_arg(typed_args, args, 1, Some(&result))?;
                Ok(result)
            },
            K::ResultGet => {
                let result = self.expected_or_fresh(expected, span, "@ResultGet 结果");
                let err = self.infer.fresh(span, "@ResultGet 错误类型");
                self.check_typed_arg(
                    typed_args,
                    args,
                    0,
                    Some(&InferTy::result(result.clone(), err)),
                )?;
                self.check_typed_arg(typed_args, args, 1, Some(&result))?;
                Ok(result)
            },
            _ => unreachable!("check_option_extract 只接收提取类操作"),
        }
    }

    /// 替代类操作: `@ResultOrElse`.
    ///
    /// 成功时原样返回容器, 失败时以错误内容调用替代函数取替代值.
    /// Result 的成功类型固定, 错误类型由替代函数决定.
    fn check_option_or_else(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        match op {
            K::ResultOrElse => {
                let (ok, err) = self.expected_result_parts(
                    expected,
                    span,
                    "@ResultOrElse 成功",
                    "@ResultOrElse 错误",
                );
                let result = InferTy::result(ok.clone(), err.clone());
                self.unify_expected(expected, &result, span)?;
                let input_err = self.infer.fresh(span, "@ResultOrElse 输入错误");
                let function = InferTy::function(
                    vec![input_err.clone()],
                    InferTy::result(ok.clone(), err),
                );
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(
                    typed_args,
                    args,
                    0,
                    Some(&InferTy::result(ok, input_err)),
                )?;
                Ok(result)
            },
            _ => unreachable!("check_option_or_else 只接收替代类操作"),
        }
    }

    /// 外层期望为 Option 时取其元素类型, 否则新建类型变量.
    fn expected_option_elem(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        context: &'static str,
    ) -> InferTy {
        let expected = expected.map(|ty| self.infer.shallow(ty));
        expected
            .as_ref()
            .and_then(InferTy::as_option)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, context))
    }

    /// 外层期望为 Result 时取其成功与错误类型, 否则新建类型变量.
    fn expected_result_parts(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        ok_context: &'static str,
        err_context: &'static str,
    ) -> (InferTy, InferTy) {
        let expected = expected.map(|ty| self.infer.shallow(ty));
        if let Some((ok, err)) = expected.as_ref().and_then(InferTy::as_result) {
            return (ok.clone(), err.clone());
        }
        (
            self.infer.fresh(span, ok_context),
            self.infer.fresh(span, err_context),
        )
    }
}
