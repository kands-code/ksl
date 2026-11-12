//! 相等与顺序比较.

use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::Value;

macro_rules! equal_impl {
    ($(#[$attr:meta])* $name:ident, $variant:ident) => {
        $(#[$attr])*
        pub(crate) fn $name(
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
equal_impl!(equal_u32, U32);
equal_impl!(equal_u64, U64);
equal_impl!(equal_i64, I64);
equal_impl!(equal_i32, I32);
// 浮点相等按 IEEE 语义.
equal_impl!(
    #[expect(clippy::float_cmp)]
    equal_f32,
    F32
);
equal_impl!(
    #[expect(clippy::float_cmp)]
    equal_f64,
    F64
);
equal_impl!(equal_char, Char);
equal_impl!(equal_string, Str);
equal_impl!(equal_bool, Bool);

ord_impl!(less_u8, U8, <);
ord_impl!(less_or_equal_u8, U8, <=);
ord_impl!(greater_u8, U8, >);
ord_impl!(greater_or_equal_u8, U8, >=);
ord_impl!(less_u32, U32, <);
ord_impl!(less_or_equal_u32, U32, <=);
ord_impl!(greater_u32, U32, >);
ord_impl!(greater_or_equal_u32, U32, >=);
ord_impl!(less_u64, U64, <);
ord_impl!(less_or_equal_u64, U64, <=);
ord_impl!(greater_u64, U64, >);
ord_impl!(greater_or_equal_u64, U64, >=);
ord_impl!(less_i64, I64, <);
ord_impl!(less_or_equal_i64, I64, <=);
ord_impl!(greater_i64, I64, >);
ord_impl!(greater_or_equal_i64, I64, >=);
ord_impl!(less_i32, I32, <);
ord_impl!(less_or_equal_i32, I32, <=);
ord_impl!(greater_i32, I32, >);
ord_impl!(greater_or_equal_i32, I32, >=);
ord_impl!(less_f32, F32, <);
ord_impl!(less_or_equal_f32, F32, <=);
ord_impl!(greater_f32, F32, >);
ord_impl!(greater_or_equal_f32, F32, >=);
ord_impl!(less_f64, F64, <);
ord_impl!(less_or_equal_f64, F64, <=);
ord_impl!(greater_f64, F64, >);
ord_impl!(greater_or_equal_f64, F64, >=);
