//! f-string 格式化引擎.
//!
//! 规则见 `docs/topics/language/syntax.md` 第 4.4 节.
//! 基础值, 浅层容器, Pid 与函数可格式化.
//! string 与 char 默认省略引号, `?` 显式加引号.
//! 数值接受类型限定的格式参数.
//! Data, Option, Result, Dict, Set 与全部不透明类型不可打印.

use ksl_base::format::{Spec, format_float_default};
use ksl_base::symbol::resolve;
use ksl_frontend::error::RuntimeError as Error;

use crate::builtin;
use crate::value::Value;

fn parse_spec(text: Option<&str>) -> Result<Spec, Error> {
    Spec::parse(text).map_err(Error::internal)
}

/// f-string 格式化: 顶层 string 与 char 默认省略引号.
///
/// # Errors
///
/// spec 解析失败返回内部错误; 正常经检查器校验的程序不会发生.
pub fn format_value(value: &Value, spec: Option<&str>) -> Result<String, Error> {
    if spec == Some("?") {
        return match value {
            Value::Str(text) => Ok(format!("{text:?}")),
            Value::Char(c) => Ok(format!("{c:?}")),
            _ => Err(Error::internal("`?` 只适用于 string 与 char")),
        };
    }
    match value {
        Value::Str(text) => Ok(text.to_string()),
        Value::Char(c) => Ok(c.to_string()),
        Value::I64(i) => match spec {
            Some(spec) => Ok(parse_spec(Some(spec))?
                .apply_int(i128::from(*i), *i < 0, "")
                .map_err(Error::internal)?),
            None => Ok(i.to_string()),
        },
        Value::U8(u) => match spec {
            Some(spec) => Ok(parse_spec(Some(spec))?
                .apply_int(i128::from(*u), false, "u")
                .map_err(Error::internal)?),
            None => Ok(format!("{u}u")),
        },
        Value::U64(u) => match spec {
            Some(spec) => Ok(parse_spec(Some(spec))?
                .apply_int(i128::from(*u), false, "U")
                .map_err(Error::internal)?),
            None => Ok(format!("{u}U")),
        },
        Value::F64(f) => match spec {
            Some(spec) => Ok(parse_spec(Some(spec))?.apply_float(*f)),
            None => Ok(format_float_default(*f)),
        },
        Value::Bool(b) => match spec {
            None => Ok(b.to_string()),
            Some(_) => Err(Error::internal("bool 不接受格式参数")),
        },
        Value::Nil => match spec {
            None => Ok("nil".to_string()),
            Some(_) => Err(Error::internal("nil 不接受格式参数")),
        },
        Value::Pid(pid) => match spec {
            None => Ok(format!("<Pid {}>", pid.id)),
            Some(_) => Err(Error::internal("Pid 不接受格式参数")),
        },
        Value::List(_) | Value::Pair(_) => match spec {
            None => Ok(render_structural(value)),
            Some(_) => Err(Error::internal("List 与 Pair 不接受格式参数")),
        },
        Value::Closure(_) | Value::Builtin(_) | Value::ForeignFn(_) => match spec {
            None => Ok(render_callable(value)),
            Some(_) => Err(Error::internal("函数不接受格式参数")),
        },
        Value::Data(_)
        | Value::Dict(_)
        | Value::Set(_)
        | Value::File(_)
        | Value::CommandOutput(_)
        | Value::Socket(_)
        | Value::TcpListener(_)
        | Value::DateTime(_)
        | Value::Regex(_)
        | Value::RegexMatch(_)
        | Value::ForeignHandle(_) => Err(Error::internal("类型不可打印")),
    }
}

/// REPL 结果展示: string 与 char 始终带引号.
#[must_use]
pub fn display_value(value: &Value) -> String {
    render_structural(value)
}

/// 结构化值的默认渲染: 容器内字符串与字符带引号.
fn render_structural(value: &Value) -> String {
    match value {
        Value::Str(text) => format!("{text:?}"),
        Value::Char(c) => format!("{c:?}"),
        Value::I64(i) => i.to_string(),
        Value::U8(u) => format!("{u}u"),
        Value::U64(u) => format!("{u}U"),
        Value::F64(f) => format_float_default(*f),
        Value::Bool(b) => b.to_string(),
        Value::List(list) => {
            let inner: Vec<String> = match &**list {
                crate::value::List::I64(items) => {
                    items.iter().map(ToString::to_string).collect()
                },
                crate::value::List::U8(items) => {
                    items.iter().map(|value| format!("{value}u")).collect()
                },
                crate::value::List::U64(items) => {
                    items.iter().map(|value| format!("{value}U")).collect()
                },
                crate::value::List::F64(items) => {
                    items.iter().map(|value| format_float_default(*value)).collect()
                },
                crate::value::List::Char(items) => {
                    items.iter().map(|value| format!("{value:?}")).collect()
                },
                crate::value::List::Bool(items) => {
                    items.iter().map(ToString::to_string).collect()
                },
                crate::value::List::Values(items) => {
                    items.iter().map(render_structural).collect()
                },
            };
            format!("{{{}}}", inner.join(", "))
        },
        Value::Pair(pair) => {
            format!(
                "({}, {})",
                render_structural(&pair.0),
                render_structural(&pair.1)
            )
        },
        // 不可打印类型只可能来自检查器漏检; 输出类型摘要以保持可读.
        Value::Data(data) => format!("<{}>", resolve(data.ty)),
        Value::Nil => "nil".to_string(),
        Value::File(_) => "<File>".to_string(),
        Value::CommandOutput(_) => "<CommandOutput>".to_string(),
        Value::Socket(_) => "<Socket>".to_string(),
        Value::TcpListener(_) => "<TcpListener>".to_string(),
        Value::DateTime(_) => "<DateTime>".to_string(),
        Value::Regex(_) => "<Regex>".to_string(),
        Value::RegexMatch(_) => "<RegexMatch>".to_string(),
        Value::ForeignHandle(handle) => format!("<{}>", resolve(handle.ty())),
        Value::Pid(pid) => format!("<Pid {}>", pid.id),
        Value::Dict(_) => "<Dict>".to_string(),
        Value::Set(_) => "<Set>".to_string(),
        Value::Closure(_) | Value::Builtin(_) | Value::ForeignFn(_) => render_callable(value),
    }
}

/// 函数显示: 具名函数加名字, 匿名函数只显示签名.
fn render_callable(value: &Value) -> String {
    match value {
        Value::Closure(closure) => {
            let signature = closure.signature_text();
            match closure.name {
                Some(name) => format!("{} : {signature}", resolve(name)),
                None => signature,
            }
        },
        Value::Builtin(id) => {
            let symbol = crate::builtin::symbol(*id);
            match builtin::signature(symbol) {
                Some(ty) => format!("{} : {ty}", resolve(symbol)),
                None => resolve(symbol).to_string(),
            }
        },
        Value::ForeignFn(function) => {
            format!("{} : {}", function.name(), function.declared())
        },
        _ => "<函数>".to_string(),
    }
}
