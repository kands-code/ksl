//! Data 值及内置构造器.

use std::sync::Arc;

use ksl_base::symbol::{Symbol, keywords};

use super::Value;

/// Data 值: 类型名, 变体名与字段.
#[derive(Debug)]
pub struct DataValue {
    pub ty: Symbol,
    pub variant: Symbol,
    pub fields: Box<[Value]>,
}

/// 构造内置 Data 值; 字段接受数组或 `Vec`, 直接转为 boxed slice.
#[must_use]
pub(crate) fn data_value(
    ty: Symbol,
    variant: Symbol,
    fields: impl Into<Box<[Value]>>,
) -> Value {
    Value::Data(Arc::new(DataValue {
        ty,
        variant,
        fields: fields.into(),
    }))
}

/// 构造无载荷的内置 Data 值; 空 boxed slice 不分配.
#[must_use]
pub(crate) fn data_value0(ty: Symbol, variant: Symbol) -> Value {
    data_value(ty, variant, Box::<[Value]>::default())
}

#[must_use]
pub(crate) fn option_some(value: Value) -> Value {
    let names = keywords();
    data_value(names.option_, names.some_, [value])
}

#[must_use]
pub(crate) fn option_none() -> Value {
    let names = keywords();
    data_value0(names.option_, names.none_)
}

#[must_use]
pub(crate) fn result_ok(value: Value) -> Value {
    let names = keywords();
    data_value(names.result_, names.ok_, [value])
}

#[must_use]
pub(crate) fn result_err(error: Value) -> Value {
    let names = keywords();
    data_value(names.result_, names.err_, [error])
}
