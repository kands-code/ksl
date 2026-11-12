//! 类型表达式与期望检查的辅助.

use super::Checker;
use super::data::resolve_type;
use super::infer::InferTy;
use super::types::Type;
use crate::ast::{Expr, TypeExpr};
use crate::error::KslError;

impl Checker {
    pub(super) fn expect_type(&mut self, expr: &Expr, expected: &Type) -> Result<(), KslError> {
        let expected = InferTy::from_concrete(expected);
        self.check_expr_expect(expr, Some(&expected))?;
        Ok(())
    }

    /// 把标注中的类型表达式解析为类型; 与 `@Data` 收集共用名字来源.
    pub(super) fn type_from_expr(&mut self, ty: &TypeExpr) -> Result<Type, KslError> {
        let resolved = resolve_type(ty, &|name| self.context.data.types.contains_key(&name))?;
        self.record_type_expr(ty, &resolved)?;
        Ok(resolved)
    }

    /// 记录类型表达式各节点的解析结果.
    ///
    /// 类型树只解析一次; 这里按已解析类型的结构分发子节点, 避免对子树重复
    /// 解析并重复构造名字集合.
    pub(super) fn record_type_expr(
        &mut self,
        expr: &TypeExpr,
        resolved: &Type,
    ) -> Result<(), KslError> {
        self.infer_types
            .insert(expr.key(), InferTy::from_concrete(resolved));
        match (expr, resolved) {
            (TypeExpr::Named(_), _) => {},
            (TypeExpr::Generic { args, .. }, resolved) => match resolved {
                Type::List(inner) | Type::OptionT(inner) | Type::Set(inner) => {
                    self.record_type_expr(&args[0], inner)?;
                },
                Type::Dict(first, second)
                | Type::Pair(first, second)
                | Type::ResultT(first, second) => {
                    self.record_type_expr(&args[0], first)?;
                    self.record_type_expr(&args[1], second)?;
                },
                Type::PidT(pid) => {
                    for (arg, ty) in args
                        .iter()
                        .zip(pid.mailboxes.iter().chain(std::iter::once(pid.ret.as_ref())))
                    {
                        self.record_type_expr(arg, ty)?;
                    }
                },
                Type::HandlerT(handler) => {
                    let Some((first, rest)) = args.split_first() else {
                        unreachable!("Handler 类型至少有一个参数");
                    };
                    self.record_type_expr(first, &handler.pid)?;
                    for (arg, ty) in rest.iter().zip(&handler.args) {
                        self.record_type_expr(arg, ty)?;
                    }
                },
                _ => unreachable!("类型解析结果与语法形态不一致"),
            },
            (TypeExpr::Fn { params, ret, .. }, Type::Fn(param_types, ret_ty)) => {
                for (param, ty) in params.iter().zip(param_types) {
                    self.record_type_expr(param, ty)?;
                }
                self.record_type_expr(ret, ret_ty)?;
            },
            (TypeExpr::Fn { .. }, _) => unreachable!("函数类型解析结果不是 Fn"),
        }
        Ok(())
    }
}
