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
                self.check_expr_expect(&args[0], Some(&InferTy::OptionT(Box::new(elem))))?;
                Ok(InferTy::Bool)
            },
            K::IsOk | K::IsErr => {
                let ok = self.infer.fresh(span, "Result 成功类型");
                let err = self.infer.fresh(span, "Result 错误类型");
                self.check_expr_expect(
                    &args[0],
                    Some(&InferTy::ResultT(Box::new(ok), Box::new(err))),
                )?;
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
                let out = match expected {
                    Some(InferTy::OptionT(inner)) => (**inner).clone(),
                    _ => self.infer.fresh(span, "@OptionMap 结果"),
                };
                let result = InferTy::OptionT(Box::new(out.clone()));
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@OptionMap 输入");
                let function = InferTy::Fn(vec![input.clone()], Box::new(out));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::OptionT(Box::new(input))))?;
                Ok(result)
            },
            K::ResultMap => {
                let (out, err) = match expected {
                    Some(InferTy::ResultT(ok, err)) => ((**ok).clone(), (**err).clone()),
                    _ => (
                        self.infer.fresh(span, "@ResultMap 成功"),
                        self.infer.fresh(span, "@ResultMap 错误"),
                    ),
                };
                let result = InferTy::ResultT(Box::new(out.clone()), Box::new(err.clone()));
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ResultMap 输入");
                let function = InferTy::Fn(vec![input.clone()], Box::new(out));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(
                    &args[0],
                    Some(&InferTy::ResultT(Box::new(input), Box::new(err))),
                )?;
                Ok(result)
            },
            K::OptionAndThen => {
                let out = match expected {
                    Some(InferTy::OptionT(inner)) => (**inner).clone(),
                    _ => self.infer.fresh(span, "@OptionAndThen 结果"),
                };
                let result = InferTy::OptionT(Box::new(out.clone()));
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@OptionAndThen 输入");
                let function =
                    InferTy::Fn(vec![input.clone()], Box::new(InferTy::OptionT(Box::new(out))));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::OptionT(Box::new(input))))?;
                Ok(result)
            },
            K::ResultAndThen => {
                let (out, err) = match expected {
                    Some(InferTy::ResultT(ok, err)) => ((**ok).clone(), (**err).clone()),
                    _ => (
                        self.infer.fresh(span, "@ResultAndThen 成功"),
                        self.infer.fresh(span, "@ResultAndThen 错误"),
                    ),
                };
                let result = InferTy::ResultT(Box::new(out.clone()), Box::new(err.clone()));
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ResultAndThen 输入");
                let function = InferTy::Fn(
                    vec![input.clone()],
                    Box::new(InferTy::ResultT(Box::new(out), Box::new(err.clone()))),
                );
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(
                    &args[0],
                    Some(&InferTy::ResultT(Box::new(input), Box::new(err))),
                )?;
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
                self.check_expr_expect(
                    &args[0],
                    Some(&InferTy::OptionT(Box::new(result.clone()))),
                )?;
                self.check_expr_expect(&args[1], Some(&result))?;
                Ok(result)
            },
            K::ResultGet => {
                let result = self.expected_or_fresh(expected, span, "@ResultGet 结果");
                let err = self.infer.fresh(span, "@ResultGet 错误类型");
                self.check_expr_expect(
                    &args[0],
                    Some(&InferTy::ResultT(Box::new(result.clone()), Box::new(err))),
                )?;
                self.check_expr_expect(&args[1], Some(&result))?;
                Ok(result)
            },
            _ => unreachable!("check_option_extract 只接收提取类操作"),
        }
    }
}
