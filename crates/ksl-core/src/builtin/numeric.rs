//! 数值, 位运算, 转换与比较内置函数.

use crate::error::{ErrorKind, RuntimeError as Error};
use crate::source::Span;
use crate::value::Value;

pub(crate) fn int1(args: &[Value]) -> Result<i64, Error> {
    match args.first() {
        Some(Value::I64(a)) => Ok(*a),
        _ => Err(Error::internal("整数运算的实参形态不符")),
    }
}

/// 取整数实参对; 形态不符即内部错误.
pub(crate) fn int2(args: &[Value]) -> Result<(i64, i64), Error> {
    match (args.first(), args.get(1)) {
        (Some(Value::I64(a)), Some(Value::I64(b))) => Ok((*a, *b)),
        _ => Err(Error::internal("整数运算的实参形态不符")),
    }
}

/// 取 u8 实参对; 形态不符即内部错误.
pub(crate) fn u82(args: &[Value]) -> Result<(u8, u8), Error> {
    match (args.first(), args.get(1)) {
        (Some(Value::U8(a)), Some(Value::U8(b))) => Ok((*a, *b)),
        _ => Err(Error::internal("u8 运算的实参形态不符")),
    }
}

/// 取 u64 实参对; 形态不符即内部错误.
pub(crate) fn args_u64(args: &[Value]) -> Result<(u64, u64), Error> {
    match (args.first(), args.get(1)) {
        (Some(Value::U64(a)), Some(Value::U64(b))) => Ok((*a, *b)),
        _ => Err(Error::internal("u64 运算的实参形态不符")),
    }
}

/// 取 f64 实参对; 形态不符即内部错误.
pub(crate) fn f642(args: &[Value]) -> Result<(f64, f64), Error> {
    match (args.first(), args.get(1)) {
        (Some(Value::F64(a)), Some(Value::F64(b))) => Ok((*a, *b)),
        _ => Err(Error::internal("f64 运算的实参形态不符")),
    }
}

/// 取单个 f64 实参; 形态不符即内部错误.
pub(crate) fn f641(args: &[Value]) -> Result<f64, Error> {
    match args.first() {
        Some(Value::F64(a)) => Ok(*a),
        _ => Err(Error::internal("f64 运算的实参形态不符")),
    }
}

/// 整数二元运算的骨架: 溢出报 ksl 级错误.
pub(crate) fn int_bin(
    args: &[Value],
    f: fn(i64, i64) -> Option<i64>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = int2(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, span))?;
    Ok(Value::I64(value))
}

/// u8 二元运算的骨架.
pub(crate) fn u8_bin(
    args: &[Value],
    f: fn(u8, u8) -> Option<u8>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = u82(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::U8Overflow, span))?;
    Ok(Value::U8(value))
}

/// u64 二元运算的骨架.
pub(crate) fn u64_bin(
    args: &[Value],
    f: fn(u64, u64) -> Option<u64>,
    span: Option<Span>,
) -> Result<Value, Error> {
    let (a, b) = args_u64(args)?;
    let value = f(a, b).ok_or_else(|| Error::runtime(ErrorKind::U64Overflow, span))?;
    Ok(Value::U64(value))
}

/// 向下取整除法: 商向负无穷取整, 与 Haskell 语义一致.
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

pub(crate) fn add_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    int_bin(a, i64::checked_add, s)
}
pub(crate) fn subtract_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    int_bin(a, i64::checked_sub, s)
}
pub(crate) fn multiply_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    int_bin(a, i64::checked_mul, s)
}
pub(crate) fn quotient_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = int2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}
pub(crate) fn divide_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = int2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = floor_div(x, y).ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}
pub(crate) fn remainder_i64(
    _: &[String],
    a: &[Value],
    s: Option<Span>,
) -> Result<Value, Error> {
    let (x, y) = int2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}
pub(crate) fn modulo_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = int2(a)?;
    if y == 0 {
        return Err(Error::runtime(ErrorKind::DivisionByZero, s));
    }
    let value = floor_mod(x, y).ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}

