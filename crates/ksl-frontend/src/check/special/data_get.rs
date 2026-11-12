//! `@DataGet` 的检查.

use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::super::infer::InferTy;
use super::super::{Checker, InferExpr};
use crate::ast::Expr;

impl Checker {
    /// 检查 `@DataGet[值, 下标]`; 下标必须是 u8 字面量, 值必须是单变体 Data.
    pub(super) fn check_data_get(
        &mut self,
        value: &Expr,
        index: &Expr,
    ) -> Result<(InferTy, InferExpr, u8), KslError> {
        let typed = self.check_expr(value)?;
        let full = match self.infer.finalize(&typed.ty)? {
            Type::Data(full) => full,
            other => {
                return Err(KslError::new(ErrorKind::NotData { found: other }, value.span));
            },
        };
        let info = self.visible_data(&full, value.span)?;
        if info.variants.len() != 1 {
            return Err(KslError::new(
                ErrorKind::DataGetOnMultiVariant {
                    variants: info.variants.len(),
                },
                value.span,
            ));
        }
        let fields = info.variants.values().next().expect("单变体");
        let field = Self::literal_index(index, "字段下标")?;
        let Some(field_ty) = fields.get(field) else {
            return Err(KslError::new(
                ErrorKind::FieldIndexOutOfRange {
                    index: field,
                    count: fields.len(),
                },
                index.span,
            ));
        };
        let index = u8::try_from(field).expect("字段数不超过 u8 上限");
        Ok((InferTy::from_concrete(field_ty), typed, index))
    }
}
