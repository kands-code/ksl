//! ksl 基础类型.
//!
//! 名字, 源码区间, 语言类型表示与失败类别.
//! 本层不依赖词法, 语法, 类型检查与运行时.

pub mod display;
pub mod fail;
pub mod format;
pub mod name;
pub mod plugin;
pub mod source;
pub mod testutil;
pub mod types;
