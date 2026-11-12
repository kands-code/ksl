//! Dict 与 Set 的检查规则.
//!
//! 键类型必须落在运行时 `Key` 的支持范围内; 空字面量需要外部期望给出类型.

use ksl_base::source::Span;
use ksl_diagnostics::error::KslError;

use super::data::ensure_dict_key;
use super::infer::InferTy;
use super::{Checker, InferExpr, TypedKind};
use crate::ast::{DictOpKind, Expr, SetOpKind};

impl Checker {
    /// `@Dict[(k, v), ...]` 字面量.
    pub(super) fn check_dict_literal(
        &mut self,
        entries: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let (key, value) = self.expected_dict_parts(expected, span, "@Dict 键", "@Dict 值");
        let pair = InferTy::pair(key.clone(), value.clone());
        let mut typed = Vec::with_capacity(entries.len());
        for entry in entries {
            typed.push(self.check_expr_expect(entry, Some(&pair))?);
        }
        self.ensure_dict_key(&key, span)?;
        Ok((InferTy::dict(key, value), TypedKind::DictLiteral(typed)))
    }

    /// `@Set[value, ...]` 字面量.
    pub(super) fn check_set_literal(
        &mut self,
        items: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let elem = self.expected_set_elem(expected, span, "@Set 元素");
        let mut typed = Vec::with_capacity(items.len());
        for item in items {
            typed.push(self.check_expr_expect(item, Some(&elem))?);
        }
        self.ensure_dict_key(&elem, span)?;
        Ok((InferTy::set(elem), TypedKind::SetLiteral(typed)))
    }

    /// Dict 操作的类型规则.
    pub(super) fn check_dict_op(
        &mut self,
        op: DictOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        use DictOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        let mut typed_args = vec![None; args.len()];
        let ty = match op {
            K::Lookup | K::Get | K::Update | K::Put | K::Remove | K::Has => {
                self.check_dict_access(op, args, span, expected, &mut typed_args)?
            },
            K::Length
            | K::IsEmpty
            | K::Entries
            | K::Keys
            | K::Values
            | K::Filter
            | K::Merge
            | K::FromEntries => {
                self.check_dict_query(op, args, span, expected, &mut typed_args)?
            },
        };
        Ok((
            ty,
            TypedKind::DictOp {
                op,
                args: Self::collect_typed_args(typed_args),
            },
        ))
    }

