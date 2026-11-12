//! 内置函数实参提取.
//!
//! 检查器保证实参类型; 这里的失败都表示实现与检查器不一致.
//! 取值与类型检查共用同一套消息形态.

use std::sync::Arc;

use arcstr::ArcStr;
use ksl_diagnostics::error::RuntimeError as Error;

use crate::value::{List, Value};

fn arg<'a>(args: &'a [Value], index: usize, what: &str) -> Result<&'a Value, Error> {
    args.get(index)
        .ok_or_else(|| Error::internal(format!("{what} 缺少实参")))
}

/// 取字符串实参借用.
///
/// # Errors
///
/// 实参缺失, 或实参不是 string 时返回内部错误.
pub fn str_arg<'a>(args: &'a [Value], index: usize, what: &str) -> Result<&'a str, Error> {
    match arg(args, index, what)? {
        Value::Str(text) => Ok(text),
        _ => Err(Error::internal(format!("{what} 不是 string"))),
    }
}

/// 取共享字符串实参; 供需要跨线程传递的任务使用.
///
/// # Errors
///
/// 实参缺失, 或实参不是 string 时返回内部错误.
pub fn string_arg(args: &[Value], index: usize, what: &str) -> Result<ArcStr, Error> {
    match arg(args, index, what)? {
        Value::Str(text) => Ok(text.clone()),
        _ => Err(Error::internal(format!("{what} 不是 string"))),
    }
}

/// 取 i64 实参.
///
/// # Errors
///
/// 实参缺失, 或实参不是 i64 时返回内部错误.
pub fn i64_arg(args: &[Value], index: usize, what: &str) -> Result<i64, Error> {
    match arg(args, index, what)? {
        Value::I64(value) => Ok(*value),
        _ => Err(Error::internal(format!("{what} 不是 i64"))),
    }
}

/// 取 u64 实参.
///
/// # Errors
///
/// 实参缺失, 或实参不是 u64 时返回内部错误.
pub fn u64_arg(args: &[Value], index: usize, what: &str) -> Result<u64, Error> {
    match arg(args, index, what)? {
        Value::U64(value) => Ok(*value),
        _ => Err(Error::internal(format!("{what} 不是 u64"))),
    }
}

/// 取 bool 实参.
///
/// # Errors
///
/// 实参缺失, 或实参不是 bool 时返回内部错误.
pub fn bool_arg(args: &[Value], index: usize, what: &str) -> Result<bool, Error> {
    match arg(args, index, what)? {
        Value::Bool(value) => Ok(*value),
        _ => Err(Error::internal(format!("{what} 不是 bool"))),
    }
}

/// 取列表实参.
///
/// # Errors
///
/// 实参缺失, 或实参不是 List 时返回内部错误.
pub fn list_arg<'a>(
    args: &'a [Value],
    index: usize,
    what: &str,
) -> Result<&'a Arc<List>, Error> {
    match arg(args, index, what)? {
        Value::List(list) => Ok(list),
        _ => Err(Error::internal(format!("{what} 不是 List"))),
    }
}

/// 取 `List[u8]` 实参并复制为字节向量.
///
/// # Errors
///
/// 实参缺失, 不是 `List[u8]`, 或包含非 u8 元素时返回内部错误.
pub fn bytes_arg(args: &[Value], index: usize, what: &str) -> Result<Vec<u8>, Error> {
    let list = list_arg(args, index, what)?;
    match &**list {
        List::U8(items) => Ok(items.iter().copied().collect()),
        List::Values(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                let Value::U8(byte) = item else {
                    return Err(Error::internal(format!("{what} 包含非 u8 元素")));
                };
                bytes.push(*byte);
            }
            Ok(bytes)
        },
        _ => Err(Error::internal(format!("{what} 不是 List[u8]"))),
    }
}
