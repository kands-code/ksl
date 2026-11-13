//! 路径, 输出, 环境与逻辑内置函数.

use std::path::Path;
use std::sync::Arc;

use crate::error::RuntimeError as Error;
use crate::source::Span;
use crate::value::{Value, list, option_none, option_some};

pub(crate) fn join_path(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(base)) = a.first() else {
        return Err(Error::internal("JoinPath 的基准路径不是字符串"));
    };
    let Some(Value::Str(child)) = a.get(1) else {
        return Err(Error::internal("JoinPath 的子路径不是字符串"));
    };
    let joined = Path::new(base.as_ref()).join(child.as_ref());
    Ok(Value::Str(Arc::from(joined.to_string_lossy().as_ref())))
}

pub(crate) fn print(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("Print 的实参形态不符"));
    };
    crate::io::print(text)?;
    Ok(Value::Nil)
}

/// 写出标准输出, 不补换行.
///
/// 用于终端控制序列一类需要精确控制落笔位置的情形.
pub(crate) fn print_raw(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("PrintRaw 的实参形态不符"));
    };
    crate::io::print_raw(text)?;
    Ok(Value::Nil)
}

pub(crate) fn print_error(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("PrintError 的实参形态不符"));
    };
    crate::io::print_error(text)?;
    Ok(Value::Nil)
}

/// 取命令行参数; 不会失败, 返回值恒为列表.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn args(arguments: &[String], _: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(list(
        arguments.iter().map(|arg| Value::Str(Arc::from(arg.as_str()))),
    ))
}

/// 空文件句柄: 处于关闭态, 任何读写都报 `IOError/InvalidHandle`.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn closed_file(_: &[String], _: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(crate::io::closed_file())
}

pub(crate) fn get_env(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(name)) = a.first() else {
        return Err(Error::internal("GetEnv 的实参形态不符"));
    };
    match std::env::var(name.as_ref()) {
        Ok(value) => Ok(option_some(Value::Str(Arc::from(value)))),
        Err(_) => Ok(option_none()),
    }
}

pub(crate) fn bool1(a: &[Value]) -> Result<bool, Error> {
    let Some(Value::Bool(x)) = a.first() else {
        return Err(Error::internal("布尔函数的实参形态不符"));
    };
    Ok(*x)
}
pub(crate) fn bool2(a: &[Value]) -> Result<(bool, bool), Error> {
    match (a.first(), a.get(1)) {
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => Ok((*x, *y)),
        _ => Err(Error::internal("布尔函数的实参形态不符")),
    }
}

pub(crate) fn not(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::Bool(!bool1(a)?))
}
pub(crate) fn and(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = bool2(a)?;
    Ok(Value::Bool(x && y))
}
pub(crate) fn or(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = bool2(a)?;
    Ok(Value::Bool(x || y))
}
