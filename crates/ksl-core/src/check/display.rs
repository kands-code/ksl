//! 类型表与类型显示.

use std::fmt;

use super::types::Type;
use crate::ast::NodeKey;
use crate::rewrite::TypeNames;
use crate::symbol::{Symbol, resolve};

/// 节点身份到已解析类型的类型表; 供 LSP 与后续代码生成使用.
///
/// 会话每轮输入都会合并历史类型表, 使用持久化映射让克隆与增量扩展
/// 保持结构共享, 避免长会话中反复复制整张表.
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
/// Pid, 函数与 Handler 可作为顶层值显示; List 与 Pair 仅允许一层基础元素;
/// Data, Option, Result, Dict, Set, `File` 与 `DateTime` 不可打印.
#[must_use]
pub fn is_printable(ty: &Type) -> bool {
    if is_base_printable(ty) || matches!(ty, Type::PidT(_) | Type::Fn(..) | Type::HandlerT(_)) {
        return true;
    }
    match ty {
        Type::List(inner) => is_base_printable(inner),
        Type::Pair(first, second) => is_base_printable(first) && is_base_printable(second),
        _ => false,
    }
}

/// 按会话别名表显示类型; 找不到别名时使用真实类型名.
#[must_use]
pub fn display_type(ty: &Type, names: &TypeNames) -> String {
    TypeFormatter::new(ty, Some(names)).to_string()
}

/// 类型格式化器: `Display` 与带别名表的显示共用同一套分支.
pub(crate) struct TypeFormatter<'a> {
    ty: &'a Type,
    names: Option<&'a TypeNames>,
}

impl<'a> TypeFormatter<'a> {
    pub(crate) fn new(ty: &'a Type, names: Option<&'a TypeNames>) -> Self {
        Self { ty, names }
    }

    fn nested(&self, ty: &'a Type) -> Self {
        Self {
            ty,
            names: self.names,
        }
    }
}

impl fmt::Display for TypeFormatter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ty {
            Type::I64 => f.write_str("i64"),
            Type::U8 => f.write_str("u8"),
            Type::U64 => f.write_str("u64"),
            Type::F64 => f.write_str("f64"),
            Type::Char => f.write_str("char"),
            Type::Bool => f.write_str("bool"),
            Type::Str => f.write_str("string"),
            Type::Nil => f.write_str("nil"),
            Type::Never => f.write_str("never"),
            Type::File => f.write_str("File"),
            Type::DateTime => f.write_str("DateTime"),
            Type::Data(name) => {
                let name = self.names.map_or(*name, |names| nominal_name(*name, names));
                f.write_str(&resolve(name))
            },
            Type::List(inner) => write!(f, "List[{}]", self.nested(inner)),
            Type::Dict(key, value) => {
                write!(f, "Dict[{}, {}]", self.nested(key), self.nested(value))
            },
            Type::Set(inner) => write!(f, "Set[{}]", self.nested(inner)),
            Type::OptionT(inner) => write!(f, "Option[{}]", self.nested(inner)),
            Type::ResultT(ok, err) => {
                write!(f, "Result[{}, {}]", self.nested(ok), self.nested(err))
            },
            Type::Pair(first, second) => {
                write!(f, "Pair[{}, {}]", self.nested(first), self.nested(second))
            },
            Type::PidT(pid) => {
                f.write_str("Pid[")?;
                write_types(f, &pid.mailboxes, self.names)?;
                if !pid.mailboxes.is_empty() {
                    f.write_str(", ")?;
                }
                write!(f, "{}]", self.nested(&pid.ret))
            },
            Type::HandlerT(handler) => {
                f.write_str("Handler[")?;
                write!(f, "{}", self.nested(&handler.pid))?;
                for arg in &handler.args {
                    write!(f, ", {}", self.nested(arg))?;
                }
                f.write_str("]")
            },
            Type::Fn(params, ret) => {
                f.write_str("Fn[[")?;
                write_types(f, params, self.names)?;
                write!(f, "] : {}]", self.nested(ret))
            },
        }
    }
}

fn write_types(
    f: &mut fmt::Formatter<'_>,
    types: &[Type],
    names: Option<&TypeNames>,
) -> fmt::Result {
    for (index, ty) in types.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}", TypeFormatter::new(ty, names))?;
    }
    Ok(())
}

/// 真实类型对应的首选可见名; 多个别名时取声明或导入顺序中的第一个.
fn nominal_name(real: Symbol, names: &TypeNames) -> Symbol {
    names
        .iter()
        .find(|(_, alias)| alias.full == real)
        .map_or(real, |(local, _)| *local)
}
