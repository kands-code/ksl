//! 模式匹配的检查规则.

use std::collections::HashSet;

use super::Checker;
use super::data::check_bindable;
use super::infer::InferTy;
use super::types::Type;
use crate::ast::Expr;
use crate::error::{ErrorKind, KslError};
use crate::symbol::{Symbol, intern, keywords, resolve};

impl Checker {
    pub(super) fn variant_set(&self, ty: &Type) -> Option<Vec<(Symbol, Vec<Type>)>> {
        match ty {
            Type::Data(name) => self.context.data.types.get(name).map(|info| {
                info.variants
                    .iter()
                    .map(|(variant, fields)| (*variant, fields.clone()))
                    .collect()
            }),
            Type::OptionT(inner) => Some(vec![
                (intern("Some"), vec![(*inner.clone())]),
                (intern("None"), vec![]),
            ]),
            Type::ResultT(ok, err) => Some(vec![
                (intern("Ok"), vec![(*ok.clone())]),
                (intern("Err"), vec![(*err.clone())]),
            ]),
            _ => None,
        }
    }

    /// 类型在受限名字中的限定名.
    pub(super) fn type_qualifier(ty: &Type) -> Option<Symbol> {
        let kw = keywords();
        match ty {
            Type::Data(name) => Some(*name),
            Type::OptionT(_) => Some(kw.option_),
            Type::ResultT(..) => Some(kw.result_),
            _ => None,
        }
    }

    /// 解析模式指向的变体: 校验限定名, 变体存在性与字段个数.
    pub(super) fn resolve_pattern(
        &self,
        subject: &Type,
        pattern: &crate::ast::Pattern,
    ) -> Result<(Symbol, Vec<Type>), KslError> {
        let Some(name) = &pattern.variant else {
            // 通配模式: 匹配任意变体, 不绑定字段.
            return Ok((intern(""), Vec::new()));
        };
        let variants = self.variant_set(subject).ok_or_else(|| {
            KslError::new(
                ErrorKind::NotMatchable {
                    found: subject.clone(),
                },
                name.span,
            )
        })?;
        if let Some(qualifier) = &pattern.qualifier {
            let expected = Self::type_qualifier(subject).expect("variant_set 已限定类型");
            if qualifier.node != expected {
                return Err(KslError::with_subject(
                    ErrorKind::TypeMismatch {
                        expected: Type::Data(expected),
                        found: Type::Data(qualifier.node),
                    },
                    qualifier.span,
                    resolve(qualifier.node).to_string(),
                ));
            }
        }
        let Some((variant, fields)) = variants.iter().find(|(known, _)| *known == name.node)
        else {
            return Err(KslError::with_subject(
                ErrorKind::UnknownName,
                name.span,
                resolve(name.node).to_string(),
            ));
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
        Ok((*variant, fields.clone()))
    }

    /// `@Match` 的单个分支: 字段绑定入局部作用域, 分支体按期望检查.
    pub(super) fn check_arm(
        &mut self,
        arm: &crate::ast::MatchArm,
        fields: &[Type],
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        self.env.push();
        let body_ty = (|| {
            let mut seen = HashSet::new();
            for (field, ty) in arm.pattern.fields.iter().zip(fields) {
                let crate::ast::PatternField::Bind(name) = field else {
                    continue;
                };
                check_bindable(name)?;
                if !seen.insert(name.node) {
                    return Err(KslError::with_subject(
                        ErrorKind::DuplicatePatternField {
                            name: resolve(name.node).to_string(),
                        },
                        name.span,
                        resolve(name.node).to_string(),
                    ));
                }
                self.infer_types.insert(name.key(), InferTy::from_concrete(ty));
                self.env.insert(name.node, ty.clone());
            }
            self.check_expr_expect(&arm.body, expected)
        })();
        self.env.pop();
        body_ty
    }

    /// `@Match`: 被匹配值的类型由其自身确定; 各分支体按期望检查.
    pub(super) fn check_match_expect(
        &mut self,
        value: &Expr,
        arms: &[crate::ast::MatchArm],
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        let subject = self.check_expr(value)?;
        let subject = self.infer.finalize(&subject)?;
        let variants = self.variant_set(&subject).ok_or_else(|| {
            KslError::new(
                ErrorKind::NotMatchable {
                    found: subject.clone(),
                },
                value.span,
            )
        })?;
        let result = expected
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(value.span, "@Match 结果"));
        let mut covered: HashSet<Symbol> = HashSet::new();
        let mut wildcard = false;
        let mut all_never = true;
        for arm in arms {
            if wildcard {
                // 通配分支之后不可再有分支: 其后分支不可达.
                return Err(KslError::new(ErrorKind::UnreachableMatchArm, arm.body.span));
            }
            let (variant, fields) = self.resolve_pattern(&subject, &arm.pattern)?;
            if arm.pattern.variant.is_none() {
                wildcard = true;
            } else if !covered.insert(variant) {
                return Err(KslError::new(
                    ErrorKind::DuplicateMatchArm {
                        variant: resolve(variant).to_string(),
                    },
                    arm.pattern
                        .variant
                        .as_ref()
                        .map_or(arm.body.span, |variant| variant.span),
                ));
            }
            let body_ty = self.check_arm(arm, &fields, Some(&result))?;
            self.infer.unify(&result, &body_ty, arm.body.span)?;
            if !matches!(self.infer.shallow(&body_ty), InferTy::Never) {
                all_never = false;
            }
        }
        let missing: Vec<String> = if wildcard {
            Vec::new()
        } else {
            variants
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| !covered.contains(name))
                .map(|name| resolve(name).to_string())
                .collect()
        };
        if !missing.is_empty() {
            return Err(KslError::new(
                ErrorKind::NonExhaustiveMatch { missing },
                value.span,
            ));
        }
        if all_never {
            Ok(InferTy::Never)
        } else {
            Ok(result)
        }
    }
}
