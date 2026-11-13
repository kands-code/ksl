//! ksl 语言核心库.
//!
//! 按流水线组织: 词法分析, 语法分析, 类型检查, 降级为 CPS IR 与线性 IR 后执行.
//! CPS 即续延传递风格, IR 即中间表示, 是执行模型与降级目标.
//! 设计见仓库根目录下的 `docs/design.md`.

pub mod ast;
pub mod builtin;
pub mod check;
pub mod diagnostic;
pub mod error;
pub mod fmtstr;
pub mod host;
pub mod io;
pub mod lexer;
pub mod module;
pub mod parser;
pub mod rewrite;
pub mod source;
pub mod symbol;
pub mod terminal;
pub mod time;
pub mod value;

pub mod cps;
#[cfg(test)]
mod testutil;
