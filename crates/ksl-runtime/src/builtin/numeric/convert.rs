//! 标量类型转换.

use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, RuntimeError as Error};
use ksl_value::value::Value;

use super::{char_1, f32_1, f64_1, i32_1, i64_1, u8_1, u32_1, u64_1};

/// 浮点转换超出目标整数范围.
fn float_out_of_range(source: Type, target: Type, span: Option<Span>) -> Error {
    Error::runtime(ErrorKind::FloatOutOfRange { source, target }, span)
}

/// 整数转换超出目标类型范围.
fn int_out_of_range(source: Type, target: Type, span: Option<Span>) -> Error {
    Error::runtime(ErrorKind::IntConversionOutOfRange { source, target }, span)
}

/// i64 到 f64 的转换允许精度损失.
#[expect(clippy::cast_precision_loss)]
pub(crate) fn i64_to_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F64(i64_1(a)? as f64))
}

/// f64 到 i64 的转换: 超出可表示范围报 ksl 级错误.
///
/// 上界取 2^63; `i64::MAX` 无法用 f64 精确表示. 范围已显式校验,
/// 转换不会发生截断.
#[expect(clippy::cast_possible_truncation)]
pub(crate) fn float_to_int(
    a: &[Value],
    f: fn(f64) -> f64,
    span: Option<Span>,
) -> Result<Value, Error> {
    let rounded = f(f64_1(a)?);
    let min = -9_223_372_036_854_775_808.0_f64;
    let max_exclusive = 9_223_372_036_854_775_808.0_f64;
    if !rounded.is_finite() || rounded < min || rounded >= max_exclusive {
        return Err(float_out_of_range(Type::F64, Type::I64, span));
    }
    Ok(Value::I64(rounded as i64))
}

/// f32 到 i64 的转换: 先按给定函数取整, 再检查范围.
#[expect(clippy::cast_possible_truncation)]
pub(crate) fn float32_to_int(
    a: &[Value],
    f: fn(f32) -> f32,
    span: Option<Span>,
) -> Result<Value, Error> {
    let rounded = f64::from(f(f32_1(a)?));
    let min = -9_223_372_036_854_775_808.0_f64;
    let max_exclusive = 9_223_372_036_854_775_808.0_f64;
    if !rounded.is_finite() || rounded < min || rounded >= max_exclusive {
        return Err(float_out_of_range(Type::F32, Type::I64, span));
    }
    Ok(Value::I64(rounded as i64))
}

pub(crate) fn truncate_f64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::trunc, s)
}
/// 银行家舍入: 平局取偶.
pub(crate) fn round_f64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::round_ties_even, s)
}
pub(crate) fn ceiling_f64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::ceil, s)
}
pub(crate) fn floor_f64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::floor, s)
}
pub(crate) fn truncate_f32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float32_to_int(a, f32::trunc, s)
}
pub(crate) fn round_f32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float32_to_int(a, f32::round_ties_even, s)
}
pub(crate) fn ceiling_f32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float32_to_int(a, f32::ceil, s)
}
pub(crate) fn floor_f32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float32_to_int(a, f32::floor, s)
}

pub(crate) fn u8_to_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::I64(i64::from(u8_1(a)?)))
}
pub(crate) fn i64_to_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u8::try_from(i64_1(a)?).map_err(|_| int_out_of_range(Type::I64, Type::U8, s))?;
    Ok(Value::U8(value))
}
pub(crate) fn u8_to_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U64(u64::from(u8_1(a)?)))
}
pub(crate) fn u32_to_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::I64(i64::from(u32_1(a)?)))
}
pub(crate) fn i64_to_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u32::try_from(i64_1(a)?).map_err(|_| int_out_of_range(Type::I64, Type::U32, s))?;
    Ok(Value::U32(value))
}
pub(crate) fn u32_to_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        i32::try_from(u32_1(a)?).map_err(|_| int_out_of_range(Type::U32, Type::I32, s))?;
    Ok(Value::I32(value))
}
pub(crate) fn i32_to_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u32::try_from(i32_1(a)?).map_err(|_| int_out_of_range(Type::I32, Type::U32, s))?;
    Ok(Value::U32(value))
}
pub(crate) fn u32_to_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U64(u64::from(u32_1(a)?)))
}
pub(crate) fn u64_to_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u32::try_from(u64_1(a)?).map_err(|_| int_out_of_range(Type::U64, Type::U32, s))?;
    Ok(Value::U32(value))
}
pub(crate) fn u32_to_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u8::try_from(u32_1(a)?).map_err(|_| int_out_of_range(Type::U32, Type::U8, s))?;
    Ok(Value::U8(value))
}
pub(crate) fn u8_to_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U32(u32::from(u8_1(a)?)))
}
/// u32 到 f64 的转换无精度损失.
pub(crate) fn u32_to_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F64(f64::from(u32_1(a)?)))
}
/// u32 到 f32 的转换允许精度损失.
#[expect(clippy::cast_precision_loss)]
pub(crate) fn u32_to_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F32(u32_1(a)? as f32))
}
pub(crate) fn char_to_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U32(u32::from(char_1(a)?)))
}
pub(crate) fn u32_to_char(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let scalar = char::from_u32(u32_1(a)?);
    let Some(c) = scalar else {
        return Err(Error::runtime(ErrorKind::InvalidCharScalar, s));
    };
    Ok(Value::Char(c))
}

