//! ksl 前端: 词法分析, 语法分析与类型检查.
//!
//! 本层不依赖运行时值, 调度器与 I/O.

pub mod ast;
pub mod check;
pub mod diagnostic;
pub mod error;
pub mod lexer;
pub mod parser;
pub mod rewrite;
pub mod testutil;
