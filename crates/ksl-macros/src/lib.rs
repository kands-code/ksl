//! ksl 过程宏.
//!
//! 错误目录的编号按阶段与声明顺序自动生成; 手写数字在插入或调整顺序后容易失配.
//! 标题与详情仍需人工维护.
//!
//! 插件导出宏生成统一入口与静态描述符, 插件作者只写普通 Rust 函数.

use proc_macro::TokenStream;

mod error_number;
mod plugin;

/// 为错误枚举生成 `number`.
///
/// 变体按声明顺序编号.
/// 遇到 `#[phase(基数)]` 时重置为该阶段基数.
/// 首个变体必须带阶段标记.
#[proc_macro_derive(ErrorNumber, attributes(phase))]
pub fn derive_error_number(input: TokenStream) -> TokenStream {
    error_number::expand(input)
}

/// 把一个模块导出为 ksl 插件.
///
/// 参数为 `name = "..."` 与可选的 `version = "..."`.
/// 模块内用 `#[ksl_export]` 标记函数, 用 `#[ksl_global]` 标记常量或静态值.
/// 两者都可带字符串参数指定暴露给 ksl 的名字.
#[proc_macro_attribute]
pub fn ksl_plugin(args: TokenStream, input: TokenStream) -> TokenStream {
    plugin::expand(args, input)
}
