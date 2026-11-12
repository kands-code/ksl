//! 类型显示.
//!
//! 默认按真实类型名显示; 会话可通过别名解析器替换标称类型名.

use std::fmt;

use crate::symbol::{Symbol, resolve};
use crate::types::Type;

/// 标称类型名到显示名的解析.
pub trait TypeNameResolver {
    /// 返回该真实类型对应的显示名. 仅对无参数的类型应用调用.
    fn resolve_name(&self, name: Symbol) -> Symbol;
}

/// 类型格式化器: `Display` 与带别名表的显示共用同一套分支.
pub struct TypeFormatter<'a> {
    ty: &'a Type,
    names: Option<&'a dyn TypeNameResolver>,
}

impl<'a> TypeFormatter<'a> {
    /// 以可选别名解析器构造格式化器.
    #[must_use]
    pub fn new(ty: &'a Type, names: Option<&'a dyn TypeNameResolver>) -> Self {
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
            Type::Opaque(name) => f.write_str(&resolve(*name)),
            Type::App { name, args } => {
                let name = if args.is_empty() {
                    self.names.map_or(*name, |names| names.resolve_name(*name))
                } else {
                    *name
                };
                f.write_str(&resolve(name))?;
                if !args.is_empty() {
                    f.write_str("[")?;
                    write_types(f, args, self.names)?;
                    f.write_str("]")?;
                }
                Ok(())
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
    names: Option<&dyn TypeNameResolver>,
) -> fmt::Result {
    for (index, ty) in types.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}", TypeFormatter::new(ty, names))?;
    }
    Ok(())
}
