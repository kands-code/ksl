//! 列表操作的检查规则.

use super::Checker;
use super::infer::InferTy;
use super::types::Type;
use crate::ast::Expr;
use crate::error::KslError;
use crate::source::Span;

impl Checker {
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

    /// 外层期望给出列表元素类型时取之, 否则新建类型变量.
    pub(super) fn list_elem(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        context: &'static str,
    ) -> InferTy {
        expected
            .and_then(InferTy::as_list)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, context))
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

    /// 列表操作族的类型规则; 结果形状与外层期望统一.
    #[allow(clippy::too_many_lines)]
    pub(super) fn check_list_op(
        &mut self,
        op: crate::ast::ListOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        use crate::ast::ListOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        match op {
            K::Get => {
                let result = self.expected_or_fresh(expected, span, "@ListGet 结果");
                let list = InferTy::list(result.clone());
                self.check_expr_expect(&args[0], Some(&list))?;
                self.expect_type(&args[1], &Type::U64)?;
                Ok(result)
            },
            K::Update => {
                let elem = self.list_elem(expected, span, "列表元素");
                let list = InferTy::list(elem.clone());
                self.check_expr_expect(&args[0], Some(&list))?;
                self.expect_type(&args[1], &Type::U64)?;
                self.check_expr_expect(&args[2], Some(&elem))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Riffle => {
                let elem = self.list_elem(expected, span, "列表元素");
                let list = InferTy::list(elem.clone());
                self.check_expr_expect(&args[0], Some(&list))?;
                self.check_expr_expect(&args[1], Some(&elem))?;
                self.expect_type(&args[2], &Type::U64)?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Repeat => {
                let elem = self.list_elem(expected, span, "Repeat 元素");
                let list = InferTy::list(elem.clone());
                self.check_expr_expect(&args[0], Some(&elem))?;
                self.expect_type(&args[1], &Type::U64)?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Prepend | K::Append => {
                let elem = self.list_elem(expected, span, "列表元素");
                let list = InferTy::list(elem.clone());
                self.check_expr_expect(&args[0], Some(&list))?;
                self.check_expr_expect(&args[1], Some(&elem))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Concat => {
                let elem = self.list_elem(expected, span, "列表元素");
                let list = InferTy::list(elem.clone());
                self.check_expr_expect(&args[0], Some(&list))?;
                self.check_expr_expect(&args[1], Some(&list))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Head => {
                let result = self.expected_or_fresh(expected, span, "@ListHead 结果");
                let list = InferTy::list(result.clone());
                self.check_expr_expect(&args[0], Some(&list))?;
                Ok(result)
            },
            K::Tail | K::Take | K::Drop | K::Reverse => {
                let elem = self.list_elem(expected, span, "列表元素");
                let list = InferTy::list(elem);
                self.check_expr_expect(&args[0], Some(&list))?;
                if matches!(op, K::Take | K::Drop) {
                    self.expect_type(&args[1], &Type::U64)?;
                }
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Length => {
                let elem = self.infer.fresh(span, "列表元素");
                self.check_expr_expect(&args[0], Some(&InferTy::list(elem)))?;
                Ok(InferTy::U64)
            },
            K::IsEmpty => {
                let elem = self.infer.fresh(span, "列表元素");
                self.check_expr_expect(&args[0], Some(&InferTy::list(elem)))?;
                Ok(InferTy::Bool)
            },
            K::Flatten => {
                let elem = self.list_elem(expected, span, "Flatten 元素");
                let list = InferTy::list(elem.clone());
                self.check_expr_expect(&args[0], Some(&InferTy::list(InferTy::list(elem))))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Chunk => {
                let elem = self.infer.fresh(span, "Chunk 元素");
                let result = InferTy::list(InferTy::list(elem.clone()));
                self.unify_expected(expected, &result, span)?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(elem)))?;
                self.expect_type(&args[1], &Type::U64)?;
                Ok(result)
            },
            K::Window => {
                let elem = self.infer.fresh(span, "窗口元素");
                let result = InferTy::list(InferTy::list(elem.clone()));
                self.unify_expected(expected, &result, span)?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(elem)))?;
                self.expect_type(&args[1], &Type::U64)?;
                Ok(result)
            },
            K::Zip => {
                let first = self.infer.fresh(span, "Zip 首列表元素");
                let second = self.infer.fresh(span, "Zip 次列表元素");
                let result = InferTy::list(InferTy::pair(first.clone(), second.clone()));
                self.unify_expected(expected, &result, span)?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(first)))?;
                self.check_expr_expect(&args[1], Some(&InferTy::list(second)))?;
                Ok(result)
            },
            K::TakeWhile
            | K::DropWhile
            | K::Map
            | K::MapIndexed
            | K::Filter
            | K::Fold
            | K::PrefixFold
            | K::Any
            | K::All
            | K::ZipWith => self.check_list_higher_op(op, args, span, expected),
        }
    }

    /// 配对访问操作的类型规则.
    pub(super) fn check_pair_op(
        &mut self,
        op: crate::ast::PairOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        use crate::ast::PairOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        let result = self.expected_or_fresh(expected, span, "配对访问结果");
        let other = self.infer.fresh(span, "配对另一分量");
        let pair = if matches!(op, K::Fst) {
            InferTy::pair(result.clone(), other)
        } else {
            InferTy::pair(other, result.clone())
        };
        self.check_expr_expect(&args[0], Some(&pair))?;
        Ok(result)
    }

    /// 带函数参数的列表操作: 谓词, 映射与折叠.
    pub(super) fn check_list_higher_op(
        &mut self,
        op: crate::ast::ListOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        use crate::ast::ListOpKind as K;
        match op {
            K::TakeWhile | K::DropWhile | K::Filter => {
                let elem = self.list_elem(expected, span, "列表元素");
                let list = InferTy::list(elem.clone());
                let predicate = InferTy::Fn(vec![elem], Box::new(InferTy::Bool));
                self.check_expr_expect(&args[1], Some(&predicate))?;
                self.check_expr_expect(&args[0], Some(&list))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Map => {
                let out = expected
                    .and_then(InferTy::as_list)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@ListMap 结果元素"));
                let result = InferTy::list(out.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ListMap 输入元素");
                let function = InferTy::Fn(vec![input.clone()], Box::new(out));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(input)))?;
                Ok(result)
            },
            K::Fold => {
                let result = self.expected_or_fresh(expected, span, "@ListFold 结果");
                let input = self.infer.fresh(span, "@ListFold 输入元素");
                let function =
                    InferTy::Fn(vec![input.clone(), result.clone()], Box::new(result.clone()));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[2], Some(&result))?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(input)))?;
                Ok(result)
            },
            K::MapIndexed => {
                let out = expected
                    .and_then(InferTy::as_list)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@ListMapIndexed 结果元素"));
                let result = InferTy::list(out.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ListMapIndexed 输入元素");
                let function = InferTy::Fn(vec![InferTy::U64, input.clone()], Box::new(out));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(input)))?;
                Ok(result)
            },
            K::PrefixFold => {
                let out = expected
                    .and_then(InferTy::as_list)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@ListPrefixFold 结果元素"));
                let result = InferTy::list(out.clone());
                self.unify_expected(expected, &result, span)?;
                let input = self.infer.fresh(span, "@ListPrefixFold 输入元素");
                let function =
                    InferTy::Fn(vec![input.clone(), out.clone()], Box::new(out.clone()));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[2], Some(&out))?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(input)))?;
                Ok(result)
            },
            K::ZipWith => {
                let out = expected
                    .and_then(InferTy::as_list)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@ZipWith 结果元素"));
                let result = InferTy::list(out.clone());
                self.unify_expected(expected, &result, span)?;
                let first = self.infer.fresh(span, "ZipWith 首列表元素");
                let second = self.infer.fresh(span, "ZipWith 次列表元素");
                let function = InferTy::Fn(vec![first.clone(), second.clone()], Box::new(out));
                self.check_expr_expect(&args[2], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(first)))?;
                self.check_expr_expect(&args[1], Some(&InferTy::list(second)))?;
                Ok(result)
            },
            K::Any | K::All => {
                let input = self.infer.fresh(span, "判定输入元素");
                let function = InferTy::Fn(vec![input.clone()], Box::new(InferTy::Bool));
                self.check_expr_expect(&args[1], Some(&function))?;
                self.check_expr_expect(&args[0], Some(&InferTy::list(input)))?;
                Ok(InferTy::Bool)
            },
            _ => unreachable!("调用处已限定高阶操作种类"),
        }
    }
}