pub(crate) fn add_u8(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u8_bin(a, u8::checked_add, s)
}
pub(crate) fn subtract_u8(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u8_bin(a, u8::checked_sub, s)
}
pub(crate) fn multiply_u8(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u8_bin(a, u8::checked_mul, s)
}
pub(crate) fn divide_u8(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u82(a)?;
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U8(value))
}
pub(crate) fn modulo_u8(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u82(a)?;
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U8(value))
}

pub(crate) fn add_u64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u64_bin(a, u64::checked_add, s)
}
pub(crate) fn subtract_u64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u64_bin(a, u64::checked_sub, s)
}
pub(crate) fn multiply_u64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    u64_bin(a, u64::checked_mul, s)
}
pub(crate) fn divide_u64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = args_u64(a)?;
    let value = x
        .checked_div(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U64(value))
}
pub(crate) fn modulo_u64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let (x, y) = args_u64(a)?;
    let value = x
        .checked_rem(y)
        .ok_or_else(|| Error::runtime(ErrorKind::DivisionByZero, s))?;
    Ok(Value::U64(value))
}

pub(crate) fn add_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f642(a)?;
    Ok(Value::F64(x + y))
}
pub(crate) fn subtract_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f642(a)?;
    Ok(Value::F64(x - y))
}
pub(crate) fn multiply_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f642(a)?;
    Ok(Value::F64(x * y))
}
pub(crate) fn divide_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f642(a)?;
    Ok(Value::F64(x / y))
}

pub(crate) fn abs_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let x = int1(a)?;
    let value = x
        .checked_abs()
        .ok_or_else(|| Error::runtime(ErrorKind::IntegerOverflow, s))?;
    Ok(Value::I64(value))
}

pub(crate) fn abs_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::abs)
}

/// f64 一元函数骨架.
pub(crate) fn f64_unary(a: &[Value], f: fn(f64) -> f64) -> Result<Value, Error> {
    Ok(Value::F64(f(f641(a)?)))
}

pub(crate) fn power(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (base, exponent) = f642(a)?;
    Ok(Value::F64(base.powf(exponent)))
}

pub(crate) fn sqrt(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::sqrt)
}

pub(crate) fn sin(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::sin)
}

pub(crate) fn cos(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::cos)
}

pub(crate) fn tan(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::tan)
}

pub(crate) fn arc_sin(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::asin)
}

pub(crate) fn arc_cos(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::acos)
}

pub(crate) fn arc_tan(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::atan)
}

/// 两参数反正切: 首参为 x, 次参为 y; 等价于常规 atan2(y, x).
pub(crate) fn arc_tan2(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f642(a)?;
    Ok(Value::F64(y.atan2(x)))
}

pub(crate) fn sinh(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::sinh)
}

pub(crate) fn cosh(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::cosh)
}

pub(crate) fn tanh(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::tanh)
}

pub(crate) fn arc_sinh(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::asinh)
}

pub(crate) fn arc_cosh(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::acosh)
}

pub(crate) fn arc_tanh(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::atanh)
}

/// 移位位数; 形态不符即内部错误.
pub(crate) fn shift_amount(arg: Option<&Value>) -> Result<u32, Error> {
    let Some(Value::U8(amount)) = arg else {
        return Err(Error::internal("移位位数的实参形态不符"));
    };
    Ok(u32::from(*amount))
}

