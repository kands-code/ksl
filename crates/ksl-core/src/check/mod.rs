//! 类型检查.
//!
//! 环境驱动的单趟检查: `@Data` 声明先行收集, 语句按顺序检查,
//! 环境随语句增长; 求值器可信赖检查通过的程序.

mod collection;
mod context;
mod data;
mod display;
mod env;
mod expr;
mod infer;
mod list;
mod option;
mod pattern;
mod registry;
mod reserved;
mod session;
mod special;
mod type_expr;
mod types;

pub use context::ModuleContext;
use context::TypeContext;
pub(crate) use data::{DataTable, collect_data};
pub use display::{TypeTable, display_type, is_base_printable, is_printable};
use env::Env;
use indexmap::IndexMap;
use infer::{InferCtx, InferTy};
pub use session::{Session, apply_type_names, check_program};
pub use types::{HandlerType, PidType, Type};

use crate::ast::{ExprKind, NodeKey, Program, Special};
use crate::symbol::Symbol;

/// 判断名字是否为语言保留名; 供模块链接阶段检查本地别名与 REPL 着色.
#[must_use]
pub fn is_reserved(name: Symbol) -> bool {
    reserved::is_reserved(name)
}

/// 内置类型名; 供编辑器名单核对使用.
#[must_use]
pub fn builtin_type_names() -> &'static [&'static str] {
    reserved::BUILTIN_TYPE_NAMES
}

/// 内置 Data 类型名; 模块导出检查使用.
#[must_use]
pub fn builtin_data_type_names() -> &'static [&'static str] {
    reserved::BUILTIN_DATA_TYPE_NAMES
}

/// 非内置函数的保留名文本.
pub fn reserved_names() -> impl Iterator<Item = String> {
    reserved::names()
}

/// `@Main` 的 self 信箱数; 程序末尾不是 `@Main` 时返回 None.
///
/// 检查器已保证 `@Main` 的类型是 Handler 且 self 是 Pid, 此处只取值.
#[must_use]
pub(crate) fn main_mailboxes(program: &Program, types: &TypeTable) -> Option<usize> {
    let tail = program.tail.as_ref()?;
    if !matches!(&tail.node, ExprKind::Special(Special::Main { .. })) {
        return None;
    }
    let (pid, _) = types.get(&tail.key())?.as_handler()?;
    let (mailboxes, _) = pid.as_pid()?;
    Some(mailboxes.len())
}

/// 当前 Handler 体检查上下文.
#[derive(Debug, Clone)]
struct ActorContext {
    handler: HandlerType,
    /// 是否为 `@Main` 体; `@Halt` 与 `@Continue` 只在此处额外禁用.
    main: bool,
}

#[derive(Clone)]
struct Checker {
    context: TypeContext,
    env: Env,
    /// 检查期的类型表; 值可能含未定类型变量, 检查结束时解析为具体类型.
    ///
    /// 保持插入顺序, 使结束时首个未定类型错误的定位可复现.
    infer_types: IndexMap<NodeKey, InferTy>,
    /// 本次检查的类型变量与统一状态.
    infer: InferCtx,
    /// 当前是否位于 Handler 体内; 决定 Receive/Continue 等原语是否可用.
    actor: Option<ActorContext>,
}

#[cfg(test)]
mod tests;
