//! 函数签名与函数体的检查.
//!
//! 匿名 `@Fn`, 具名 `@Def` 与 `@Fix` 组共用签名与函数体检查.

use std::collections::HashSet;

use ksl_base::name::Name;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::super::data::check_bindable;
use super::super::infer::InferTy;
use super::super::{Checker, InferExpr, TypedKind};
use crate::ast::{Def, Expr, TypeExpr};

impl Checker {
    /// 解析函数签名: 参数类型表与返回类型.
    pub(super) fn fn_signature(
        &self,
        params: &[crate::ast::Param],
        ret: &TypeExpr,
    ) -> Result<(Vec<Type>, Type), KslError> {
        let mut param_types = Vec::with_capacity(params.len());
        for param in params {
            param_types.push(self.type_from_expr(&param.ty)?);
        }
        let ret_ty = self.type_from_expr(ret)?;
        Ok((param_types, ret_ty))
    }

    /// 在参数作用域内检查函数体; 普通函数与 Handler 共用.
    pub(super) fn check_fn_body_inner(
        &mut self,
        params: &[crate::ast::Param],
        param_types: &[Type],
        body: &Expr,
        ret_ty: &Type,
    ) -> Result<InferExpr, KslError> {
        self.with_scope(|checker| {
            let mut seen = HashSet::new();
            for (param, ty) in params.iter().zip(param_types) {
                if !seen.insert(param.name.node.clone()) {
                    return Err(KslError::with_name(
                        ErrorKind::DuplicateParam,
                        param.name.span,
                        &param.name.node,
                    ));
                }
                checker.bind_value(&param.name, ty.clone())?;
            }
            let body =
                checker.check_expr_expect(body, Some(&InferTy::from_concrete(ret_ty)))?;
            // 期望统一已在 check_expr_expect 内完成.
            // 仅解析剩余的类型变量; 未定类型的错误位置由变量来源给出.
            checker.infer.finalize(&body.ty)?;
            Ok(body)
        })
    }

    /// 由参数与签名类型构造 typed 绑定者.
    pub(super) fn typed_params(
        params: &[crate::ast::Param],
        types: &[Type],
    ) -> Vec<ksl_typed::Binder<InferTy>> {
        params
            .iter()
            .zip(types)
            .map(|(param, ty)| ksl_typed::Binder {
                name: Some(param.name.node.clone()),
                ty: InferTy::from_concrete(ty),
            })
            .collect()
    }

    /// 普通函数体: 不继承外层 Handler 上下文.
    fn check_fn_body(
        &mut self,
        params: &[crate::ast::Param],
        param_types: &[Type],
        body: &Expr,
        ret_ty: &Type,
    ) -> Result<InferExpr, KslError> {
        self.with_actor(None, |checker| {
            checker.check_fn_body_inner(params, param_types, body, ret_ty)
        })
    }

    /// 匿名 `@Fn`: 类型完全由参数与返回标注决定.
    pub(super) fn check_fn(
        &mut self,
        params: &[crate::ast::Param],
        ret: &TypeExpr,
        body: &Expr,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let (param_types, ret_ty) = self.fn_signature(params, ret)?;
        let body = self.check_fn_body(params, &param_types, body, &ret_ty)?;
        let ty = InferTy::from_concrete(&Type::function(param_types.clone(), ret_ty.clone()));
        Ok((
            ty,
            TypedKind::Fn {
                params: Self::typed_params(params, &param_types),
                ret: InferTy::from_concrete(&ret_ty),
                body: Box::new(body),
            },
        ))
    }

    /// 具名 `@Def`: 先登记签名, 再检查体; 函数体可引用自身.
    pub(super) fn check_def(&mut self, def: &Def) -> Result<(InferTy, TypedKind), KslError> {
        check_bindable(&def.name)?;
        self.check_link_name(&def.name.node, def.name.span)?;
        let (param_types, ret_ty) = self.fn_signature(&def.params, &def.ret)?;
        let fn_ty = Type::function(param_types.clone(), ret_ty.clone());
        self.env.insert(def.name.node.clone(), fn_ty);
        let body = self.check_fn_body(&def.params, &param_types, &def.body, &ret_ty)?;
        Ok((
            InferTy::Nil,
            TypedKind::Def(ksl_typed::Def {
                name: def.name.node.clone(),
                params: Self::typed_params(&def.params, &param_types),
                ret: InferTy::from_concrete(&ret_ty),
                body: Box::new(body),
            }),
        ))
    }

    /// `@Fix`: 先登记组内全部签名, 再逐个检查体; 组内函数可互相引用.
    pub(super) fn check_fix(&mut self, defs: &[Def]) -> Result<(InferTy, TypedKind), KslError> {
        let mut prebound: Vec<(Name, Type, Vec<Type>, Type)> = Vec::new();
        for def in defs {
            check_bindable(&def.name)?;
            self.check_link_name(&def.name.node, def.name.span)?;
            if prebound.iter().any(|(name, ..)| *name == def.name.node) {
                return Err(KslError::with_name(
                    ErrorKind::DuplicateFixName,
                    def.name.span,
                    &def.name.node,
                ));
            }
            let (param_types, ret_ty) = self.fn_signature(&def.params, &def.ret)?;
            let fn_ty = Type::function(param_types.clone(), ret_ty.clone());
            prebound.push((def.name.node.clone(), fn_ty, param_types, ret_ty));
        }
        for (name, fn_ty, ..) in &prebound {
            self.env.insert(name.clone(), fn_ty.clone());
        }
        let mut typed = Vec::with_capacity(defs.len());
        for (def, (_, _fn_ty, param_types, ret_ty)) in defs.iter().zip(&prebound) {
            let body = self.check_fn_body(&def.params, param_types, &def.body, ret_ty)?;
            typed.push(ksl_typed::Def {
                name: def.name.node.clone(),
                params: Self::typed_params(&def.params, param_types),
                ret: InferTy::from_concrete(ret_ty),
                body: Box::new(body),
            });
        }
        Ok((InferTy::Nil, TypedKind::Fix(typed)))
    }
}
