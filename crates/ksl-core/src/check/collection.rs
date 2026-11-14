//! Dict 与 Set 的检查规则.
//!
//! 键类型必须落在运行时 `Key` 的支持范围内; 空字面量需要外部期望给出类型.

use super::Checker;
use super::data::ensure_dict_key;
use super::infer::InferTy;
use crate::ast::{DictOpKind, Expr, SetOpKind};
use crate::error::KslError;
use crate::source::Span;

impl Checker {
    /// `@Dict[(k, v), ...]` 字面量.
    pub(super) fn check_dict_literal(
        &mut self,
        entries: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        let expected = expected.map(|ty| self.infer.shallow(ty));
        let (key, value) =
            if let Some((key, value)) = expected.as_ref().and_then(InferTy::as_dict) {
                (key.clone(), value.clone())
            } else {
                (
                    self.infer.fresh(span, "@Dict 键"),
                    self.infer.fresh(span, "@Dict 值"),
                )
            };
        let pair = InferTy::pair(key.clone(), value.clone());
        for entry in entries {
            self.check_expr_expect(entry, Some(&pair))?;
        }
        self.ensure_dict_key(&key, span)?;
        Ok(InferTy::dict(key, value))
    }

    /// `@Set[value, ...]` 字面量.
    pub(super) fn check_set_literal(
        &mut self,
        items: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        let expected = expected.map(|ty| self.infer.shallow(ty));
        let elem = expected
            .as_ref()
            .and_then(InferTy::as_set)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, "@Set 元素"));
        for item in items {
            self.check_expr_expect(item, Some(&elem))?;
        }
        self.ensure_dict_key(&elem, span)?;
        Ok(InferTy::set(elem))
    }

    /// Dict 操作的类型规则.
    #[allow(clippy::too_many_lines)]
    pub(super) fn check_dict_op(
        &mut self,
        op: DictOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        use DictOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        match op {
            K::Lookup => {
                let value = expected
                    .and_then(InferTy::as_option)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@DictLookup 结果"));
                let key = self.infer.fresh(span, "@DictLookup 键");
                let dict = InferTy::dict(key.clone(), value.clone());
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&key))?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::option(value))
            },
            K::Get => {
                let value = self.expected_or_fresh(expected, span, "@DictGet 结果");
                let key = self.infer.fresh(span, "@DictGet 键");
                let dict = InferTy::dict(key.clone(), value.clone());
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&key))?;
                self.check_expr_expect(&args[2], Some(&value))?;
                self.ensure_dict_key(&key, span)?;
                Ok(value)
            },
            K::Update => {
                let (key, value) =
                    if let Some((key, value)) = expected.and_then(InferTy::as_dict) {
                        (key.clone(), value.clone())
                    } else {
                        (
                            self.infer.fresh(span, "@DictUpdate 键"),
                            self.infer.fresh(span, "@DictUpdate 值"),
                        )
                    };
                let dict = InferTy::dict(key.clone(), value.clone());
                let function = InferTy::Fn(vec![value.clone()], Box::new(value.clone()));
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&key))?;
                self.check_expr_expect(&args[2], Some(&function))?;
                self.check_expr_expect(&args[3], Some(&value))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Put => {
                let (key, value) =
                    if let Some((key, value)) = expected.and_then(InferTy::as_dict) {
                        (key.clone(), value.clone())
                    } else {
                        (
                            self.infer.fresh(span, "@DictPut 键"),
                            self.infer.fresh(span, "@DictPut 值"),
                        )
                    };
                let dict = InferTy::dict(key.clone(), value.clone());
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&key))?;
                self.check_expr_expect(&args[2], Some(&value))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Remove => {
                let key = self.infer.fresh(span, "@DictRemove 键");
                let value = self.infer.fresh(span, "@DictRemove 值");
                let dict = InferTy::dict(key.clone(), value);
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&key))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Has => {
                let key = self.infer.fresh(span, "@DictHas 键");
                let value = self.infer.fresh(span, "@DictHas 值");
                let dict = InferTy::dict(key.clone(), value);
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&key))?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::Bool)
            },
            K::Length => {
                let key = self.infer.fresh(span, "@DictLength 键");
                let value = self.infer.fresh(span, "@DictLength 值");
                self.check_expr_expect(&args[0], Some(&InferTy::dict(key, value)))?;
                Ok(InferTy::U64)
            },
            K::IsEmpty => {
                let key = self.infer.fresh(span, "@IsDictEmpty 键");
                let value = self.infer.fresh(span, "@IsDictEmpty 值");
                self.check_expr_expect(&args[0], Some(&InferTy::dict(key, value)))?;
                Ok(InferTy::Bool)
            },
            K::Entries => {
                let key = self.infer.fresh(span, "@DictEntries 键");
                let value = self.infer.fresh(span, "@DictEntries 值");
                self.check_expr_expect(
                    &args[0],
                    Some(&InferTy::dict(key.clone(), value.clone())),
                )?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::list(InferTy::pair(key, value)))
            },
            K::Keys => {
                let key = self.infer.fresh(span, "@DictKeys 键");
                let value = self.infer.fresh(span, "@DictKeys 值");
                self.check_expr_expect(&args[0], Some(&InferTy::dict(key.clone(), value)))?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::list(key))
            },
            K::Values => {
                let key = self.infer.fresh(span, "@DictValues 键");
                let value = self.infer.fresh(span, "@DictValues 值");
                self.check_expr_expect(&args[0], Some(&InferTy::dict(key, value.clone())))?;
                Ok(InferTy::list(value))
            },
            K::Filter => {
                let key = self.infer.fresh(span, "@DictFilter 键");
                let value = self.infer.fresh(span, "@DictFilter 值");
                let dict = InferTy::dict(key.clone(), value.clone());
                let predicate =
                    InferTy::Fn(vec![key.clone(), value.clone()], Box::new(InferTy::Bool));
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&predicate))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::Merge => {
                let (key, value) =
                    if let Some((key, value)) = expected.and_then(InferTy::as_dict) {
                        (key.clone(), value.clone())
                    } else {
                        (
                            self.infer.fresh(span, "@DictMerge 键"),
                            self.infer.fresh(span, "@DictMerge 值"),
                        )
                    };
                let dict = InferTy::dict(key.clone(), value.clone());
                self.check_expr_expect(&args[0], Some(&dict))?;
                self.check_expr_expect(&args[1], Some(&dict))?;
                self.ensure_dict_key(&key, span)?;
                Ok(dict)
            },
            K::FromEntries => {
                let key = self.infer.fresh(span, "@DictFromEntries 键");
                let value = self.infer.fresh(span, "@DictFromEntries 值");
                let entries = InferTy::list(InferTy::pair(key.clone(), value.clone()));
                self.check_expr_expect(&args[0], Some(&entries))?;
                self.ensure_dict_key(&key, span)?;
                Ok(InferTy::dict(key, value))
            },
        }
    }

    /// Set 操作的类型规则.
    #[allow(clippy::too_many_lines)]
    pub(super) fn check_set_op(
        &mut self,
        op: SetOpKind,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        use SetOpKind as K;
        Self::check_arity(args, op.arity(), span)?;
        match op {
            K::Insert => {
                let elem = expected
                    .and_then(InferTy::as_set)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@SetInsert 元素"));
                let set = InferTy::set(elem.clone());
                self.check_expr_expect(&args[0], Some(&set))?;
                self.check_expr_expect(&args[1], Some(&elem))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::Remove => {
                let elem = self.infer.fresh(span, "@SetRemove 元素");
                let set = InferTy::set(elem.clone());
                self.check_expr_expect(&args[0], Some(&set))?;
                self.check_expr_expect(&args[1], Some(&elem))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::Has => {
                let elem = self.infer.fresh(span, "@SetHas 元素");
                let set = InferTy::set(elem.clone());
                self.check_expr_expect(&args[0], Some(&set))?;
                self.check_expr_expect(&args[1], Some(&elem))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(InferTy::Bool)
            },
            K::Length => {
                let elem = self.infer.fresh(span, "@SetLength 元素");
                self.check_expr_expect(&args[0], Some(&InferTy::set(elem)))?;
                Ok(InferTy::U64)
            },
            K::IsEmpty => {
                let elem = self.infer.fresh(span, "@IsSetEmpty 元素");
                self.check_expr_expect(&args[0], Some(&InferTy::set(elem)))?;
                Ok(InferTy::Bool)
            },
            K::Items => {
                let elem = self.infer.fresh(span, "@SetItems 元素");
                self.check_expr_expect(&args[0], Some(&InferTy::set(elem.clone())))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(InferTy::list(elem))
            },
            K::Filter => {
                let elem = self.infer.fresh(span, "@SetFilter 元素");
                let set = InferTy::set(elem.clone());
                let predicate = InferTy::Fn(vec![elem.clone()], Box::new(InferTy::Bool));
                self.check_expr_expect(&args[0], Some(&set))?;
                self.check_expr_expect(&args[1], Some(&predicate))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::Union | K::Intersect | K::Difference => {
                let elem = expected
                    .and_then(InferTy::as_set)
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "Set 运算元素"));
                let set = InferTy::set(elem.clone());
                self.check_expr_expect(&args[0], Some(&set))?;
                self.check_expr_expect(&args[1], Some(&set))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(set)
            },
            K::FromList => {
                let elem = self.infer.fresh(span, "@SetFromList 元素");
                self.check_expr_expect(&args[0], Some(&InferTy::list(elem.clone())))?;
                self.ensure_dict_key(&elem, span)?;
                Ok(InferTy::set(elem))
            },
        }
    }

    fn ensure_dict_key(&mut self, key: &InferTy, span: Span) -> Result<(), KslError> {
        let key = self.infer.finalize(key)?;
        ensure_dict_key(&key, span)
    }
}
