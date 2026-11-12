//! 浮点一元函数与数学函数.

use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::Value;

use super::{f32_1, f32_2, f64_1, f64_2};

/// f64 一元函数的通用实现.
pub(crate) fn f64_unary(a: &[Value], f: fn(f64) -> f64) -> Result<Value, Error> {
    Ok(Value::F64(f(f64_1(a)?)))
}

/// f32 一元函数的通用实现.
pub(crate) fn f32_unary(a: &[Value], f: fn(f32) -> f32) -> Result<Value, Error> {
    Ok(Value::F32(f(f32_1(a)?)))
}

pub(crate) fn abs_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::abs)
}

pub(crate) fn abs_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::abs)
}

pub(crate) fn power_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (base, exponent) = f64_2(a)?;
    Ok(Value::F64(base.powf(exponent)))
}

pub(crate) fn power_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (base, exponent) = f32_2(a)?;
    Ok(Value::F32(base.powf(exponent)))
}

pub(crate) fn sqrt_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::sqrt)
}

pub(crate) fn sqrt_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::sqrt)
}

pub(crate) fn sin_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::sin)
}

pub(crate) fn sin_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::sin)
}

pub(crate) fn cos_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::cos)
}

pub(crate) fn cos_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::cos)
}

pub(crate) fn tan_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::tan)
}

pub(crate) fn tan_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::tan)
}

pub(crate) fn arc_sin_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::asin)
}

pub(crate) fn arc_sin_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::asin)
}

pub(crate) fn arc_cos_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::acos)
}

pub(crate) fn arc_cos_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::acos)
}

pub(crate) fn arc_tan_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::atan)
}

pub(crate) fn arc_tan_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::atan)
}

/// 两参数反正切: 首参为 x, 次参为 y; 等价于常规 atan2(y, x).
pub(crate) fn arc_tan2_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f64_2(a)?;
    Ok(Value::F64(y.atan2(x)))
}

pub(crate) fn arc_tan2_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    let (x, y) = f32_2(a)?;
    Ok(Value::F32(y.atan2(x)))
}

pub(crate) fn sinh_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::sinh)
}

pub(crate) fn sinh_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::sinh)
}

pub(crate) fn cosh_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::cosh)
}

pub(crate) fn cosh_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::cosh)
}

pub(crate) fn tanh_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::tanh)
}

pub(crate) fn tanh_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::tanh)
}

pub(crate) fn arc_sinh_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::asinh)
}

pub(crate) fn arc_sinh_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::asinh)
}

pub(crate) fn arc_cosh_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::acosh)
}

pub(crate) fn arc_cosh_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::acosh)
}

pub(crate) fn arc_tanh_f64(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f64_unary(a, f64::atanh)
}

pub(crate) fn arc_tanh_f32(a: &[Value], _: Option<Span>) -> Result<Value, Error> {
    f32_unary(a, f32::atanh)
}
