//! 线性 CPS IR.
//!
//! 由 CPS 树线性化为基本块; 闭包转换与槽位解析在此完成, 是当前执行后端的输入.

mod display;
mod free;
mod lower;
mod model;

pub use lower::linearize;
pub use model::*;

#[cfg(test)]
mod tests;
