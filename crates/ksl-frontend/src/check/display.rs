//! 类型显示与格式参数校验.

use ksl_base::display::TypeFormatter;
use ksl_base::format;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::TypeBindings;

/// 基础值能否出现在 List 与 Pair 中并打印.
#[must_use]
pub fn is_base_printable(ty: &Type) -> bool {
    ty.is_primitive()
}

/// 该类型是否可以打印.
///
/// Pid 可作为顶层值显示; 函数与 Handler 没有文本表示, 由检查器与工具按静态类型展示.
/// List 与 Pair 仅允许一层基础元素.
/// Data, Option, Result, Dict, Set 与不透明类型不可打印.
#[must_use]
pub fn is_printable(ty: &Type) -> bool {
    match ty {
        Type::Pid { .. } => true,
        Type::List(inner) => is_base_printable(inner),
        Type::Pair(first, second) => is_base_printable(first) && is_base_printable(second),
        other => is_base_printable(other),
    }
}

/// 按会话类型名绑定显示类型; 找不到可见名时使用真实类型名.
#[must_use]
pub fn display_type(ty: &Type, bindings: &TypeBindings) -> String {
    let names = bindings.type_names();
    TypeFormatter::new(ty, Some(&names)).to_string()
}

/// 格式化字符串的格式参数校验.
///
/// 数值按类型接受对应格式; string 与 char 接受排版与 `?`.
/// 其他可打印类型不接受参数. 显式冒号但参数为空一律报错.
pub(super) fn check_format_spec(spec: &str, ty: &Type, span: Span) -> Result<(), KslError> {
    if !is_printable(ty) {
        return Err(KslError::new(
            ErrorKind::UnprintableType { found: ty.clone() },
            span,
        ));
    }
    if spec.is_empty() {
        return Err(KslError::new(
            ErrorKind::InvalidFormatSpec {
                spec: spec.to_string(),
            },
            span,
        ));
    }
    let Some(kind) = format_kind(ty) else {
        return Err(KslError::new(
            ErrorKind::InvalidFormatSpec {
                spec: spec.to_string(),
            },
            span,
        ));
    };
    if format::spec_matches(kind, spec) {
        Ok(())
    } else {
        Err(KslError::new(
            ErrorKind::InvalidFormatSpec {
                spec: spec.to_string(),
            },
            span,
        ))
    }
}

/// 类型对应的格式参数类别; None 表示不接受任何格式参数.
pub(super) fn format_kind(ty: &Type) -> Option<format::FormatKind> {
    if ty.is_integer() {
        Some(format::FormatKind::Int)
    } else if ty.is_float() {
        Some(format::FormatKind::Float)
    } else if ty.is_text() {
        Some(format::FormatKind::Text)
    } else {
        None
    }
}
