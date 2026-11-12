//! 类型检查.
//!
//! 环境驱动的单趟检查.
//! `@Data` 声明先行收集, 语句按顺序检查, 环境随语句增长.
//! 求值器可信赖检查通过的程序.

mod collection;
mod context;
mod data;
mod display;
mod env;
mod expected;
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
pub use data::{DataTable, collect_data};
pub use display::{TypeTable, display_type, is_base_printable, is_printable};
use env::Env;
use indexmap::IndexMap;
use infer::{InferCtx, InferTy};
use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, resolve};
pub use session::{Session, apply_type_names, check_program};
pub use types::{HandlerType, PidType, Type};

use crate::ast::NodeKey;

/// 判断名字是否为语言保留名. 供模块链接阶段检查本地别名与 REPL 着色.
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

/// 内置不透明类型名; 模块导出检查使用.
#[must_use]
pub fn builtin_opaque_type_names() -> &'static [&'static str] {
    reserved::BUILTIN_OPAQUE_TYPE_NAMES
}

/// 非内置函数的保留名文本.
pub fn reserved_names() -> impl Iterator<Item = String> {
    reserved::names()
}

/// 校验 `@Link` 声明中的类型是否落在插件 ABI 支持范围内.
///
/// 函数参数与返回值只能是标量, 字符串, `nil` 或不透明类型; 返回值另可为 `Result[T, string]`.
/// 非函数值只能是标量, 字符串, `nil` 或不透明类型.
///
/// # Errors
///
/// 返回第一个不受支持的类型文本.
pub fn validate_plugin_type(ty: &Type) -> Result<(), String> {
    match ty {
        Type::Fn(params, ret) => {
            for param in params {
                validate_plugin_value(param)?;
            }
            validate_plugin_return(ret)
        },
        other => validate_plugin_value(other),
    }
}

fn validate_plugin_return(ty: &Type) -> Result<(), String> {
    let Type::App { name, args } = ty else {
        return validate_plugin_value(ty);
    };
    if &*resolve(*name) != "Result" {
        return validate_plugin_value(ty);
    }
    if args.len() != 2 || args[1] != Type::Str {
        return Err("Result 的错误类型必须是 string".to_string());
    }
    validate_plugin_value(&args[0])
}

fn validate_plugin_value(ty: &Type) -> Result<(), String> {
    match ty {
        Type::I64
        | Type::U64
        | Type::U8
        | Type::F64
        | Type::Bool
        | Type::Char
        | Type::Str
        | Type::Opaque(_)
        | Type::Nil => Ok(()),
        other => Err(other.to_string()),
    }
}

/// 当前 Handler 体检查上下文.
#[derive(Debug, Clone)]
struct ActorContext {
    handler: HandlerType,
    /// 是否为 `@Main` 体; 为真时额外禁用 `@Halt` 与 `@Continue`.
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
    /// 检查结束时仍需校验的 Dict 与 Set 键类型.
    ///
    /// 键类型在出现位置可能尚未由兄弟表达式或外层期望确定, 因此推迟到本轮检查结束.
    pending_keys: Vec<(InferTy, Span)>,
    /// 当前是否位于 Handler 体内; 决定 Receive/Continue 等原语是否可用.
    actor: Option<ActorContext>,
}

#[cfg(test)]
mod tests;
