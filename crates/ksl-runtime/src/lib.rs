//! ksl 运行时: 值表示, CPS 执行, 调度器, I/O 与模块加载.
//!
//! 本层依赖前端与 catalog, 不依赖命令行与编辑器集成.

pub mod builtin;
pub mod cps;
pub mod ffi;
pub mod fmtstr;
pub mod host;
pub mod io;
pub mod module;
pub mod regex;
pub(crate) mod root;
pub mod runtime;
pub mod terminal;
pub mod testutil;
pub mod time;
pub(crate) mod timer_heap;
pub mod value;
