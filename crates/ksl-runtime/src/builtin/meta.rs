//! 路径, 命令行参数, 环境变量, 关闭态文件句柄与逻辑内置函数.

use std::path::Path;

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_frontend::error::RuntimeError as Error;

use crate::value::{Value, list, option_none, option_some};

pub(crate) fn join_path(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(base)) = a.first() else {
        return Err(Error::internal("JoinPath 的基准路径不是字符串"));
    };
    let Some(Value::Str(child)) = a.get(1) else {
        return Err(Error::internal("JoinPath 的子路径不是字符串"));
    };
    let joined = Path::new(base.as_str()).join(child.as_str());
    Ok(Value::Str(ArcStr::from(joined.to_string_lossy().as_ref())))
}

/// 取命令行参数; 不会失败, 返回值恒为列表.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn args(arguments: &[String], _: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(list(
        arguments
            .iter()
            .map(|arg| Value::Str(ArcStr::from(arg.as_str()))),
    ))
}

/// 空文件句柄: 处于关闭态, 任何读写都报 `FileError/InvalidHandle`.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn closed_file(_: &[String], _: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(crate::io::closed_file())
}

pub(crate) fn get_env(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(name)) = a.first() else {
        return Err(Error::internal("GetEnv 的实参形态不符"));
    };
    match std::env::var(name.as_str()) {
        Ok(value) => Ok(option_some(Value::Str(ArcStr::from(value)))),
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
