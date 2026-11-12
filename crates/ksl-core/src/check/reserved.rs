//! 保留名: 内置函数, 特殊形式, 字面关键字, 内置类型与构造器.
//!
//! 这些名字在值位置与类型位置都不允许用户覆盖, 以保证同一名字在全语言
//! 只有一种含义. 中文及其他 Unicode 标识符不受影响.

use std::collections::HashSet;
use std::sync::LazyLock;

use crate::symbol::{Symbol, intern, keywords};

/// 内置 Data 类型名; 导出签名检查与保留名共用这一份定义.
pub const BUILTIN_DATA_TYPE_NAMES: &[&str] =
    &["ExitReason", "IOError", "TimeError", "KeyEvent"];

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
    "ExitReason",
    "IOError",
    "TimeError",
    "KeyEvent",
    "File",
    "DateTime",
];

/// 判断名字是否为保留名.
pub fn is_reserved(name: Symbol) -> bool {
    crate::builtin::id(name).is_some() || NAMES.contains(&name)
}

/// 除内置函数外的保留名文本; 内置函数名由内置注册表单独提供.
pub fn names() -> impl Iterator<Item = String> {
    NAMES
        .iter()
        .map(|name| crate::symbol::resolve(*name).to_string())
}

/// 内置函数之外的保留名集合; 内置函数名由内置注册表判定, 其余在此登记.
static NAMES: LazyLock<HashSet<Symbol>> = LazyLock::new(|| {
    let kw = keywords();
    let mut names: HashSet<Symbol> = [
        // 字面关键字与特殊形式.
        kw.true_,
        kw.false_,
        kw.nil_,
        kw.let_,
        kw.block_,
        kw.if_,
        kw.fn_,
        kw.def_,
        kw.exit_,
        kw.continue_,
        kw.spawn_,
        kw.spawn_detached_,
        kw.send_,
        kw.receive_,
        kw.join_,
        kw.kill_,
        kw.monitor_,
        kw.unmonitor_,
        kw.pid_serial_,
        kw.set_timer_,
        kw.cancel_timer_,
        kw.halt_,
        kw.sleep_,
        kw.fix_,
        kw.data_,
        kw.export_,
        kw.use_,
        kw.match_,
        kw.assert_,
        kw.panic_,
        kw.todo_,
        kw.unreachable_,
        kw.handler_,
    ]
    .into_iter()
    .collect();
    // 内置类型名由同一份定义提供, 生成脚本也读取它.
    for text in BUILTIN_TYPE_NAMES {
        names.insert(intern(text));
    }
    // 操作名由 ast::operation_names 提供, 避免与解析表重复.
    names.extend(crate::ast::operation_names().map(intern));
    names
});
