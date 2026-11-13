//! 字符串与解析内置函数.

use std::sync::Arc;

use crate::error::RuntimeError as Error;
use crate::source::Span;
use crate::value::{List, Value, list, result_err_str, result_ok};

pub(crate) fn string_to_chars(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringToChars 的实参形态不符"));
    };
    Ok(Value::List(Arc::new(List::chars(text.chars()))))
}

pub(crate) fn chars_to_string(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::List(list)) = a.first() else {
        return Err(Error::internal("CharsToString 的实参形态不符"));
    };
    let mut text = String::new();
    match &**list {
        List::Char(items) => text.extend(items.iter()),
        List::Values(items) => {
            for c in items {
                let Value::Char(c) = c else {
                    return Err(Error::internal("CharsToString 的元素形态不符"));
                };
                text.push(*c);
            }
        },
        List::I64(_) | List::U8(_) | List::U64(_) | List::F64(_) | List::Bool(_) => {
            return Err(Error::internal("CharsToString 的实参不是字符列表"));
        },
    }
    Ok(Value::Str(Arc::from(text)))
}

pub(crate) fn string_length(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringLength 的实参形态不符"));
    };
    let length = u64::try_from(text.chars().count())
        .map_err(|_| Error::internal("字符串长度超出 u64 上限"))?;
    Ok(Value::U64(length))
}

/// 字符谓词的公共形态检查; 名字只用于内部错误.
fn char_predicate(
    name: &str,
    a: &[Value],
    predicate: fn(char) -> bool,
) -> Result<Value, Error> {
    let Some(Value::Char(c)) = a.first() else {
        return Err(Error::internal(format!("{name} 的实参形态不符")));
    };
    Ok(Value::Bool(predicate(*c)))
}

pub(crate) fn is_whitespace(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    char_predicate("IsWhitespace", a, char::is_whitespace)
}

pub(crate) fn is_ascii_digit(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    char_predicate("IsAsciiDigit", a, |c| c.is_ascii_digit())
}

pub(crate) fn is_letter(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    char_predicate("IsLetter", a, char::is_alphabetic)
}

pub(crate) fn is_uppercase(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    char_predicate("IsUppercase", a, char::is_uppercase)
}

pub(crate) fn is_lowercase(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    char_predicate("IsLowercase", a, char::is_lowercase)
}

pub(crate) fn is_alphanumeric(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    char_predicate("IsAlphanumeric", a, char::is_alphanumeric)
}

pub(crate) fn string_split(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringSplit 的实参形态不符"));
    };
    let Some(Value::Str(separator)) = a.get(1) else {
        return Err(Error::internal("StringSplit 的分隔符形态不符"));
    };
    if separator.is_empty() {
        return Ok(list([Value::Str(Arc::clone(text))]));
    }
    Ok(list(
        text.split(&**separator).map(|part| Value::Str(Arc::from(part))),
    ))
}

pub(crate) fn string_lines(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringLines 的实参形态不符"));
    };
    if text.is_empty() {
        return Ok(list(Vec::<Value>::new()));
    }
    let mut lines: Vec<Value> = text
        .split('\n')
        .map(|line| {
            let line = line.trim_suffix('\r');
            Value::Str(Arc::from(line))
        })
        .collect();
    if text.ends_with('\n') {
        lines.pop();
    }
    Ok(list(lines))
}

pub(crate) fn string_words(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringWords 的实参形态不符"));
    };
    Ok(list(
        text.split_whitespace().map(|word| Value::Str(Arc::from(word))),
    ))
}

pub(crate) fn string_trim(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringTrim 的实参形态不符"));
    };
    Ok(Value::Str(Arc::from(text.trim())))
}

pub(crate) fn string_trim_start(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringTrimStart 的实参形态不符"));
    };
    Ok(Value::Str(Arc::from(text.trim_start())))
}

pub(crate) fn string_trim_end(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringTrimEnd 的实参形态不符"));
    };
    Ok(Value::Str(Arc::from(text.trim_end())))
}

pub(crate) fn string_starts_with(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let (text, prefix) = string_pair(a, "StringStartsWith")?;
    Ok(Value::Bool(text.starts_with(prefix)))
}

pub(crate) fn string_ends_with(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let (text, suffix) = string_pair(a, "StringEndsWith")?;
    Ok(Value::Bool(text.ends_with(suffix)))
}

pub(crate) fn string_contains(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let (text, needle) = string_pair(a, "StringContains")?;
    Ok(Value::Bool(text.contains(needle)))
}

pub(crate) fn string_pair<'a>(a: &'a [Value], name: &str) -> Result<(&'a str, &'a str), Error> {
    let (Some(Value::Str(text)), Some(Value::Str(other))) = (a.first(), a.get(1)) else {
        return Err(Error::internal(format!("{name} 的实参形态不符")));
    };
    Ok((text, other))
}

pub(crate) fn string_uppercase(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringUppercase 的实参形态不符"));
    };
    Ok(Value::Str(Arc::from(text.to_uppercase())))
}

pub(crate) fn string_lowercase(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("StringLowercase 的实参形态不符"));
    };
    Ok(Value::Str(Arc::from(text.to_lowercase())))
}

pub(crate) fn string_join(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::List(list)) = a.first() else {
        return Err(Error::internal("StringJoin 的实参形态不符"));
    };
    let List::Values(items) = &**list else {
        return Err(Error::internal("StringJoin 的实参不是字符串列表"));
    };
    let mut text = String::new();
    for item in items {
        let Value::Str(piece) = item else {
            return Err(Error::internal("StringJoin 的元素形态不符"));
        };
        text.push_str(piece);
    }
    Ok(Value::Str(Arc::from(text)))
}

pub(crate) fn encode_utf8(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("EncodeUtf8 的实参形态不符"));
    };
    Ok(Value::List(Arc::new(List::u8s(
        text.as_bytes().iter().copied(),
    ))))
}

pub(crate) fn decode_utf8(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::List(list)) = a.first() else {
        return Err(Error::internal("DecodeUtf8 的实参形态不符"));
    };
    let bytes: Vec<u8> = match &**list {
        List::U8(items) => items.iter().copied().collect(),
        List::Values(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                let Value::U8(byte) = item else {
                    return Err(Error::internal("DecodeUtf8 的字节形态不符"));
                };
                bytes.push(*byte);
            }
            bytes
        },
        List::I64(_) | List::U64(_) | List::F64(_) | List::Char(_) | List::Bool(_) => {
            return Err(Error::internal("DecodeUtf8 的实参不是字节列表"));
        },
    };
    match std::str::from_utf8(&bytes) {
        Ok(text) => Ok(result_ok(Value::Str(Arc::from(text)))),
        Err(err) => Ok(result_err_str(err.to_string())),
    }
}

pub(crate) fn parse_i64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("ParseI64 的实参形态不符"));
    };
    match text.parse::<i64>() {
        Ok(value) => Ok(result_ok(Value::I64(value))),
        Err(err) => Ok(result_err_str(err.to_string())),
    }
}

pub(crate) fn parse_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("ParseF64 的实参形态不符"));
    };
    match text.parse::<f64>() {
        Ok(value) => Ok(result_ok(Value::F64(value))),
        Err(err) => Ok(result_err_str(err.to_string())),
    }
}

pub(crate) fn parse_u64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(text)) = a.first() else {
        return Err(Error::internal("ParseU64 的实参形态不符"));
    };
    match text.parse::<u64>() {
        Ok(value) => Ok(result_ok(Value::U64(value))),
        Err(err) => Ok(result_err_str(err.to_string())),
    }
}
