//! 模式匹配的检查规则.

use std::collections::HashSet;

use ksl_base::name::{Name, keywords};
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::infer::InferTy;
use super::{Checker, TypeTarget};
use crate::ast::Expr;

/// 可匹配类型的限定名与变体集合.
pub(super) struct Matchable {
    pub(super) qualifier: Name,
    pub(super) variants: Vec<(Name, Vec<Type>)>,
}

impl Checker {
    /// 取可匹配类型的限定名与变体; 其他类型返回 None.
    ///
    /// 具名 Data 必须可见; 不可见时报用户错误.
    ///
    /// # Errors
    ///
    /// 具名 Data 不在当前环境中可见, 或其声明缺失.
    pub(super) fn matchable(
        &self,
        ty: &Type,
        span: Span,
    ) -> Result<Option<Matchable>, KslError> {
        if let Some(name) = ty.as_data() {
            let info = self.visible_data(name, span)?;
            return Ok(Some(Matchable {
                qualifier: name.clone(),
                variants: info
                    .variants
                    .iter()
                    .map(|(variant, fields)| (variant.clone(), fields.clone()))
                    .collect(),
            }));
        }
        if let Some(inner) = ty.as_option() {
            return Ok(Some(Matchable {
                qualifier: keywords().option_.clone(),
                variants: vec![
                    (keywords().some_.clone(), vec![inner.clone()]),
                    (keywords().none_.clone(), Vec::new()),
                ],
            }));
        }
        if let Some((ok, err)) = ty.as_result() {
            return Ok(Some(Matchable {
                qualifier: keywords().result_.clone(),
                variants: vec![
                    (keywords().ok_.clone(), vec![ok.clone()]),
                    (keywords().err_.clone(), vec![err.clone()]),
                ],
            }));
        }
        Ok(None)
    }

    /// 解析模式指向的变体: 校验限定名, 变体存在性与字段个数.
    ///
    /// 通配模式不指向具体变体, 返回 None.
    pub(super) fn resolve_pattern(
        &self,
        pattern: &crate::ast::Pattern,
        matchable: &Matchable,
    ) -> Result<(Option<Name>, Vec<Type>), KslError> {
        let Some(name) = &pattern.variant else {
            // 通配模式: 匹配任意变体, 不绑定字段.
            return Ok((None, Vec::new()));
        };
        if let Some(qualifier) = &pattern.qualifier {
            let expected = &matchable.qualifier;
            let kw = keywords();
            // 内置 Option 与 Result 以关键字作限定名, 不进入类型名绑定.
            let found = self.context.type_env.resolve(&qualifier.node).or_else(|| {
                (qualifier.node == kw.option_ || qualifier.node == kw.result_)
                    .then(|| TypeTarget::Data(qualifier.node.clone()))
            });
            let Some(found) = found else {
                return Err(KslError::unknown_type(&qualifier.node, qualifier.span));
            };
            if &found.full() != expected {
                return Err(KslError::with_name(
                    ErrorKind::TypeMismatch {
                        expected: Type::data(expected.clone()),
                        found: found.to_type(),
                    },
                    qualifier.span,
                    &qualifier.node,
                ));
            }
        }
        let Some((variant, fields)) =
            matchable.variants.iter().find(|(known, _)| *known == name.node)
        else {
            return Err(KslError::unknown_name(&name.node, name.span));
        };
        if pattern.fields.len() != fields.len() {
            return Err(KslError::new(
                ErrorKind::ArityMismatch {
                    expected: fields.len(),
                    found: pattern.fields.len(),
                },
                name.span,
            ));
        }
        Ok((Some(variant.clone()), fields.clone()))
    }