/// 逻辑左移: 位数达到或超过字宽时结果为 0, 与右移对称, 移位满足结合律.
pub(crate) fn shift_left_i64(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let x = int1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 64 { 0 } else { x << amount };
    Ok(Value::I64(value))
}
/// 逻辑右移: 位数达到或超过字宽时结果为 0, 高位补 0 且满足结合律.
///
/// 实现上把位模式重解释为无符号数再移回; i64 直接右移是算术移位, 高位补符号.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
pub(crate) fn shift_right_i64(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let x = int1(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 64 {
        0
    } else {
        ((x as u64) >> amount) as i64
    };
    Ok(Value::I64(value))
}
pub(crate) fn bit_or_i64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = int2(a)?;
    Ok(Value::I64(x | y))
}
pub(crate) fn bit_and_i64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = int2(a)?;
    Ok(Value::I64(x & y))
}
pub(crate) fn bit_xor_i64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = int2(a)?;
    Ok(Value::I64(x ^ y))
}
pub(crate) fn bit_reverse_i64(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::I64(x)) = a.first() else {
        return Err(Error::internal("位反转的实参形态不符"));
    };
    Ok(Value::I64(x.reverse_bits()))
}

pub(crate) fn shift_left_u8(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let (x, _) = u82(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 8 { 0 } else { x << amount };
    Ok(Value::U8(value))
}
pub(crate) fn shift_right_u8(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let (x, _) = u82(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 8 { 0 } else { x >> amount };
    Ok(Value::U8(value))
}
pub(crate) fn bit_or_u8(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u82(a)?;
    Ok(Value::U8(x | y))
}
pub(crate) fn bit_and_u8(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u82(a)?;
    Ok(Value::U8(x & y))
}
pub(crate) fn bit_xor_u8(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = u82(a)?;
    Ok(Value::U8(x ^ y))
}
pub(crate) fn bit_reverse_u8(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::U8(x)) = a.first() else {
        return Err(Error::internal("位反转的实参形态不符"));
    };
    Ok(Value::U8(x.reverse_bits()))
}

pub(crate) fn shift_left_u64(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let (x, _) = args_u64(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 64 { 0 } else { x << amount };
    Ok(Value::U64(value))
}
pub(crate) fn shift_right_u64(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let (x, _) = args_u64(a)?;
    let amount = shift_amount(a.get(1))?;
    let value = if amount >= 64 { 0 } else { x >> amount };
    Ok(Value::U64(value))
}
pub(crate) fn bit_or_u64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = args_u64(a)?;
    Ok(Value::U64(x | y))
}
pub(crate) fn bit_and_u64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = args_u64(a)?;
    Ok(Value::U64(x & y))
}
pub(crate) fn bit_xor_u64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = args_u64(a)?;
    Ok(Value::U64(x ^ y))
}
pub(crate) fn bit_reverse_u64(
    _: &[String],
    a: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::U64(x)) = a.first() else {
        return Err(Error::internal("位反转的实参形态不符"));
    };
    Ok(Value::U64(x.reverse_bits()))
}

/// i64 到 f64 的转换允许精度损失.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn i64_to_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::I64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    Ok(Value::F64(*x as f64))
}

/// f64 到 i64 的骨架: 超出可表示范围报 ksl 级错误.
///
/// 上界取 2^63 而非 `i64::MAX`, 后者无法用 f64 精确表示; 范围已显式校验,
/// 转换不会发生截断.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub(crate) fn float_to_int(
    a: &[Value],
    f: fn(f64) -> f64,
    span: Option<Span>,
) -> Result<Value, Error> {
    let Some(Value::F64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    let rounded = f(*x);
    let min = -9_223_372_036_854_775_808.0_f64;
    let max_exclusive = 9_223_372_036_854_775_808.0_f64;
    if !rounded.is_finite() || rounded < min || rounded >= max_exclusive {
        return Err(Error::runtime(ErrorKind::FloatOutOfRange, span));
    }
    Ok(Value::I64(rounded as i64))
}

pub(crate) fn truncate(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::trunc, s)
}
/// 银行家舍入: 平局取偶.
pub(crate) fn round(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::round_ties_even, s)
}
pub(crate) fn ceiling(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::ceil, s)
}
pub(crate) fn floor(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    float_to_int(a, f64::floor, s)
}

