//! CPS 后端: 把类型检查后的 AST 降级为函数级 CPS IR, 再线性化为基本块.
//!
//! 执行模型采用续延传递风格, 下文统一简称 CPS; 中间表示简称 IR.
//! 输入是 `ksl_frontend::ast::Program` 与检查器给出的 `TypeTable`.
//! 产物是 `ir::Module`, 线性化后得到 `linear::Module`.
//! 执行器只接受线性化产物; 抽象机与调度器位于 `crate::runtime`.

pub(crate) mod atom;
pub(crate) mod display;
pub mod ir;
pub mod linear;
