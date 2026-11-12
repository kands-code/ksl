//! 类型表, 类型显示与格式参数校验.

use ksl_base::display::{TypeFormatter, TypeNameResolver};
use ksl_base::format;
use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, keywords};

use super::types::Type;
use crate::ast::NodeKey;
use crate::error::{ErrorKind, KslError};
use crate::rewrite::TypeNames;

/// 节点身份到已解析类型的类型表; 供 LSP 与 IR 降级使用.
///
/// 会话每轮输入都会合并历史类型表.
/// 使用持久化映射让克隆与增量扩展保持结构共享, 避免长会话中反复复制整张表.
pub type TypeTable = imbl::HashMap<NodeKey, Type>;

/// 基础值能否出现在 List 与 Pair 中并打印.
#[must_use]
pub fn is_base_printable(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I64
            | Type::U8
            | Type::U64
            | Type::F64
            | Type::Char
            | Type::Bool
            | Type::Str
            | Type::Nil
    )
}

/// 该类型是否可以打印.
///
/// Pid, 函数与 Handler 可作为顶层值显示.
/// List 与 Pair 仅允许一层基础元素.
/// Data, Option, Result, Dict, Set 与不透明类型不可打印.
#[must_use]
pub fn is_printable(ty: &Type) -> bool {
    if is_base_printable(ty) || matches!(ty, Type::Fn(..)) {
        return true;
    }
    if let Some((name, args)) = ty.as_app() {
        let kw = keywords();
        if name == kw.pid_ || name == kw.handler_ {
            return true;
        }
        if name == kw.list_
            && let [inner] = args
        {
            return is_base_printable(inner);
        }
        if name == kw.pair_
            && let [first, second] = args
        {
            return is_base_printable(first) && is_base_printable(second);
        }
    }
    false
}

/// 按会话别名表显示类型; 找不到别名时使用真实类型名.
#[must_use]
pub fn display_type(ty: &Type, names: &TypeNames) -> String {
    TypeFormatter::new(ty, Some(&Names(names))).to_string()
}

/// 会话别名表适配器.
struct Names<'a>(&'a TypeNames);

impl TypeNameResolver for Names<'_> {
    fn resolve_name(&self, name: Symbol) -> Symbol {
        self.0
            .iter()
            .find(|(_, alias)| alias.full == name)
            .map_or(name, |(local, _)| *local)
    }
}

/// 格式化字符串的格式参数校验.
///
/// `?` 仅限 string 与 char. 数值按类型接受对应格式.
/// 其他可打印类型不接受参数. 显式冒号但参数为空一律报错.
pub(super) fn check_format_spec(spec: &str, ty: &Type, span: Span) -> Result<(), KslError> {
    if !is_printable(ty) {
        return Err(KslError::new(
            ErrorKind::UnprintableType { found: ty.clone() },
            span,
        ));
    }
    if spec == "?" {
        if matches!(ty, Type::Str | Type::Char) {
            return Ok(());
        }
        return Err(KslError::with_subject(
            ErrorKind::TypeMismatch {
                expected: Type::Str,
                found: ty.clone(),
            },
            span,
            spec.to_string(),
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
    match ty {
        Type::I64 | Type::U8 | Type::U64 => Some(format::FormatKind::Int),
        Type::F64 => Some(format::FormatKind::Float),
        Type::Str | Type::Char => Some(format::FormatKind::Text),
        _ => None,
    }
}
