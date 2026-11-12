//! f-string 格式化引擎.
//!
//! 规则见 `docs/design.md` 第 4.3 节: 基础值, 浅层容器, Pid 与函数可格式化,
//! string 与 char 默认省略引号, `?` 显式加引号; 数值接受类型限定的
//! 格式参数; Data, Option, Result, Dict, Set, `File` 与 `DateTime` 不可打印.

use crate::builtin;
use crate::error::RuntimeError as Error;
use crate::symbol::resolve;
use crate::value::Value;

/// 宽度上限; 只影响显示.
const MAX_WIDTH: usize = 256;
/// 精度上限; f64 的有效十进制位远少于此.
const MAX_PRECISION: usize = 24;

/// 格式参数适用的值类别.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatKind {
    Int,
    Float,
    Text,
}

/// 校验格式参数是否适用于给定类别; 检查器以此为准, 与运行时同一文法.
pub(crate) fn spec_matches(kind: FormatKind, text: &str) -> bool {
    let Ok(spec) = Spec::parse(Some(text)) else {
        return false;
    };
    match kind {
        FormatKind::Int => !matches!(spec.ty, Some('e' | 'E')),
        FormatKind::Float => !spec.alternate && matches!(spec.ty, None | Some('e' | 'E')),
        FormatKind::Text => false,
    }
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
            Some(spec) => Ok(Spec::parse(Some(spec))?.apply_int(i128::from(*i), *i < 0)),
            None => Ok(i.to_string()),
        },
        Value::U8(u) => {
            let body = match spec {
                Some(spec) => Spec::parse(Some(spec))?.apply_int(i128::from(*u), false),
                None => u.to_string(),
            };
            Ok(format!("{body}u"))
        },
        Value::U64(u) => {
            let body = match spec {
                Some(spec) => Spec::parse(Some(spec))?.apply_int(i128::from(*u), false),
                None => u.to_string(),
            };
            Ok(format!("{body}U"))
        },
        Value::F64(f) => match spec {
            Some(spec) => Ok(Spec::parse(Some(spec))?.apply_float(*f)),
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
        Value::Closure(_) | Value::Builtin(_) => match spec {
            None => Ok(render_callable(value)),
            Some(_) => Err(Error::internal("函数不接受格式参数")),
        },
        Value::Data(_)
        | Value::Dict(_)
        | Value::Set(_)
        | Value::File(_)
        | Value::DateTime(_) => Err(Error::internal("该类型不可打印")),
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
        // 不可打印类型出现在这里说明检查器漏检; 用类型摘要保持输出可读.
        Value::Data(data) => format!("<{}>", resolve(data.ty)),
        Value::Nil => "nil".to_string(),
        Value::File(_) => "<File>".to_string(),
        Value::DateTime(_) => "<DateTime>".to_string(),
        Value::Pid(pid) => format!("<Pid {}>", pid.id),
        Value::Dict(_) => "<Dict>".to_string(),
        Value::Set(_) => "<Set>".to_string(),
        Value::Closure(_) | Value::Builtin(_) => render_callable(value),
    }
}

/// 函数显示: 具名函数加名字, 匿名函数只显示签名.
fn render_callable(value: &Value) -> String {
    match value {
        Value::Closure(closure) => match closure.name {
            Some(name) => format!("{} : {}", resolve(name), closure.signature),
            None => closure.signature.to_string(),
        },
        Value::Builtin(id) => {
            let symbol = crate::builtin::symbol(*id);
            match builtin::signature(symbol) {
                Some(ty) => format!("{} : {ty}", resolve(symbol)),
                None => resolve(symbol).to_string(),
            }
        },
        _ => "<函数>".to_string(),
    }
}

/// f64 默认输出保证带小数点或指数, 避免回贴到源码时变成 i64.
fn format_float_default(value: f64) -> String {
    let text = value.to_string();
    if value.is_finite() && !text.contains('.') && !text.contains('e') && !text.contains('E') {
        format!("{text}.0")
    } else {
        text
    }
}

/// 对齐方式.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Right,
    Center,
}

/// 解析后的格式参数.
#[derive(Debug, Default)]
struct Spec {
    fill: Option<char>,
    align: Option<Align>,
    sign: Option<char>,
    alternate: bool,
    zero: bool,
    width: Option<usize>,
    precision: Option<usize>,
    ty: Option<char>,
}