    /// `@Match` 的单个分支: 字段绑定入局部作用域, 分支体按期望检查.
    pub(super) fn check_arm(
        &mut self,
        arm: &crate::ast::MatchArm,
        fields: &[Type],
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, crate::check::InferExpr), KslError> {
        self.with_scope(|checker| {
            let mut seen = HashSet::new();
            for (field, ty) in arm.pattern.fields.iter().zip(fields) {
                let crate::ast::PatternField::Bind(name) = field else {
                    continue;
                };
                if !seen.insert(name.node.clone()) {
                    return Err(KslError::duplicate_pattern_field(&name.node, name.span));
                }
                checker.bind_value(name, ty.clone())?;
            }
            let body = checker.check_expr_expect(&arm.body, expected)?;
            let ty = body.ty.clone();
            Ok((ty, body))
        })
    }

    pub(super) fn check_match_typed(
        &mut self,
        value: &Expr,
        arms: &[crate::ast::MatchArm],
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, crate::check::TypedKind), KslError> {
        let value = self.check_expr(value)?;
        let subject = self.infer.finalize(&value.ty)?;
        let Some(matchable) = self.matchable(&subject, value.span)? else {
            return Err(KslError::new(
                ErrorKind::NotMatchable { found: subject },
                value.span,
            ));
        };
        let result = expected
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(value.span, "@Match 结果"));
        let mut covered: HashSet<Name> = HashSet::new();
        let mut wildcard = false;
        let mut all_never = true;
        let mut typed_arms = Vec::with_capacity(arms.len());
        for arm in arms {
            if wildcard {
                return Err(KslError::new(ErrorKind::UnreachableMatchArm, arm.body.span));
            }
            let (variant, fields) = self.resolve_pattern(&arm.pattern, &matchable)?;
            if let Some(variant) = variant {
                if !covered.insert(variant.clone()) {
                    return Err(KslError::duplicate_match_arm(
                        &variant,
                        arm.pattern
                            .variant
                            .as_ref()
                            .map_or(arm.body.span, |variant| variant.span),
                    ));
                }
            } else {
                wildcard = true;
            }
            let (body_ty, body) = self.check_arm(arm, &fields, Some(&result))?;
            self.infer.unify(&result, &body_ty, arm.body.span)?;
            if !matches!(self.infer.shallow(&body_ty), InferTy::Never) {
                all_never = false;
            }
            let pattern = Self::typed_pattern(&arm.pattern, &matchable, &fields);
            typed_arms.push(ksl_typed::MatchArm {
                pattern,
                body: Box::new(body),
            });
        }
        let missing: Vec<String> = if wildcard {
            Vec::new()
        } else {
            matchable
                .variants
                .iter()
                .map(|(name, _)| name.clone())
                .filter(|name| !covered.contains(name))
                .map(|name| name.as_str().to_string())
                .collect()
        };
        if !missing.is_empty() {
            return Err(KslError::new(
                ErrorKind::NonExhaustiveMatch { missing },
                value.span,
            ));
        }
        let ty = if all_never { InferTy::Never } else { result };
        Ok((
            ty,
            crate::check::TypedKind::Match {
                value: Box::new(value),
                arms: typed_arms,
            },
        ))
    }

    fn typed_pattern(
        pattern: &crate::ast::Pattern,
        matchable: &Matchable,
        fields: &[Type],
    ) -> ksl_typed::Pattern<InferTy> {
        let Some(name) = &pattern.variant else {
            return ksl_typed::Pattern::Wildcard;
        };
        let qualifier = pattern.qualifier.as_ref().map(|_| matchable.qualifier.clone());
        let fields = pattern
            .fields
            .iter()
            .zip(fields)
            .map(|(field, ty)| match field {
                crate::ast::PatternField::Bind(name) => Some(ksl_typed::Binder {
                    name: Some(name.node.clone()),
                    ty: InferTy::from_concrete(ty),
                }),
                crate::ast::PatternField::Wildcard => None,
            })
            .collect();
        ksl_typed::Pattern::Variant {
            qualifier,
            name: name.node.clone(),
            fields,
        }
    }
}
