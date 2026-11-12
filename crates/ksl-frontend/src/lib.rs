//! ksl 前端: 词法分析, 语法分析与类型检查.
//!
//! 本层不依赖运行时值, 调度器与 I/O. typed AST 接口位于 `ksl-typed`.

pub mod ast;
pub mod check;
pub mod lexer;
pub mod parser;
pub mod testutil;
pub mod validate;
