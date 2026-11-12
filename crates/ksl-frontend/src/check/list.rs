//! 列表操作的检查规则.

use ksl_base::source::Span;
use ksl_diagnostics::error::KslError;

use super::infer::InferTy;
use super::{Checker, InferExpr, TypedKind};
use crate::ast::Expr;

impl Checker {
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

    /// 列表操作族的类型规则; 结果形状与外层期望统一.
    pub(super) fn check_list_op(
        &mut self,
        op: crate::ast::ListOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        use crate::ast::ListOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        let mut typed_args = vec![None; args.len()];
        if op.is_higher_order() {
            let ty = self.check_list_higher_op(op, args, span, expected, &mut typed_args)?;
            return Ok((
                ty,
                TypedKind::ListOp {
                    op,
                    args: Self::collect_typed_args(typed_args),
                },
            ));
        }
        let ty = match op {
            K::Get
            | K::Lookup
            | K::Update
            | K::Riffle
            | K::Repeat
            | K::Prepend
            | K::Append
            | K::Head
            | K::Tail
            | K::Take
            | K::Drop
            | K::Reverse => {
                self.check_list_edit_op(op, args, span, expected, &mut typed_args)?
            },
            K::Length | K::IsEmpty | K::Flatten | K::Chunk | K::Window | K::Zip => {
                self.check_list_shape_op(op, args, span, expected, &mut typed_args)?
            },
            _ => unreachable!("高阶操作已提前处理"),
        };
        Ok((
            ty,
            TypedKind::ListOp {
                op,
                args: Self::collect_typed_args(typed_args),
            },
        ))
    }