    fn check_dict_access(
        &mut self,
        op: DictOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use DictOpKind as K;
        match op {
            K::Lookup => {
                let value = expected
                    .and_then(InferTy::as_option)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@DictLookup 结果"));
                let key = self.infer.fresh(span, "@DictLookup 键");
                let dict = InferTy::dict(key.clone(), value.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&key))?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::option(value))
            },
            K::Get => {
                let value = self.expected_or_fresh(expected, span, "@DictGet 结果");
                let key = self.infer.fresh(span, "@DictGet 键");
                let dict = InferTy::dict(key.clone(), value.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&key))?;
                self.check_typed_arg(typed_args, args, 2, Some(&value))?;
                self.ensure_dict_key(&key, span)?;
                Ok(value)
            },
            K::Update => {
                let (key, value) = self.expected_dict_parts(
                    expected,
                    span,
                    "@DictUpdate 键",
                    "@DictUpdate 值",
                );
                let dict = InferTy::dict(key.clone(), value.clone());
                let function = InferTy::function(vec![value.clone()], value.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&key))?;
                self.check_typed_arg(typed_args, args, 2, Some(&function))?;
                self.check_typed_arg(typed_args, args, 3, Some(&value))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Put => {
                let (key, value) =
                    self.expected_dict_parts(expected, span, "@DictPut 键", "@DictPut 值");
                let dict = InferTy::dict(key.clone(), value.clone());
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&key))?;
                self.check_typed_arg(typed_args, args, 2, Some(&value))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Remove => {
                let key = self.infer.fresh(span, "@DictRemove 键");
                let value = self.infer.fresh(span, "@DictRemove 值");
                let dict = InferTy::dict(key.clone(), value);
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&key))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Has => {
                let key = self.infer.fresh(span, "@DictHas 键");
                let value = self.infer.fresh(span, "@DictHas 值");
                let dict = InferTy::dict(key.clone(), value);
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&key))?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::Bool)
            },
            _ => unreachable!("非访问类 Dict 操作"),
        }
    }

    fn check_dict_query(
        &mut self,
        op: DictOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
        typed_args: &mut [Option<InferExpr>],
    ) -> Result<InferTy, KslError> {
        use DictOpKind as K;
        match op {
            K::Length => {
                let key = self.infer.fresh(span, "@DictLength 键");
                let value = self.infer.fresh(span, "@DictLength 值");
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::dict(key, value)))?;
                Ok(InferTy::U64)
            },
            K::IsEmpty => {
                let key = self.infer.fresh(span, "@IsDictEmpty 键");
                let value = self.infer.fresh(span, "@IsDictEmpty 值");
                self.check_typed_arg(typed_args, args, 0, Some(&InferTy::dict(key, value)))?;
                Ok(InferTy::Bool)
            },
            K::Entries => {
                let (key, value) = self.expected_entry_parts(
                    expected,
                    span,
                    "@DictEntries 键",
                    "@DictEntries 值",
                );
                self.check_typed_arg(
                    typed_args,
                    args,
                    0,
                    Some(&InferTy::dict(key.clone(), value.clone())),
                )?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::list(InferTy::pair(key, value)))
            },
            K::Keys => {
                let key = self.expected_list_elem(expected, span, "@DictKeys 键");
                let value = self.infer.fresh(span, "@DictKeys 值");
                self.check_typed_arg(
                    typed_args,
                    args,
                    0,
                    Some(&InferTy::dict(key.clone(), value)),
                )?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::list(key))
            },
            K::Values => {
                let key = self.infer.fresh(span, "@DictValues 键");
                let value = self.expected_list_elem(expected, span, "@DictValues 值");
                self.check_typed_arg(
                    typed_args,
                    args,
                    0,
                    Some(&InferTy::dict(key, value.clone())),
                )?;
                Ok(InferTy::list(value))
            },
            K::Filter => {
                let key = self.infer.fresh(span, "@DictFilter 键");
                let value = self.infer.fresh(span, "@DictFilter 值");
                let dict = InferTy::dict(key.clone(), value.clone());
                let predicate = InferTy::function(vec![key.clone(), value], InferTy::Bool);
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&predicate))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Merge => {
                let (key, value) =
                    self.expected_dict_parts(expected, span, "@DictMerge 键", "@DictMerge 值");
                let dict = InferTy::dict(key.clone(), value);
                self.check_typed_arg(typed_args, args, 0, Some(&dict))?;
                self.check_typed_arg(typed_args, args, 1, Some(&dict))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::FromEntries => {
                let (key, value) = self.expected_dict_parts(
                    expected,
                    span,
                    "@DictFromEntries 键",
                    "@DictFromEntries 值",
                );
                let entries = InferTy::list(InferTy::pair(key.clone(), value.clone()));
                self.check_typed_arg(typed_args, args, 0, Some(&entries))?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::dict(key, value))
            },
            _ => unreachable!("非查询类 Dict 操作"),
        }
    }

    /// Set 操作的类型规则.
    pub(super) fn check_set_op(
        &mut self,
        op: SetOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        use SetOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        let mut typed_args = vec![None; args.len()];
        let ty = (match op {
            K::Insert => {
                let elem = self.expected_set_elem(expected, span, "@SetInsert 元素");
                let set = InferTy::set(elem.clone());
                self.check_typed_arg(&mut typed_args, args, 0, Some(&set))?;
                self.check_typed_arg(&mut typed_args, args, 1, Some(&elem))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::Remove => {
                let elem = self.infer.fresh(span, "@SetRemove 元素");
                let set = InferTy::set(elem.clone());
                self.check_typed_arg(&mut typed_args, args, 0, Some(&set))?;
                self.check_typed_arg(&mut typed_args, args, 1, Some(&elem))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::Has => {
                let elem = self.infer.fresh(span, "@SetHas 元素");
                let set = InferTy::set(elem.clone());
                self.check_typed_arg(&mut typed_args, args, 0, Some(&set))?;
                self.check_typed_arg(&mut typed_args, args, 1, Some(&elem))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(InferTy::Bool)
            },
            K::Length => {
                let elem = self.infer.fresh(span, "@SetLength 元素");
                self.check_typed_arg(&mut typed_args, args, 0, Some(&InferTy::set(elem)))?;
                Ok(InferTy::U64)
            },
            K::IsEmpty => {
                let elem = self.infer.fresh(span, "@IsSetEmpty 元素");
                self.check_typed_arg(&mut typed_args, args, 0, Some(&InferTy::set(elem)))?;
                Ok(InferTy::Bool)
            },
            K::Items => {
                let elem = self.expected_list_elem(expected, span, "@SetItems 元素");
                self.check_typed_arg(
                    &mut typed_args,
                    args,
                    0,
                    Some(&InferTy::set(elem.clone())),
                )?;
                self.ensure_dict_key(&elem, span)?;
                Ok(InferTy::list(elem))
            },
            K::Filter => {
                let elem = self.infer.fresh(span, "@SetFilter 元素");
                let set = InferTy::set(elem.clone());
                let predicate = InferTy::function(vec![elem.clone()], InferTy::Bool);
                self.check_typed_arg(&mut typed_args, args, 0, Some(&set))?;
                self.check_typed_arg(&mut typed_args, args, 1, Some(&predicate))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::Union | K::Intersect | K::Difference => {
                let elem = self.expected_set_elem(expected, span, "Set 运算元素");
                let set = InferTy::set(elem.clone());
                self.check_typed_arg(&mut typed_args, args, 0, Some(&set))?;
                self.check_typed_arg(&mut typed_args, args, 1, Some(&set))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::FromList => {
                let elem = self.expected_set_elem(expected, span, "@SetFromList 元素");
                self.check_typed_arg(
                    &mut typed_args,
                    args,
                    0,
                    Some(&InferTy::list(elem.clone())),
                )?;
                self.ensure_dict_key(&elem, span)?;
                Ok(InferTy::set(elem))
            },
        })?;
        Ok((
            ty,
            TypedKind::SetOp {
                op,
                args: Self::collect_typed_args(typed_args),
            },
        ))
    }

    /// 外层期望为 Dict 时取其键与值, 否则新建类型变量.
    fn expected_dict_parts(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        key_context: &'static str,
        value_context: &'static str,
    ) -> (InferTy, InferTy) {
        let expected = expected.map(|ty| self.infer.shallow(ty));
        if let Some((key, value)) = expected.as_ref().and_then(InferTy::as_dict) {
            return (key.clone(), value.clone());
        }
        (
            self.infer.fresh(span, key_context),
            self.infer.fresh(span, value_context),
        )
    }

    /// 外层期望为 Set 时取其元素类型, 否则新建类型变量.
    fn expected_set_elem(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        context: &'static str,
    ) -> InferTy {
        let expected = expected.map(|ty| self.infer.shallow(ty));
        expected
            .as_ref()
            .and_then(InferTy::as_set)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, context))
    }

    /// 外层期望为 `List[T]` 时取元素类型, 否则新建类型变量.
    fn expected_list_elem(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        context: &'static str,
    ) -> InferTy {
        let expected = expected.map(|ty| self.infer.shallow(ty));
        expected
            .as_ref()
            .and_then(InferTy::as_list)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, context))
    }

    /// 外层期望为 `List[Pair[A, B]]` 时取两个分量, 否则新建类型变量.
    fn expected_entry_parts(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
        key_context: &'static str,
        value_context: &'static str,
    ) -> (InferTy, InferTy) {
        let pair = expected
            .map(|ty| self.infer.shallow(ty))
            .as_ref()
            .and_then(InferTy::as_list)
            .cloned();
        let pair = pair.map(|ty| self.infer.shallow(&ty));
        if let Some((key, value)) = pair.as_ref().and_then(InferTy::as_pair) {
            return (key.clone(), value.clone());
        }
        (
            self.infer.fresh(span, key_context),
            self.infer.fresh(span, value_context),
        )
    }

    /// 校验键类型; 仍未确定时留到本轮检查结束再校验.
    fn ensure_dict_key(&mut self, key: &InferTy, span: Span) -> Result<(), KslError> {
        let key = self.infer.shallow(key);
        if matches!(&key, InferTy::Var(_)) {
            self.pending_keys.push((key, span));
            return Ok(());
        }
        let key = self.infer.finalize(&key)?;
        ensure_dict_key(&key, span)
    }

    /// 校验推迟到检查结束的键类型; 此时兄弟表达式与期望已完成统一.
    pub(super) fn validate_pending_keys(&mut self) -> Result<(), KslError> {
        for (key, span) in std::mem::take(&mut self.pending_keys) {
            let key = self.infer.finalize(&key)?;
            ensure_dict_key(&key, span)?;
        }
        Ok(())
    }
}
