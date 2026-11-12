//! 正则表达式内置函数.
//!
//! 正则由 `RegexCompile` 编译一次后反复使用; 匹配结果由 `RegexMatch` 保存.

use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_io::host;
use ksl_io::regex::{CompiledRegex, MatchRecord};
use ksl_value::value::{Value, list, option_none, option_some, result_err, result_ok};

pub(crate) fn regex_compile(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Str(pattern)) = a.first() else {
        return Err(Error::internal("RegexCompile 的实参形态不符"));
    };
    match CompiledRegex::compile(pattern) {
        Ok(compiled) => Ok(result_ok(host::regex(Arc::new(compiled)))),
        Err(error) => Ok(result_err(error.to_value())),
    }
}

pub(crate) fn regex_is_match(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let compiled = regex_arg(a, "RegexIsMatch")?;
    let Some(Value::Str(text)) = a.get(1) else {
        return Err(Error::internal("RegexIsMatch 的文本形态不符"));
    };
    Ok(Value::Bool(compiled.is_match(text)))
}

pub(crate) fn regex_find(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let compiled = regex_arg(a, "RegexFind")?;
    let Some(Value::Str(text)) = a.get(1) else {
        return Err(Error::internal("RegexFind 的文本形态不符"));
    };
    Ok(match compiled.find(text) {
        Some(record) => option_some(host::regex_match(Arc::new(record))),
        None => option_none(),
    })
}

pub(crate) fn regex_find_all(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let compiled = regex_arg(a, "RegexFindAll")?;
    let Some(Value::Str(text)) = a.get(1) else {
        return Err(Error::internal("RegexFindAll 的文本形态不符"));
    };
    Ok(list(
        compiled
            .find_all(text)
            .into_iter()
            .map(|record| host::regex_match(Arc::new(record))),
    ))
}

pub(crate) fn regex_replace(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let compiled = regex_arg(a, "RegexReplace")?;
    let Some(Value::Str(text)) = a.get(1) else {
        return Err(Error::internal("RegexReplace 的文本形态不符"));
    };
    let Some(Value::Str(replacement)) = a.get(2) else {
        return Err(Error::internal("RegexReplace 的替换串形态不符"));
    };
    Ok(Value::Str(ArcStr::from(compiled.replace(text, replacement, 1))))
}

pub(crate) fn regex_replace_all(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let compiled = regex_arg(a, "RegexReplaceAll")?;
    let Some(Value::Str(text)) = a.get(1) else {
        return Err(Error::internal("RegexReplaceAll 的文本形态不符"));
    };
    let Some(Value::Str(replacement)) = a.get(2) else {
        return Err(Error::internal("RegexReplaceAll 的替换串形态不符"));
    };
    Ok(Value::Str(ArcStr::from(compiled.replace(text, replacement, 0))))
}

pub(crate) fn regex_split(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let compiled = regex_arg(a, "RegexSplit")?;
    let Some(Value::Str(text)) = a.get(1) else {
        return Err(Error::internal("RegexSplit 的文本形态不符"));
    };
    Ok(list(
        compiled
            .split(text)
            .into_iter()
            .map(|part| Value::Str(ArcStr::from(part))),
    ))
}

pub(crate) fn regex_capture_names(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let compiled = regex_arg(a, "RegexCaptureNames")?;
    Ok(list(
        compiled
            .capture_names()
            .into_iter()
            .map(|name| Value::Str(ArcStr::from(name))),
    ))
}

/// 空匹配; 无匹配时作为占位值.
// 内置函数构造器统一返回 Result.
#[expect(clippy::unnecessary_wraps)]
pub(crate) fn regex_empty_match(_: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(host::regex_match(Arc::new(MatchRecord::empty())))
}

pub(crate) fn regex_match_text(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let record = match_arg(a, "RegexMatchText")?;
    Ok(Value::Str(ArcStr::from(record.text())))
}

pub(crate) fn regex_match_start(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let record = match_arg(a, "RegexMatchStart")?;
    Ok(Value::U64(record.start()))
}

pub(crate) fn regex_match_end(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let record = match_arg(a, "RegexMatchEnd")?;
    Ok(Value::U64(record.end()))
}

pub(crate) fn regex_match_group(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let record = match_arg(a, "RegexMatchGroup")?;
    let Some(Value::U64(index)) = a.get(1) else {
        return Err(Error::internal("RegexMatchGroup 的下标形态不符"));
    };
    Ok(match record.group(*index) {
        Some(text) => option_some(Value::Str(ArcStr::from(text))),
        None => option_none(),
    })
}

pub(crate) fn regex_match_named_group(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let record = match_arg(a, "RegexMatchNamedGroup")?;
    let Some(Value::Str(name)) = a.get(1) else {
        return Err(Error::internal("RegexMatchNamedGroup 的组名形态不符"));
    };
    Ok(match record.named_group(name) {
        Some(text) => option_some(Value::Str(ArcStr::from(text))),
        None => option_none(),
    })
}

/// 取出正则实参; 名字用于内部错误.
fn regex_arg<'a>(a: &'a [Value], name: &str) -> Result<&'a CompiledRegex, Error> {
    let Some(compiled) = a.first().and_then(host::as_regex) else {
        return Err(Error::internal(format!("{name} 的实参形态不符")));
    };
    Ok(compiled)
}

/// 取出匹配结果实参; 名字用于内部错误.
fn match_arg<'a>(a: &'a [Value], name: &str) -> Result<&'a MatchRecord, Error> {
    let Some(record) = a.first().and_then(host::as_regex_match) else {
        return Err(Error::internal(format!("{name} 的实参形态不符")));
    };
    Ok(record)
}
