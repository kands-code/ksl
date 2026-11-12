//! ksl 执行计划数据.
//!
//! 线性 IR 与执行视图共用的类型; 值表示与抽象机只依赖本层.

pub mod atom;
mod model;

pub use model::*;
