//! Option 与 Result 便捷操作的检查规则.

use super::Checker;
use super::infer::InferTy;
use crate::ast::Expr;
use crate::error::KslError;
use crate::source::Span;

impl Checker {
    pub(super) fn check_option_op(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        match op {
            K::IsSome | K::IsNone | K::IsOk | K::IsErr => {
                self.check_option_predicate(op, args, span)
            },
            K::OptionMap | K::ResultMap | K::OptionAndThen | K::ResultAndThen => {
                self.check_option_map(op, args, span, expected)
            },
            K::OptionGet | K::ResultGet => self.check_option_extract(op, args, span, expected),
        }
    }

    /// 判定类操作: `@IsSome` / `@IsOk` 等, 结果恒为 bool.
    pub(super) fn check_option_predicate(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        match op {
            K::IsSome | K::IsNone => {
                let elem = self.infer.fresh(span, "Option 元素");
                self.check_expr_expect(&args[0], Some(&InferTy::option(elem)))?;
                Ok(InferTy::Bool)
            },
            K::IsOk | K::IsErr => {
                let ok = self.infer.fresh(span, "Result 成功类型");
                let err = self.infer.fresh(span, "Result 错误类型");
                self.check_expr_expect(&args[0], Some(&InferTy::result(ok, err)))?;
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
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        match op {
            K::OptionMap => {
                let out = expected
                    .and_then(InferTy::as_option)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@OptionMap 结果"));
                let result = InferTy::option(out.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@OptionMap 输入");
                let function = InferTy::Fn(vec![input.clone()], Box::new(out));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::option(input)))?;
                Ok(result)
            },
            K::ResultMap => {
                let (out, err) = if let Some((ok, err)) = expected.and_then(InferTy::as_result)
                {
                    (ok.clone(), err.clone())
                } else {
                    (
                        self.infer.fresh(span, "@ResultMap 成功"),
                        self.infer.fresh(span, "@ResultMap 错误"),
                    )
                };
                let result = InferTy::result(out.clone(), err.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ResultMap 输入");
                let function = InferTy::Fn(vec![input.clone()], Box::new(out));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::result(input, err)))?;
                Ok(result)
            },
            K::OptionAndThen => {
                let out = expected
                    .and_then(InferTy::as_option)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@OptionAndThen 结果"));
                let result = InferTy::option(out.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@OptionAndThen 输入");
                let function = InferTy::Fn(vec![input.clone()], Box::new(InferTy::option(out)));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::option(input)))?;
                Ok(result)
            },
            K::ResultAndThen => {
                let (out, err) = if let Some((ok, err)) = expected.and_then(InferTy::as_result)
                {
                    (ok.clone(), err.clone())
                } else {
                    (
                        self.infer.fresh(span, "@ResultAndThen 成功"),
                        self.infer.fresh(span, "@ResultAndThen 错误"),
                    )
                };
                let result = InferTy::result(out.clone(), err.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ResultAndThen 输入");
                let function = InferTy::Fn(
                    vec![input.clone()],
                    Box::new(InferTy::result(out, err.clone())),
                );
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::result(input, err)))?;
                Ok(result)
            },
            _ => unreachable!("check_option_map 只接收映射类操作"),
        }
    }

    /// 提取类操作: 默认值只在前一分支执行.
    pub(super) fn check_option_extract(
        &mut self,
        op: crate::ast::OptionOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        use crate::ast::OptionOpKind as K;
        match op {
            K::OptionGet => {
                let result = self.expected_or_fresh(expected, span, "@OptionGet 结果");
                self.check_expr_expect(&args[0], Some(&InferTy::option(result.clone())))?;
                self.check_expr_expect(&args[1], Some(&result))?;
                Ok(result)
            },
            K::ResultGet => {
                let result = self.expected_or_fresh(expected, span, "@ResultGet 结果");
                let err = self.infer.fresh(span, "@ResultGet 错误类型");
                self.check_expr_expect(&args[0], Some(&InferTy::result(result.clone(), err)))?;
                self.check_expr_expect(&args[1], Some(&result))?;
                Ok(result)
            },
            _ => unreachable!("check_option_extract 只接收提取类操作"),
        }
    }
}
