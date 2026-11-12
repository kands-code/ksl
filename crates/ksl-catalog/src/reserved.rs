//! 保留名: 内置函数, 特殊形式, 字面关键字与内置类型名.
//!
//! 这些名字在值位置与类型位置都不允许用户覆盖.
//! 同一名字在全语言只有一种含义.
//! 中文及其他 Unicode 标识符不与保留名冲突.

use std::collections::HashSet;
use std::sync::LazyLock;

use ksl_base::name::{Name, keywords};

/// 内置不透明类型名; 导出签名检查与保留名共用这一份定义.
pub const BUILTIN_OPAQUE_TYPE_NAMES: &[&str] = &[
    "File",
    "CommandOutput",
    "DateTime",
    "Socket",
    "TcpListener",
    "Regex",
    "RegexMatch",
];

/// 原始类型名; 不可再分解的基础值类型.
pub const PRIMITIVE_TYPE_NAMES: &[&str] = &[
    "i64", "i32", "u8", "u32", "u64", "f32", "f64", "char", "bool", "string", "nil",
];

/// 内置参数化类型构造器名.
pub const TYPE_CONSTRUCTOR_NAMES: &[&str] = &[
    "List",
    "Dict",
    "Set",
    "Pair",
    "Option",
    "Result",
    "Pid",
    "Handler",
];

/// 语言内置类型名; 不含内置 Data 与不透明类型名.
static BASE_TYPE_NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    PRIMITIVE_TYPE_NAMES
        .iter()
        .chain(TYPE_CONSTRUCTOR_NAMES)
        .copied()
        .collect()
});

/// 内置 Data 类型名; 由内置 Data 表派生.
pub static BUILTIN_DATA_TYPE_NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    crate::data::builtin_data_types()
        .iter()
        .map(|data| data.name)
        .collect()
});

/// 内置类型名; 保留名, 编辑器名单与生成脚本共用这一份定义.
pub static BUILTIN_TYPE_NAMES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut names = BASE_TYPE_NAMES.to_vec();
    names.extend(BUILTIN_DATA_TYPE_NAMES.iter().copied());
    names.extend(BUILTIN_OPAQUE_TYPE_NAMES.iter().copied());
    names
});

/// 名字是否为保留名. 内置函数由注册表查询, 其余由本表查询.
pub fn is_reserved(name: &Name) -> bool {
    crate::builtin::id(name).is_some() || NAMES.contains(name)
}

/// 除内置函数外的保留名文本; 内置函数名由内置注册表单独提供.
pub fn names() -> impl Iterator<Item = String> {
    NAMES.iter().map(|name| name.as_str().to_string())
}

/// 内置函数之外的保留名集合.
static NAMES: LazyLock<HashSet<Name>> = LazyLock::new(|| {
    let kw = keywords();
    let mut names: HashSet<Name> = [kw.true_.clone(), kw.false_.clone(), kw.nil_.clone()]
        .into_iter()
        .chain(crate::forms::special_form_names().map(Name::new))
        .collect();
    for text in BUILTIN_TYPE_NAMES.iter() {
        names.insert(Name::new(text));
    }
    names.extend(crate::ops::operation_names().map(Name::new));
    names
});
