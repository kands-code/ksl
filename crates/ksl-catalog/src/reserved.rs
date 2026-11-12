//! 保留名: 内置函数, 特殊形式, 字面关键字与内置类型名.
//!
//! 这些名字在值位置与类型位置都不允许用户覆盖.
//! 同一名字在全语言只有一种含义.
//! 中文及其他 Unicode 标识符不与保留名冲突.

use std::collections::HashSet;
use std::sync::LazyLock;

use ksl_base::symbol::{Symbol, intern, keywords};

/// 生成内置 Data 类型名, 内置不透明类型名与内置类型名三份名单.
///
/// Data 与不透明类型名同时是内置类型名的一部分, 共用一份列表避免多处漂移.
macro_rules! define_builtin_type_names {
    ($($data:literal),+ $(,)? ; $($opaque:literal),+ $(,)?) => {
        /// 内置 Data 类型名; 导出签名检查与保留名共用这一份定义.
        pub const BUILTIN_DATA_TYPE_NAMES: &[&str] = &[$($data),+];

        /// 内置不透明类型名; 导出签名检查与保留名共用这一份定义.
        pub const BUILTIN_OPAQUE_TYPE_NAMES: &[&str] = &[$($opaque),+];

        /// 内置类型名; 保留名, 编辑器名单与生成脚本共用这一份定义.
        pub const BUILTIN_TYPE_NAMES: &[&str] = &[
            "i64",
            "u8",
            "u64",
            "f64",
            "char",
            "bool",
            "string",
            "List",
            "Dict",
            "Set",
            "Pair",
            "Option",
            "Result",
            "Pid",
            "Handler",
            $($data),+,
            $($opaque),+,
        ];
    };
}

define_builtin_type_names! {
    "ExitReason",
    "FileError",
    "InputError",
    "NumberParseError",
    "Utf8Error",
    "SocketError",
    "TimeError",
    "KeyEvent",
    "RegexError",
    "CommandInput",
    "CommandOption",
    "CommandStatus",
    "CommandError",
    ;
    "File",
    "CommandOutput",
    "DateTime",
    "Socket",
    "TcpListener",
    "Regex",
    "RegexMatch",
}

/// 名字是否为保留名. 内置函数由注册表查询, 其余由本表查询.
pub fn is_reserved(name: Symbol) -> bool {
    crate::builtin::id(name).is_some() || NAMES.contains(&name)
}

/// 除内置函数外的保留名文本; 内置函数名由内置注册表单独提供.
pub fn names() -> impl Iterator<Item = String> {
    NAMES
        .iter()
        .map(|name| ksl_base::symbol::resolve(*name).to_string())
}

/// 内置函数之外的保留名集合.
static NAMES: LazyLock<HashSet<Symbol>> = LazyLock::new(|| {
    let kw = keywords();
    let mut names: HashSet<Symbol> = [kw.true_, kw.false_, kw.nil_]
        .into_iter()
        .chain(crate::forms::special_form_names().map(intern))
        .collect();
    for text in BUILTIN_TYPE_NAMES {
        names.insert(intern(text));
    }
    names.extend(crate::ops::operation_names().map(intern));
    names
});
