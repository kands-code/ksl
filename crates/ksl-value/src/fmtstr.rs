//! 格式化字符串引擎.
//!
//! 规则见 `docs/topics/language/syntax.md` 第 4.4 节至第 4.7 节.
//! 基础值, 浅层容器与 Pid 可格式化.
//! string 与 char 默认省略引号, `?` 输出带引号的源码形式.
//! 数值默认不带类型后缀, `?` 输出带后缀的源码形式; 后缀计入宽度.
//! Data, Option, Result, Dict, Set, 函数与全部不透明类型不可打印.

use ksl_base::format::{Spec, format_float_default, format_float32_default};
use ksl_diagnostics::error::RuntimeError as Error;

use crate::value::Value;

fn parse_spec(text: &str) -> Result<Spec, Error> {
    Spec::parse(Some(text)).map_err(Error::internal)
}

/// 格式化字符串: 顶层 string 与 char 默认省略引号.
///
/// # Errors
///
/// spec 解析失败返回内部错误.
pub fn format_value(value: &Value, spec: Option<&str>) -> Result<String, Error> {
    let spec = spec.map(parse_spec).transpose()?;
    match value {
        Value::Str(text) => match &spec {
            Some(spec) => Ok(spec.apply_text(&if spec.repr() {
                format!("{text:?}")
            } else {
                text.to_string()
            })),
            None => Ok(text.to_string()),
        },
        Value::Char(c) => match &spec {
            Some(spec) => Ok(spec.apply_text(&if spec.repr() {
                format!("{c:?}")
            } else {
                c.to_string()
            })),
            None => Ok(c.to_string()),
        },
        Value::I64(i) => match &spec {
            Some(spec) => Ok(spec
                .apply_int(i128::from(*i), *i < 0, suffix(spec, "i64"))
                .map_err(Error::internal)?),
            None => Ok(i.to_string()),
        },
        Value::I32(i) => match &spec {
            Some(spec) => Ok(spec
                .apply_int(i128::from(*i), *i < 0, suffix(spec, "i32"))
                .map_err(Error::internal)?),
            None => Ok(i.to_string()),
        },
        Value::U8(u) => match &spec {
            Some(spec) => Ok(spec
                .apply_int(i128::from(*u), false, suffix(spec, "u8"))
                .map_err(Error::internal)?),
            None => Ok(u.to_string()),
        },
        Value::U32(u) => match &spec {
            Some(spec) => Ok(spec
                .apply_int(i128::from(*u), false, suffix(spec, "u32"))
                .map_err(Error::internal)?),
            None => Ok(u.to_string()),
        },
        Value::U64(u) => match &spec {
            Some(spec) => Ok(spec
                .apply_int(i128::from(*u), false, suffix(spec, "u64"))
                .map_err(Error::internal)?),
            None => Ok(u.to_string()),
        },
        Value::F32(f) => match &spec {
            Some(spec) => Ok(spec.apply_float32(*f, suffix(spec, "f32"))),
            None => Ok(format_float32_default(*f)),
        },
        Value::F64(f) => match &spec {
            Some(spec) => Ok(spec.apply_float(*f, suffix(spec, "f64"))),
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
        Value::Closure(_)
        | Value::Builtin(_)
        | Value::ForeignFn(_)
        | Value::Data(_)
        | Value::Dict(_)
        | Value::Set(_)
        | Value::NativeOpaque(_) => Err(Error::internal("类型不可打印")),
    }
}

/// 只在 `?` 时附加类型后缀; 否则源码形式不参与排版.
fn suffix(spec: &Spec, text: &'static str) -> &'static str {
    if spec.repr() { text } else { "" }
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
        Value::I64(i) => format!("{i}i64"),
        Value::I32(i) => format!("{i}i32"),
        Value::U8(u) => format!("{u}u8"),
        Value::U32(u) => format!("{u}u32"),
        Value::U64(u) => format!("{u}u64"),
        Value::F32(f) => format!("{}f32", format_float32_default(*f)),
        Value::F64(f) => format!("{}f64", format_float_default(*f)),
        Value::Bool(b) => b.to_string(),
        Value::List(list) => {
            let inner: Vec<String> = match &**list {
                crate::value::List::I64(items) => {
                    items.iter().map(|value| format!("{value}i64")).collect()
                },
                crate::value::List::I32(items) => {
                    items.iter().map(|value| format!("{value}i32")).collect()
                },
                crate::value::List::U8(items) => {
                    items.iter().map(|value| format!("{value}u8")).collect()
                },
                crate::value::List::U32(items) => {
                    items.iter().map(|value| format!("{value}u32")).collect()
                },
                crate::value::List::U64(items) => {
                    items.iter().map(|value| format!("{value}u64")).collect()
                },
                crate::value::List::F32(items) => items
                    .iter()
                    .map(|value| format!("{}f32", format_float32_default(*value)))
                    .collect(),
                crate::value::List::F64(items) => items
                    .iter()
                    .map(|value| format!("{}f64", format_float_default(*value)))
                    .collect(),
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
        // 不可打印类型只可能来自检查器漏检; 此时输出类型摘要.
        Value::Data(data) => format!("<{}>", data.ty.as_str()),
        Value::Nil => "nil".to_string(),
        Value::NativeOpaque(value) => format!("<{}>", value.type_name()),
        Value::Pid(pid) => format!("<Pid {}>", pid.id),
        Value::Dict(_) => "<Dict>".to_string(),
        Value::Set(_) => "<Set>".to_string(),
        // 函数没有文本表示; 类型由检查器与 REPL 的静态类型展示.
        Value::Closure(_) | Value::Builtin(_) | Value::ForeignFn(_) => "<Fn>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{display_value, format_value};
    use crate::value::{List, Value};

    #[test]
    fn f32_suffix_only_in_repr() {
        assert_eq!(format_value(&Value::F32(1.5), None).unwrap(), "1.5");
        assert_eq!(format_value(&Value::F32(0.1), None).unwrap(), "0.1");
        assert_eq!(
            format_value(&Value::F32(1.0 / 3.0), None).unwrap(),
            "0.33333334"
        );
        assert_eq!(format_value(&Value::F32(1.5), Some("?")).unwrap(), "1.5f32");
        assert_eq!(format_value(&Value::F32(1.5), Some(".2")).unwrap(), "1.50");
        assert_eq!(format_value(&Value::F32(1.5), Some(".2?")).unwrap(), "1.50f32");
        assert_eq!(format_value(&Value::F32(1.5), Some(">8?")).unwrap(), "  1.5f32");
        assert_eq!(format_value(&Value::F32(f32::NAN), None).unwrap(), "NaN");
        assert_eq!(
            format_value(&Value::F32(f32::NAN), Some("?")).unwrap(),
            "NaNf32"
        );
        // REPL 的结构化显示始终使用源码形式.
        assert_eq!(
            display_value(&Value::List(Arc::new(List::f32s([0.1, 2.0])))),
            "{0.1f32, 2.0f32}"
        );
    }
}