    fn check_list_edit_op(
        &mut self,
        op: crate::ast::ListOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use crate::ast::ListOpKind as K;
        match op {
            K::Get => {
                let result = self.expected_or_fresh(expected, span, "@ListGet 结果");
                let list = InferTy::list(result.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&list))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::U64))?;
                Ok(result)
            },
            K::Lookup => {
                let elem = expected
                    .and_then(InferTy::as_option)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@ListLookup 元素"));
                let result = InferTy::option(elem.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(elem)))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::U64))?;
                self.unify_expected(expected, &result, span)?;
                Ok(result)
            },
            K::Update => {
                let elem = self.list_elem(expected, span, "List 元素");
                let list = InferTy::list(elem.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&list))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::U64))?;
                self.check_typed_arg(typed_args, args, 2, Some(&elem))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Riffle => {
                let elem = self.list_elem(expected, span, "List 元素");
                let list = InferTy::list(elem.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&list))?;
                self.check_typed_arg(typed_args, args, 1, Some(&elem))?;
                self.check_typed_arg(typed_args, args, 2, Some(&InferTy::U64))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Repeat => {
                let elem = self.list_elem(expected, span, "Repeat 元素");
                let list = InferTy::list(elem.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&elem))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::U64))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Prepend | K::Append => {
                let elem = self.list_elem(expected, span, "List 元素");
                let list = InferTy::list(elem.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&list))?;
                self.check_typed_arg(typed_args, args, 1, Some(&elem))?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Head => {
                let result = self.expected_or_fresh(expected, span, "@ListHead 结果");
                let list = InferTy::list(result.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&list))?;
                Ok(result)
            },
            K::Tail | K::Take | K::Drop | K::Reverse => {
                let elem = self.list_elem(expected, span, "List 元素");
                let list = InferTy::list(elem);
                self.check_typed_arg(typed_args, args, 0, Some(&list))?;
                if matches!(op, K::Take | K::Drop) {
                    self.check_typed_arg(typed_args, args, 1, Some(&InferTy::U64))?;
                }
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            _ => unreachable!("非编辑类 List 操作"),
        }
    }

    fn check_list_shape_op(
        &mut self,
        op: crate::ast::ListOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use crate::ast::ListOpKind as K;
        match op {
            K::Length => {
                let elem = self.infer.fresh(span, "List 元素");
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(elem)))?;
                Ok(InferTy::U64)
            },
            K::IsEmpty => {
                let elem = self.infer.fresh(span, "List 元素");
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(elem)))?;
                Ok(InferTy::Bool)
            },
            K::Flatten => {
                let elem = self.list_elem(expected, span, "Flatten 元素");
                let list = InferTy::list(elem.clone());
                self.check_typed_arg(
                    typed_args,
                    args,
                    0,
                    Some(&InferTy::list(InferTy::list(elem))),
                )?;
                self.unify_expected(expected, &list, span)?;
                Ok(list)
            },
            K::Chunk => {
                let elem = self.infer.fresh(span, "Chunk 元素");
                let result = InferTy::list(InferTy::list(elem.clone()));
                self.unify_expected(expected, &result, span)?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(elem)))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::U64))?;
                Ok(result)
            },
            K::Window => {
                let elem = self.infer.fresh(span, "窗口元素");
                let result = InferTy::list(InferTy::list(elem.clone()));
                self.unify_expected(expected, &result, span)?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(elem)))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::U64))?;
                Ok(result)
            },
            K::Zip => {
                let first = self.infer.fresh(span, "Zip 首 List 元素");
                let second = self.infer.fresh(span, "Zip 次 List 元素");
                let result = InferTy::list(InferTy::pair(first.clone(), second.clone()));
                self.unify_expected(expected, &result, span)?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(first)))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::list(second)))?;
                Ok(result)
            },
            _ => unreachable!("非形状类 List 操作"),
        }
    }

    /// 配对访问操作的类型规则.
    pub(super) fn check_pair_op(
        &mut self,
        op: crate::ast::PairOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        use crate::ast::PairOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        let mut typed_args = vec![None; args.len()];
        let result = self.expected_or_fresh(expected, span, "Pair 访问结果");
        let other = self.infer.fresh(span, "Pair 另一分量");
        let pair = if matches!(op, K::Fst) {
            InferTy::pair(result.clone(), other)
        } else {
            InferTy::pair(other, result.clone())
        };
        self.check_typed_arg(&mut typed_args, args, 0, Some(&pair))?;
        Ok((
            result,
            TypedKind::PairOp {
                op,
                args: Self::collect_typed_args(typed_args),
            },
        ))
    }

    /// 带函数参数的列表操作: 谓词, 映射与折叠.
    pub(super) fn check_list_higher_op(
        &mut self,
        op: crate::ast::ListOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use crate::ast::ListOpKind as K;
        match op {
            K::TakeWhile | K::DropWhile | K::Filter => {
                let elem = self.list_elem(expected, span, "List 元素");
                let list = InferTy::list(elem.clone());
                let predicate = InferTy::function(vec![elem], InferTy::Bool);
                self.check_typed_arg(typed_args, args, 1, Some(&predicate))?;
                self.check_typed_arg(typed_args, args, 0, Some(&list))?;
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
                let function = InferTy::function(vec![input.clone()], out);
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(input)))?;
                Ok(result)
            },
            K::FindIndex => {
                let input = self.infer.fresh(span, "@ListFindIndex 输入元素");
                let function = InferTy::function(vec![input.clone()], InferTy::Bool);
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(input)))?;
                let result = InferTy::option(InferTy::U64);
                self.unify_expected(expected, &result, span)?;
                Ok(result)
            },
            K::Fold => {
                let result = self.expected_or_fresh(expected, span, "@ListFold 结果");
                let input = self.infer.fresh(span, "@ListFold 输入元素");
                let function =
                    InferTy::function(vec![input.clone(), result.clone()], result.clone());
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 2, Some(&result))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(input)))?;
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
                let function = InferTy::function(vec![InferTy::U64, input.clone()], out);
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(input)))?;
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
                let function = InferTy::function(vec![input.clone(), out.clone()], out.clone());
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 2, Some(&out))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(input)))?;
                Ok(result)
            },
            K::ZipWith => {
                let out = expected
                    .and_then(InferTy::as_list)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@ZipWith 结果元素"));
                let result = InferTy::list(out.clone());
                self.unify_expected(expected, &result, span)?;
                let first = self.infer.fresh(span, "ZipWith 首 List 元素");
                let second = self.infer.fresh(span, "ZipWith 次 List 元素");
                let function = InferTy::function(vec![first.clone(), second.clone()], out);
                self.check_typed_arg(typed_args, args, 2, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(first)))?;
                self.check_typed_arg(typed_args, args, 1, Some(&InferTy::list(second)))?;
                Ok(result)
            },
            K::Any | K::All => {
                let input = self.infer.fresh(span, "判定输入元素");
                let function = InferTy::function(vec![input.clone()], InferTy::Bool);
                self.check_typed_arg(typed_args, args, 1, Some(&function))?;
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::list(input)))?;
                Ok(InferTy::Bool)
            },
            _ => unreachable!("调用处已限定高阶操作种类"),
        }
    }
}
