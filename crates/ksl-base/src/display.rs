//! 类型显示.
//!
//! 默认按真实类型名显示; 会话可通过别名解析器替换标称类型名.

use std::fmt;

use crate::name::Name;
use crate::types::Type;

/// 标称类型名到显示名的解析.
pub trait TypeNameResolver {
    /// 返回 Data 与 Opaque 标称类型的显示名.
    fn resolve_name(&self, name: Name) -> Name;
}

/// 一份模块局部的类型名显示表.
///
/// 按 `(真实名, 可见名)` 保存; 同一真实名的首个可见名生效.
/// 找不到可见名时回退到真实名.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypeNames {
    aliases: Vec<(Name, Name)>,
}

impl TypeNames {
    /// 由真实名到可见名的映射构造.
    #[must_use]
    pub fn new(aliases: impl IntoIterator<Item = (Name, Name)>) -> Self {
        Self {
            aliases: aliases.into_iter().collect(),
        }
    }

    /// 是否没有保存任何可见名.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
    }

    /// 解析真实名; 找不到可见名时返回原名.
    #[must_use]
    pub fn resolve(&self, name: &Name) -> Name {
        self.aliases
            .iter()
            .find(|(full, _)| full == name)
            .map_or_else(|| name.clone(), |(_, local)| local.clone())
    }
}

impl TypeNameResolver for TypeNames {
    fn resolve_name(&self, name: Name) -> Name {
        self.resolve(&name)
    }
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

    fn resolve(&self, name: &Name) -> Name {
        self.names
            .map_or_else(|| name.clone(), |names| names.resolve_name(name.clone()))
    }
}

impl fmt::Display for TypeFormatter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.ty {
            Type::I64 => f.write_str("i64"),
            Type::I32 => f.write_str("i32"),
            Type::U8 => f.write_str("u8"),
            Type::U32 => f.write_str("u32"),
            Type::U64 => f.write_str("u64"),
            Type::F32 => f.write_str("f32"),
            Type::F64 => f.write_str("f64"),
            Type::Char => f.write_str("char"),
            Type::Bool => f.write_str("bool"),
            Type::Str => f.write_str("string"),
            Type::Nil => f.write_str("nil"),
            Type::Never => f.write_str("never"),
            Type::Data(name) | Type::Opaque(name) => f.write_str(self.resolve(name).as_str()),
            Type::List(inner) => write!(f, "List[{}]", self.nested(inner)),
            Type::Dict(key, value) => {
                write!(f, "Dict[{}, {}]", self.nested(key), self.nested(value))
            },
            Type::Set(inner) => write!(f, "Set[{}]", self.nested(inner)),
            Type::Pair(first, second) => {
                write!(f, "Pair[{}, {}]", self.nested(first), self.nested(second))
            },
            Type::Option(inner) => write!(f, "Option[{}]", self.nested(inner)),
            Type::Result(ok, err) => {
                write!(f, "Result[{}, {}]", self.nested(ok), self.nested(err))
            },
            Type::Pid { mailboxes, ret } => {
                f.write_str("Pid[")?;
                write_types(f, mailboxes, self.names)?;
                write!(f, ", {}]", self.nested(ret))
            },
            Type::Handler { pid, args } => {
                f.write_str("Handler[")?;
                write!(f, "{}", self.nested(pid))?;
                if !args.is_empty() {
                    f.write_str(", ")?;
                    write_types(f, args, self.names)?;
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

#[cfg(test)]
mod tests {
    use super::{TypeFormatter, TypeNames};
    use crate::name::Name;
    use crate::types::Type;

    #[test]
    fn aliases_replace_data_and_opaque_names() {
        let names = TypeNames::new([
            (Name::new("lib/a/A"), Name::new("B")),
            (Name::new("$plugin/demo/Widget"), Name::new("WidgetAlias")),
        ]);
        let data = Type::list(Type::data(Name::new("lib/a/A")));
        assert_eq!(TypeFormatter::new(&data, Some(&names)).to_string(), "List[B]");
        let opaque = Type::opaque(Name::new("$plugin/demo/Widget"));
        assert_eq!(
            TypeFormatter::new(&opaque, Some(&names)).to_string(),
            "WidgetAlias"
        );
        assert_eq!(
            TypeFormatter::new(&opaque, None).to_string(),
            "$plugin/demo/Widget"
        );
        let other = Type::data(Name::new("lib/b/C"));
        assert_eq!(TypeFormatter::new(&other, Some(&names)).to_string(), "lib/b/C");
    }
}
