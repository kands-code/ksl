//! 整数与浮点算术.

use ksl_base::source::Span;
use ksl_diagnostics::error::{ErrorKind, RuntimeError as Error};
use ksl_value::value::Value;

use super::{f32_1, f32_2, f64_1, f64_2, i32_1, i32_2, i64_1, i64_2, u8_2, u32_2, u64_2};

/// 整数二元运算的通用实现: 溢出报 ksl 级错误.
pub(crate) fn i64_bin(
    args: &[Value],
    f: fn(i64, i64) -> Option<i64>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = i64_2(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, span))?;
    Ok(Value::I64(value))
}

/// i32 二元运算的通用实现: 溢出报 ksl 级错误.
pub(crate) fn i32_bin(
    args: &[Value],
    f: fn(i32, i32) -> Option<i32>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = i32_2(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::I32Overflow, span))?;
    Ok(Value::I32(value))
}

/// u8 二元运算的通用实现.
pub(crate) fn u8_bin(
    args: &[Value],
    f: fn(u8, u8) -> Option<u8>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = u8_2(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::U8Overflow, span))?;
    Ok(Value::U8(value))
}

/// u32 二元运算的通用实现.
pub(crate) fn u32_bin(
    args: &[Value],
    f: fn(u32, u32) -> Option<u32>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = u32_2(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::U32Overflow, span))?;
    Ok(Value::U32(value))
}

/// u64 二元运算的通用实现.
pub(crate) fn u64_bin(
    args: &[Value],
    f: fn(u64, u64) -> Option<u64>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = u64_2(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::U64Overflow, span))?;
    Ok(Value::U64(value))
}

/// 向下取整除法: 商向负无穷取整.
pub(crate) fn floor_div(a: i64, b: i64) -> Option<i64> {
    let quot = a.checked_div(b)?;
    if a % b != 0 && ((a < 0) != (b < 0)) {
        quot.checked_sub(1)
    } else {
        Some(quot)
    }
}

/// 取模, 符号随除数: `a - floor_div(a, b) * b`.
pub(crate) fn floor_mod(a: i64, b: i64) -> Option<i64> {
    let div = floor_div(a, b)?;
    a.checked_sub(div.checked_mul(b)?)
}

pub(crate) fn add_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    i64_bin(a, i64::checked_add, s)
}
pub(crate) fn subtract_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    i64_bin(a, i64::checked_sub, s)
}
pub(crate) fn negate_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value = i64_1(a)?
        .checked_neg()
        .ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}
pub(crate) fn multiply_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    i64_bin(a, i64::checked_mul, s)
}
pub(crate) fn quotient_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i64_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}
pub(crate) fn divide_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i64_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = floor_div(x, y).ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}
pub(crate) fn remainder_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i64_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}
pub(crate) fn modulo_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i64_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = floor_mod(x, y).ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}

pub(crate) fn add_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    i32_bin(a, i32::checked_add, s)
}
pub(crate) fn subtract_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    i32_bin(a, i32::checked_sub, s)
}
pub(crate) fn negate_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let value = i32_1(a)?
        .checked_neg()
        .ok_or_else(|| Error::runtime(ErrorKind::I32Overflow, s))?;
    Ok(Value::I32(value))
}
pub(crate) fn multiply_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    i32_bin(a, i32::checked_mul, s)
}
pub(crate) fn quotient_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i32_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::I32Overflow, s))?;
    Ok(Value::I32(value))
}
pub(crate) fn divide_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i32_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    // 商在 i64 中计算, 再检查是否超出 i32 范围.
    let quotient = floor_div(i64::from(x), i64::from(y))
        .ok_or_else(|| Error::runtime(ErrorKind::I32Overflow, s))?;
    let value =
        i32::try_from(quotient).map_err(|_| Error::runtime(ErrorKind::I32Overflow, s))?;
    Ok(Value::I32(value))
}
pub(crate) fn remainder_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i32_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::I32Overflow, s))?;
    Ok(Value::I32(value))
}
pub(crate) fn modulo_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i32_2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = floor_mod(i64::from(x), i64::from(y))
        .ok_or_else(|| Error::runtime(ErrorKind::I32Overflow, s))?;
    let value = i32::try_from(value).expect("带号余数的绝对值小于除数");
    Ok(Value::I32(value))
}

pub(crate) fn add_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u8_bin(a, u8::checked_add, s)
}
pub(crate) fn subtract_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u8_bin(a, u8::checked_sub, s)
}
pub(crate) fn multiply_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u8_bin(a, u8::checked_mul, s)
}
pub(crate) fn divide_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u8_2(a)?;
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U8(value))
}
pub(crate) fn modulo_u8(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u8_2(a)?;
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U8(value))
}

pub(crate) fn add_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u32_bin(a, u32::checked_add, s)
}
pub(crate) fn subtract_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u32_bin(a, u32::checked_sub, s)
}
pub(crate) fn multiply_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u32_bin(a, u32::checked_mul, s)
}
pub(crate) fn divide_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u32_2(a)?;
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U32(value))
}
pub(crate) fn modulo_u32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u32_2(a)?;
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U32(value))
}

pub(crate) fn add_u64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u64_bin(a, u64::checked_add, s)
}
pub(crate) fn subtract_u64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u64_bin(a, u64::checked_sub, s)
}
pub(crate) fn multiply_u64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u64_bin(a, u64::checked_mul, s)
}
pub(crate) fn divide_u64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u64_2(a)?;
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U64(value))
}
pub(crate) fn modulo_u64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u64_2(a)?;
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U64(value))
}

pub(crate) fn add_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f64_2(a)?;
    Ok(Value::F64(x + y))
}
pub(crate) fn subtract_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f64_2(a)?;
    Ok(Value::F64(x - y))
}
pub(crate) fn negate_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = f64_1(a)?;
    Ok(Value::F64(-x))
}
pub(crate) fn multiply_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f64_2(a)?;
    Ok(Value::F64(x * y))
}
pub(crate) fn divide_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f64_2(a)?;
    Ok(Value::F64(x / y))
}
pub(crate) fn add_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f32_2(a)?;
    Ok(Value::F32(x + y))
}
pub(crate) fn subtract_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f32_2(a)?;
    Ok(Value::F32(x - y))
}
pub(crate) fn negate_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = f32_1(a)?;
    Ok(Value::F32(-x))
}
pub(crate) fn multiply_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f32_2(a)?;
    Ok(Value::F32(x * y))
}
pub(crate) fn divide_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f32_2(a)?;
    Ok(Value::F32(x / y))
}

pub(crate) fn abs_i64(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let x = i64_1(a)?;
    let value = x
        .checked_abs()
        .ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}

pub(crate) fn abs_i32(a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let x = i32_1(a)?;
    let value = x
        .checked_abs()
        .ok_or_else(|| Error::runtime(ErrorKind::I32Overflow, s))?;
    Ok(Value::I32(value))
}
