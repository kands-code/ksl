//! 数值, 位运算, 转换与比较内置函数.

use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::Value;

/// 生成单个标量实参提取器.
macro_rules! scalar_one {
    ($variant:ident, $ty:ty, $name:ident, $label:literal) => {
        /// 取单个标量实参; 形态不符即内部错误.
        pub(crate) fn $name(args: &[Value]) -> Result<$ty, Error> {
            match args.first() {
                Some(Value::$variant(value)) => Ok(*value),
                _ => Err(Error::internal(concat!($label, " 运算的实参形态不符"))),
            }
        }
    };
}

/// 生成标量实参对提取器.
macro_rules! scalar_two {
    ($variant:ident, $ty:ty, $name:ident, $label:literal) => {
        /// 取标量实参对; 形态不符即内部错误.
        pub(crate) fn $name(args: &[Value]) -> Result<($ty, $ty), Error> {
            match (args.first(), args.get(1)) {
                (Some(Value::$variant(a)), Some(Value::$variant(b))) => Ok((*a, *b)),
                _ => Err(Error::internal(concat!($label, " 运算的实参形态不符"))),
            }
        }
    };
}

scalar_one!(I64, i64, i64_1, "i64");
scalar_one!(I32, i32, i32_1, "i32");
scalar_one!(U8, u8, u8_1, "u8");
scalar_one!(U32, u32, u32_1, "u32");
scalar_one!(U64, u64, u64_1, "u64");
scalar_one!(F32, f32, f32_1, "f32");
scalar_one!(F64, f64, f64_1, "f64");
scalar_one!(Char, char, char_1, "char");

scalar_two!(I64, i64, i64_2, "i64");
scalar_two!(I32, i32, i32_2, "i32");
scalar_two!(U8, u8, u8_2, "u8");
scalar_two!(U32, u32, u32_2, "u32");
scalar_two!(U64, u64, u64_2, "u64");
scalar_two!(F32, f32, f32_2, "f32");
scalar_two!(F64, f64, f64_2, "f64");

mod arith;
mod bitwise;
mod compare;
mod convert;
mod float;

pub(crate) use arith::*;
pub(crate) use bitwise::*;
pub(crate) use compare::*;
pub(crate) use convert::*;
pub(crate) use float::*;
