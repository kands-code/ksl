//! ksl 错误目录与诊断渲染.
//!
//! 词法, 语法, 模块, 类型检查与运行时错误共用同一套编号与渲染.
//! 本层只依赖基础类型与过程宏, 不依赖词法, 语法, 类型检查或运行时.

pub mod diagnostic;
pub mod error;

pub use error::{ErrorKind, FailKind, KslError, RuntimeError};
