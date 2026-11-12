//! 内置函数注册表.
//!
//! 名字与签名由 `ksl-catalog` 提供, 本模块按编号对应运行时实现.
//! 每个实现接收命令行参数, 实参列表与调用点位置, 返回 `Value` 或 ksl 级错误.
//! 实参形态由检查器担保, 形态不符即内部错误.
//! 溢出, 除零, 非法转换等以 ksl 级错误报告, 位置取 IR 调用节点.
//!
//! 实现按数值, 字符串, 正则, 路径与宿主接口分组.

use ksl_base::source::Span;
use ksl_frontend::error::RuntimeError as Error;

use crate::value::Value;

/// 内置函数: 与闭包调用共用分派, 额外接收调用点位置.
///
/// 返回 `Result` 是分派签名的统一形态; 不会失败的实现用
/// `#[allow(clippy::unnecessary_wraps)]` 标注.
/// 内置函数不产生回调; 高阶列表操作由抽象机的续延帧驱动.
pub(crate) type BuiltinFn = fn(&[String], &[Value], Option<Span>) -> Result<Value, Error>;

mod meta;
mod numeric;
mod regex;
mod registry;
mod string;

// 实现函数按注册表名字集中引用; 逐项列出会与注册表维护两份名单.
pub(crate) use meta::*;
pub(crate) use numeric::*;
pub(crate) use regex::*;
pub(crate) use registry::{Builtin, id, name, resolve, signature, symbol};
pub use registry::{BuiltinId, names};
pub(crate) use string::*;

/// 内置函数是否可原地调用且无副作用.
#[must_use]
pub(crate) fn is_pure(id: BuiltinId) -> bool {
    pure(id).is_some()
}

/// 纯内置函数的直接入口; 其他类别返回 None.
#[must_use]
pub(crate) fn pure(id: BuiltinId) -> Option<BuiltinFn> {
    match resolve(id) {
        Some(Builtin::Pure(implementation)) => Some(implementation),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