impl Spec {
    /// 解析 Rust 式格式参数子集: `[[fill]align][sign][#][0][width][.precision][type]`.
    fn parse(text: Option<&str>) -> Result<Self, Error> {
        let Some(text) = text else {
            return Ok(Self::default());
        };
        if text.is_empty() {
            return Err(Error::internal("格式参数为空"));
        }
        let chars: Vec<char> = text.chars().collect();
        let mut pos = 0;
        let mut spec = Self::default();
        // 填充 + 对齐: 两个字符且次位为对齐符.
        if chars.len() >= 2 && matches!(chars[1], '<' | '>' | '^') {
            spec.fill = Some(chars[0]);
            spec.align = Some(parse_align(chars[1]));
            pos = 2;
        } else if pos < chars.len() && matches!(chars[pos], '<' | '>' | '^') {
            spec.align = Some(parse_align(chars[pos]));
            pos += 1;
        }
        if pos < chars.len() && matches!(chars[pos], '+' | '-') {
            spec.sign = Some(chars[pos]);
            pos += 1;
        }
        if pos < chars.len() && chars[pos] == '#' {
            spec.alternate = true;
            pos += 1;
        }
        if pos < chars.len() && chars[pos] == '0' {
            spec.zero = true;
            pos += 1;
        }
        let width_start = pos;
        while pos < chars.len() && chars[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos > width_start {
            let digits: String = chars[width_start..pos].iter().collect();
            let width = digits
                .parse::<usize>()
                .map_err(|_| Error::internal(format!("格式宽度无法解析: {text}")))?;
            if width > MAX_WIDTH {
                return Err(Error::internal(format!("格式宽度超出上限: {text}")));
            }
            spec.width = Some(width);
        }
        if pos < chars.len() && chars[pos] == '.' {
            pos += 1;
            let precision_start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() {
                pos += 1;
            }
            let digits: String = chars[precision_start..pos].iter().collect();
            let precision = digits
                .parse::<usize>()
                .map_err(|_| Error::internal(format!("格式精度无法解析: {text}")))?;
            if precision > MAX_PRECISION {
                return Err(Error::internal(format!("格式精度超出上限: {text}")));
            }
            spec.precision = Some(precision);
        }
        if pos < chars.len() {
            spec.ty = Some(chars[pos]);
            pos += 1;
        }
        if pos != chars.len() {
            return Err(Error::internal(format!("格式参数无法解析: {text}")));
        }
        // 类型字符限定为运行时实际支持的集合; 不支持八进制.
        if !matches!(spec.ty, None | Some('x' | 'X' | 'b' | 'e' | 'E')) {
            return Err(Error::internal(format!("不支持的格式类型: {:?}", spec.ty)));
        }
        Ok(spec)
    }

    /// 渲染整数: 进制, 符号, 最少位数与宽度填充.
    fn apply_int(&self, magnitude: i128, negative: bool) -> String {
        let digits = match self.ty {
            Some('x') => format!("{:x}", magnitude.unsigned_abs()),
            Some('X') => format!("{:X}", magnitude.unsigned_abs()),
            Some('b') => format!("{:b}", magnitude.unsigned_abs()),
            _ => magnitude.unsigned_abs().to_string(),
        };
        let prefixed = if self.alternate {
            match self.ty {
                Some('x') => format!("0x{digits}"),
                Some('X') => format!("0X{digits}"),
                Some('b') => format!("0b{digits}"),
                _ => digits,
            }
        } else {
            digits
        };
        // 精度: 整数的最少位数; 进制前缀不计入位数.
        let precise = match self.precision {
            Some(precision) => {
                let (prefix, digits) = split_int_prefix(&prefixed);
                let pad = precision.saturating_sub(digits.len());
                format!("{prefix}{}{digits}", "0".repeat(pad))
            },
            None => prefixed,
        };
        let signed = match (negative, self.sign) {
            (true, _) => format!("-{precise}"),
            (false, Some('+')) => format!("+{precise}"),
            (false, _) => precise,
        };
        self.pad(signed)
    }

    /// 渲染浮点数: 精度, 指数形态与宽度填充.
    fn apply_float(&self, value: f64) -> String {
        if !value.is_finite() {
            // NaN 与无穷不参与填充, 原样输出.
            return value.to_string();
        }
        let body = match (self.ty, self.precision) {
            (Some('e'), Some(p)) => format!("{value:.p$e}"),
            (Some('E'), Some(p)) => format!("{value:.p$E}"),
            (Some('e'), None) => format!("{value:e}"),
            (Some('E'), None) => format!("{value:E}"),
            (_, Some(p)) => format!("{value:.p$}"),
            // 显式宽度或对齐仍沿用 f64 的默认形态, 例如 `1.0` 不能退化成 `1`.
            _ => format_float_default(value),
        };
        let signed = if value.is_sign_negative() && value == 0.0 && !body.starts_with('-') {
            // 负零保持负号; 显式格式可能已经写出符号, 不能重复添加.
            format!("-{body}")
        } else if self.sign == Some('+') && !body.starts_with('-') {
            format!("+{body}")
        } else {
            body
        };
        self.pad(signed)
    }

    /// 宽度填充: 零填充紧贴符号, 其余按对齐与填充符.
    fn pad(&self, body: String) -> String {
        let Some(width) = self.width else {
            return body;
        };
        if body.len() >= width {
            return body;
        }
        let deficit = width - body.len();
        if self.zero && self.fill.is_none() && self.align.is_none() {
            // 零填充在符号与进制前缀之后, 与 Rust 一致: `0x00ff` 而非 `00xff`.
            let (sign, rest) = match body.as_bytes().first() {
                Some(b'-' | b'+') => body.split_at(1),
                _ => ("", body.as_str()),
            };
            let (prefix, rest) = split_int_prefix(rest);
            return format!("{sign}{prefix}{}{rest}", "0".repeat(deficit));
        }
        let fill = self.fill.unwrap_or(' ');
        match self.align.unwrap_or(Align::Right) {
            Align::Left => format!("{body}{}", fill.to_string().repeat(deficit)),
            Align::Right => format!("{}{body}", fill.to_string().repeat(deficit)),
            Align::Center => {
                let left = deficit / 2;
                let right = deficit - left;
                format!(
                    "{}{body}{}",
                    fill.to_string().repeat(left),
                    fill.to_string().repeat(right)
                )
            },
        }
    }
}

/// 拆分整数的进制前缀; 前缀不参与精度与零填充的位数计算.
fn split_int_prefix(text: &str) -> (&str, &str) {
    if text.starts_with("0x") || text.starts_with("0X") || text.starts_with("0b") {
        text.split_at(2)
    } else {
        ("", text)
    }
}

fn parse_align(c: char) -> Align {
    match c {
        '<' => Align::Left,
        '>' => Align::Right,
        _ => Align::Center,
    }
}
