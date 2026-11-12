//! 内置函数注册表.
//!
//! 名字, 签名, 实现与编号的唯一来源; 检查器与运行时共用本表. 参数形态已由
//! 检查器担保, 形态不符即内部错误. 溢出, 除零, 非法转换等以 ksl 级错误报告,
//! 位置由 IR 调用节点提供.
//!
//! 实现按数值, 字符串, 路径与输出分组; 注册表只消费名字与签名.

#![allow(clippy::wildcard_imports)]

use crate::error::RuntimeError as Error;
use crate::source::Span;
use crate::value::Value;

/// 内置函数: 与闭包调用共用分派, 额外接收调用点位置.
/// 内置函数不产生回调; 高阶列表操作由抽象机的续延帧驱动.
pub(crate) type BuiltinFn = fn(&[String], &[Value], Option<Span>) -> Result<Value, Error>;

mod meta;
mod numeric;
mod registry;
mod string;

pub(crate) use meta::*;
pub(crate) use numeric::*;
pub use registry::{BuiltinId, names};
pub(crate) use registry::{id, io_builder, lookup, name, signature, symbol};
pub(crate) use string::*;

#[cfg(test)]
mod tests;