pub(crate) fn u64_to_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u8::try_from(u64_1(a)?).map_err(|_| int_out_of_range(Type::U64, Type::U8, s))?;
    Ok(Value::U8(value))
}
pub(crate) fn i64_to_u64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u64::try_from(i64_1(a)?).map_err(|_| int_out_of_range(Type::I64, Type::U64, s))?;
    Ok(Value::U64(value))
}
pub(crate) fn u64_to_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        i64::try_from(u64_1(a)?).map_err(|_| int_out_of_range(Type::U64, Type::I64, s))?;
    Ok(Value::I64(value))
}
/// u64 到 f64 的转换允许精度损失.
#[expect(clippy::cast_precision_loss)]
pub(crate) fn u64_to_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F64(u64_1(a)? as f64))
}

/// i64 到 f32 的转换允许精度损失.
#[expect(clippy::cast_precision_loss)]
pub(crate) fn i64_to_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F32(i64_1(a)? as f32))
}

/// u64 到 f32 的转换允许精度损失.
#[expect(clippy::cast_precision_loss)]
pub(crate) fn u64_to_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F32(u64_1(a)? as f32))
}

pub(crate) fn i32_to_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::I64(i64::from(i32_1(a)?)))
}

pub(crate) fn i64_to_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        i32::try_from(i64_1(a)?).map_err(|_| int_out_of_range(Type::I64, Type::I32, s))?;
    Ok(Value::I32(value))
}

pub(crate) fn i32_to_u64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u64::try_from(i32_1(a)?).map_err(|_| int_out_of_range(Type::I32, Type::U64, s))?;
    Ok(Value::U64(value))
}

pub(crate) fn u64_to_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        i32::try_from(u64_1(a)?).map_err(|_| int_out_of_range(Type::U64, Type::I32, s))?;
    Ok(Value::I32(value))
}

pub(crate) fn u8_to_i32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::I32(i32::from(u8_1(a)?)))
}

pub(crate) fn i32_to_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value =
        u8::try_from(i32_1(a)?).map_err(|_| int_out_of_range(Type::I32, Type::U8, s))?;
    Ok(Value::U8(value))
}

/// i32 到 f32 的转换允许精度损失.
#[expect(clippy::cast_precision_loss)]
pub(crate) fn i32_to_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F32(i32_1(a)? as f32))
}

pub(crate) fn i32_to_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F64(f64::from(i32_1(a)?)))
}

pub(crate) fn f32_to_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F64(f64::from(f32_1(a)?)))
}

/// f64 到 f32 的转换按 IEEE 754 舍入, 溢出得无穷.
#[expect(clippy::cast_possible_truncation)]
pub(crate) fn f64_to_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::F32(f64_1(a)? as f32))
}
pub(crate) fn char_to_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U64(u64::from(u32::from(char_1(a)?))))
}
pub(crate) fn u64_to_char(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let scalar = u32::try_from(u64_1(a)?).ok().and_then(char::from_u32);
    let Some(c) = scalar else {
        return Err(Error::runtime(ErrorKind::InvalidCharScalar, s));
    };
    Ok(Value::Char(c))
}
