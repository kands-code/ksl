//! 求值辅助: 闭包构造与值描述.

use std::sync::Arc;

use ksl_plan::{Function as LinearFunction, Functions as LinearFunctions};
use ksl_value::value::{Capture, Closure, Value};

use crate::runtime::gc::GcRegistry;

pub(super) fn closure_linear(
    functions: &Arc<LinearFunctions>,
    function: &Arc<LinearFunction>,
    captures: Arc<[Capture]>,
    gc: &GcRegistry,
) -> Value {
    let entry = &function.blocks[function.entry.index() as usize];
    let closure = Arc::new(Closure::new(
        entry.param_count,
        Arc::clone(function),
        captures,
        Arc::clone(functions),
    ));
    gc.register_closure(&closure);
    Value::Closure(closure)
}

pub(super) fn describe(value: &Value) -> String {
    match value {
        Value::I64(_) => "i64".to_string(),
        Value::I32(_) => "i32".to_string(),
        Value::U8(_) => "u8".to_string(),
        Value::U32(_) => "u32".to_string(),
        Value::U64(_) => "u64".to_string(),
        Value::F32(_) => "f32".to_string(),
        Value::F64(_) => "f64".to_string(),
        Value::Char(_) => "char".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Str(_) => "string".to_string(),
        Value::Nil => "nil".to_string(),
        Value::List(_) => "List".to_string(),
        Value::Pair(_) => "Pair".to_string(),
        Value::Data(data) => data.ty.as_str().to_string(),
        Value::Dict(_) => "Dict".to_string(),
        Value::Set(_) => "Set".to_string(),
        Value::Closure(_) | Value::Builtin(_) | Value::ForeignFn(_) => "Fn".to_string(),
        Value::Pid(_) => "Pid".to_string(),
        Value::NativeOpaque(value) => value.type_name().to_string(),
    }
}
