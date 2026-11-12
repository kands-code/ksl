//! 内置函数注册表.
//!
//! 名字与签名由 `ksl-catalog` 提供, 本模块按编号对应运行时实现.
//! 每个实现接收实参列表与调用点位置, 返回 `Value` 或 ksl 级错误.
//! 实参形态由检查器担保, 形态不符即内部错误.
//! 溢出, 除零, 非法转换等以 ksl 级错误报告, 位置取 IR 调用节点.
//!
//! 实现按数值, 字符串, 正则, 路径与宿主接口分组.

use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::Value;

/// 内置函数: 与闭包调用共用分派, 额外接收调用点位置.
///
/// 全部内置构造器统一返回 `Result`.
/// 内置函数不产生回调; 高阶列表操作由抽象机的续延帧驱动.
pub(crate) type BuiltinFn = fn(&[Value], Option<Span>) -> Result<Value, Error>;

mod meta;
mod numeric;
mod regex;
mod registry;
mod string;

// 实现函数按分组整体导入; 完整名单由注册表维护.
pub(crate) use meta::*;
pub(crate) use numeric::*;
pub(crate) use regex::*;
pub(crate) use registry::{Builtin, resolve};
pub use registry::{BuiltinId, names};
pub(crate) use string::*;

#[cfg(test)]
mod tests;
