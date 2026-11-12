//! 错误目录: 全部用户可见错误的唯一来源.
//!
//! 词法, 语法, 模块, 类型检查, 运行时五类的错误归入同一枚举.
//! 每种错误有编号与规范标题.
//! 编号分段为 `E01xx` 词法, `E02xx` 语法, `E03xx` 模块, `E04xx` 类型检查, `E05xx` 运行时.
//! 编号由过程宏按段基数与枚举声明顺序生成.
//! 编号只用于报告与编辑器诊断, 不是稳定契约.
//!
//! 内部错误属实现缺陷, 不入目录, 单独渲染.
//! 种类可携带数据, 如期望与实得的类型, 实参个数; 措辞由种类生成.

mod kind;
mod ksl;
mod render;
mod runtime;

pub use kind::ErrorKind;
pub use ksl::KslError;
pub use ksl_base::fail::FailKind;
pub use runtime::RuntimeError;

#[cfg(test)]
mod tests;
