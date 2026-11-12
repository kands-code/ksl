//! 移位与位运算.

use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::Value;

use super::{i32_1, i32_2, i64_1, i64_2, u8_1, u8_2, u32_1, u32_2, u64_1, u64_2};

/// 移位位数; 形态不符即内部错误.
pub(crate) fn shift_amount(arg: Option<&Value>) -> Result<u32, Error> {
    let Some(Value::U8(amount)) = arg else {
        return Err(Error::internal("移位位数的实参形态不符"));
    };
    Ok(u32::from(*amount))
}

/// 逻辑左移: 位数达到或超过字宽时结果为 0.
pub(crate) fn shift_left_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = i64_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 64 { 0 } else { x << amount };
    Ok(Value::I64(value))
}
/// 逻辑右移: 位数达到或超过字宽时结果为 0, 高位补 0.
///
/// `i64` 直接右移是算术移位, 会补符号位; 实现先把位模式重解释为无符号数再移回.
pub(crate) fn shift_right_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = i64_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = x
        .cast_unsigned()
        .checked_shr(amount)
        .map_or(0, u64::cast_signed);
    Ok(Value::I64(value))
}
pub(crate) fn bit_or_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i64_2(a)?;
    Ok(Value::I64(x | y))
}
pub(crate) fn bit_and_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i64_2(a)?;
    Ok(Value::I64(x & y))
}
pub(crate) fn bit_xor_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i64_2(a)?;
    Ok(Value::I64(x ^ y))
}
pub(crate) fn bit_reverse_i64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::I64(i64_1(a)?.reverse_bits()))
}

pub(crate) fn shift_left_i32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = i32_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 32 { 0 } else { x << amount };
    Ok(Value::I32(value))
}
/// 逻辑右移: 位数达到或超过字宽时结果为 0, 高位补 0.
pub(crate) fn shift_right_i32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = i32_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = x
        .cast_unsigned()
        .checked_shr(amount)
        .map_or(0, u32::cast_signed);
    Ok(Value::I32(value))
}
pub(crate) fn bit_or_i32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i32_2(a)?;
    Ok(Value::I32(x | y))
}
pub(crate) fn bit_and_i32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i32_2(a)?;
    Ok(Value::I32(x & y))
}
pub(crate) fn bit_xor_i32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = i32_2(a)?;
    Ok(Value::I32(x ^ y))
}
pub(crate) fn bit_reverse_i32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::I32(i32_1(a)?.reverse_bits()))
}

pub(crate) fn shift_left_u8(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, _) = u8_2(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 8 { 0 } else { x << amount };
    Ok(Value::U8(value))
}
pub(crate) fn shift_right_u8(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, _) = u8_2(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 8 { 0 } else { x >> amount };
    Ok(Value::U8(value))
}
pub(crate) fn bit_or_u8(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u8_2(a)?;
    Ok(Value::U8(x | y))
}
pub(crate) fn bit_and_u8(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u8_2(a)?;
    Ok(Value::U8(x & y))
}
pub(crate) fn bit_xor_u8(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u8_2(a)?;
    Ok(Value::U8(x ^ y))
}
pub(crate) fn bit_reverse_u8(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U8(u8_1(a)?.reverse_bits()))
}

pub(crate) fn shift_left_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = u32_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 32 { 0 } else { x << amount };
    Ok(Value::U32(value))
}
pub(crate) fn shift_right_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = u32_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 32 { 0 } else { x >> amount };
    Ok(Value::U32(value))
}
pub(crate) fn bit_or_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u32_2(a)?;
    Ok(Value::U32(x | y))
}
pub(crate) fn bit_and_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u32_2(a)?;
    Ok(Value::U32(x & y))
}
pub(crate) fn bit_xor_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u32_2(a)?;
    Ok(Value::U32(x ^ y))
}
pub(crate) fn bit_reverse_u32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U32(u32_1(a)?.reverse_bits()))
}

pub(crate) fn shift_left_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = u64_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 64 { 0 } else { x << amount };
    Ok(Value::U64(value))
}
pub(crate) fn shift_right_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let x = u64_1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 64 { 0 } else { x >> amount };
    Ok(Value::U64(value))
}
pub(crate) fn bit_or_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u64_2(a)?;
    Ok(Value::U64(x | y))
}
pub(crate) fn bit_and_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u64_2(a)?;
    Ok(Value::U64(x & y))
}
pub(crate) fn bit_xor_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u64_2(a)?;
    Ok(Value::U64(x ^ y))
}
pub(crate) fn bit_reverse_u64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    Ok(Value::U64(u64_1(a)?.reverse_bits()))
}
