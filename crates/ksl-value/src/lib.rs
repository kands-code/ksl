//! ksl 值表示与宿主资源接口.
//!
//! 值, 宿主资源, 闭包与外来函数; 不依赖抽象机, 调度器与 I/O 服务.
//! 插件只依赖本 crate, 原生插件导出宏也在本层.

pub mod args;
pub mod ffi;
pub mod fmtstr;
pub mod native;
pub mod panic;
pub mod root;
pub mod value;

pub use ksl_macros::ksl_native_plugin;
