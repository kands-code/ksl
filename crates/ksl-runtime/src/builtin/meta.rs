//! 路径, 环境变量, 关闭态文件句柄与逻辑内置函数.

use std::path::Path;

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::args::{bool_arg, str_arg};
use ksl_value::value::{Value, option_none, option_some};

pub(crate) fn join_path(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let base = str_arg(a, 0, "JoinPath 的基准路径")?;
    let child = str_arg(a, 1, "JoinPath 的子路径")?;
    let joined = Path::new(base).join(child);
    Ok(Value::Str(ArcStr::from(joined.to_string_lossy().as_ref())))
}

/// 空文件句柄: 处于关闭态, 任何读写都报 `FileError/InvalidHandle`.
// 内置函数构造器统一返回 Result.
#[expect(clippy::unnecessary_wraps)]
pub(crate) fn closed_file(_: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(ksl_io::io::closed_file())
}

pub(crate) fn get_env(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let name = str_arg(a, 0, "GetEnv 的实参")?;
    match std::env::var(name) {
        Ok(value) => Ok(option_some(Value::Str(ArcStr::from(value)))),
        Err(_) => Ok(option_none()),
    }
}

pub(crate) fn bool1(a: &[Value]) -> Result<bool, Error> {
    bool_arg(a, 0, "bool 函数的实参")
}
pub(crate) fn bool2(a: &[Value]) -> Result<(bool, bool), Error> {
    Ok((
        bool_arg(a, 0, "bool 函数的实参")?,
        bool_arg(a, 1, "bool 函数的实参")?,
    ))
}

pub(crate) fn not(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::Bool(!bool1(a)?))
}
pub(crate) fn and(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = bool2(a)?;
    Ok(Value::Bool(x && y))
}
pub(crate) fn or(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = bool2(a)?;
    Ok(Value::Bool(x || y))
}
