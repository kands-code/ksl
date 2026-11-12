//! ksl 过程宏.
//!
//! 错误编号按阶段与声明顺序生成; 标题与详情手写.
//!
//! 插件导出宏生成统一入口与静态描述符, 插件作者只写普通 Rust 函数.

use proc_macro::TokenStream;

mod error_number;
mod native;

/// 为错误枚举生成 `number`.
///
/// 变体按声明顺序编号.
/// 遇到 `#[phase(基数)]` 时重置为该阶段基数.
/// 首个变体必须带阶段标记.
#[proc_macro_derive(ErrorNumber, attributes(phase))]
pub fn derive_error_number(input: TokenStream) -> TokenStream {
    error_number::expand(input)
}

/// 把一个模块导出为原生 ksl 插件.
///
/// 参数为 `name = "..."` 与可选的 `version = "..."`.
/// 模块内用 `#[ksl_native_export]` 标记导出函数; 函数可写类型化签名,
/// 单参数 `&[Value]` 视为原始入口.
#[proc_macro_attribute]
pub fn ksl_native_plugin(args: TokenStream, input: TokenStream) -> TokenStream {
    native::expand(args.into(), input.into()).into()
}