pub(crate) fn u8_to_i64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::U8(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    Ok(Value::I64(i64::from(*x)))
}
pub(crate) fn i64_to_u8(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let Some(Value::I64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    let value =
        u8::try_from(*x).map_err(|_| Error::runtime(ErrorKind::IntConversionOutOfRange, s))?;
    Ok(Value::U8(value))
}
pub(crate) fn u8_to_u64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::U8(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    Ok(Value::U64(u64::from(*x)))
}
pub(crate) fn u64_to_u8(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let Some(Value::U64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    let value =
        u8::try_from(*x).map_err(|_| Error::runtime(ErrorKind::IntConversionOutOfRange, s))?;
    Ok(Value::U8(value))
}
pub(crate) fn i64_to_u64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let Some(Value::I64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    let value =
        u64::try_from(*x).map_err(|_| Error::runtime(ErrorKind::IntConversionOutOfRange, s))?;
    Ok(Value::U64(value))
}
pub(crate) fn u64_to_i64(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let Some(Value::U64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    let value =
        i64::try_from(*x).map_err(|_| Error::runtime(ErrorKind::IntConversionOutOfRange, s))?;
    Ok(Value::I64(value))
}
/// u64 到 f64 的转换允许精度损失.
#[allow(clippy::cast_precision_loss)]
pub(crate) fn u64_to_f64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::U64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    Ok(Value::F64(*x as f64))
}
pub(crate) fn char_to_i64(_: &[String], a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let Some(Value::Char(c)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    Ok(Value::I64(i64::from(u32::from(*c))))
}
pub(crate) fn i64_to_char(_: &[String], a: &[Value], s: Option<Span>) -> Result<Value, Error> {
    let Some(Value::I64(x)) = a.first() else {
        return Err(Error::internal("转换的实参形态不符"));
    };
    let scalar = u32::try_from(*x).ok().and_then(char::from_u32);
    let Some(c) = scalar else {
        return Err(Error::runtime(ErrorKind::InvalidCharScalar, s));
    };
    Ok(Value::Char(c))
}

macro_rules! equal_impl {
    ($name:ident, $variant:ident) => {
        #[allow(clippy::float_cmp)]
        pub(crate) fn $name(
            _: &[String],
            a: &[Value],
            _: Option<Span>,
        ) -> Result<Value, Error> {
            let (Some(Value::$variant(x)), Some(Value::$variant(y))) = (a.first(), a.get(1))
            else {
                return Err(Error::internal("相等性函数的实参形态不符"));
            };
            Ok(Value::Bool(x == y))
        }
    };
}

macro_rules! ord_impl {
    ($name:ident, $variant:ident, $op:tt) => {
        pub(crate) fn $name(
            _: &[String],
            a: &[Value],
            _: Option<Span>,
        ) -> Result<Value, Error> {
            let (Some(Value::$variant(x)), Some(Value::$variant(y))) = (a.first(), a.get(1))
            else {
                return Err(Error::internal("比较函数的实参形态不符"));
            };
            Ok(Value::Bool(x $op y))
        }
    };
}

equal_impl!(equal_u8, U8);
equal_impl!(equal_u64, U64);
equal_impl!(equal_i64, I64);
equal_impl!(equal_f64, F64);
equal_impl!(equal_char, Char);
equal_impl!(equal_string, Str);
equal_impl!(equal_bool, Bool);

ord_impl!(less_u8, U8, <);
ord_impl!(less_or_equal_u8, U8, <=);
ord_impl!(greater_u8, U8, >);
ord_impl!(greater_or_equal_u8, U8, >=);
ord_impl!(less_u64, U64, <);
ord_impl!(less_or_equal_u64, U64, <=);
ord_impl!(greater_u64, U64, >);
ord_impl!(greater_or_equal_u64, U64, >=);
ord_impl!(less_i64, I64, <);
ord_impl!(less_or_equal_i64, I64, <=);
ord_impl!(greater_i64, I64, >);
ord_impl!(greater_or_equal_i64, I64, >=);
ord_impl!(less_f64, F64, <);
ord_impl!(less_or_equal_f64, F64, <=);
ord_impl!(greater_f64, F64, >);
ord_impl!(greater_or_equal_f64, F64, >=);
