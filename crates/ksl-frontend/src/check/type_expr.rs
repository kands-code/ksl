//! 类型表达式与期望检查的辅助.

use ksl_base::types::Type;
use ksl_diagnostics::error::KslError;

use super::Checker;
use super::data::resolve_type;
use crate::ast::TypeExpr;

impl Checker {
    /// 把标注中的类型表达式解析为类型; 与 `@Data` 收集共用名字来源.
    pub(super) fn type_from_expr(&self, ty: &TypeExpr) -> Result<Type, KslError> {
        resolve_type(ty, &self.context.type_env)
    }
}
