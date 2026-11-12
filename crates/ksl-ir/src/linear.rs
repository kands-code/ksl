//! 线性 CPS IR.
//!
//! 由 CPS 树线性化为基本块; 闭包转换与槽位解析在此完成.
//! [`model::Module`] 保留槽位名与类型等展示信息; 执行器使用 [`plan`] 中的执行视图.

mod build;
mod dead;
mod display;
mod model;
pub mod plan;
mod walk;

pub use build::linearize;
pub use model::*;

#[cfg(test)]
mod tests;
