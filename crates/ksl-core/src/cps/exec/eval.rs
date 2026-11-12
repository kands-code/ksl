//! 求值辅助: 闭包构造与值描述.

use std::sync::Arc;

use crate::cps::gc::GcRegistry;
use crate::cps::ir::{Binding, FunctionKind};
use crate::cps::linear::{Function as LinearFunction, Functions as LinearFunctions};
use crate::symbol::Symbol;
use crate::value::{Capture, Closure, Value};

pub(super) fn closure_linear(
    functions: &Arc<LinearFunctions>,
    function: &Arc<LinearFunction>,
    captures: Arc<[Capture]>,
    gc: &GcRegistry,
) -> Value {
    let params: Vec<Symbol> = function
        .params
        .iter()
        .map(|binder| match binder.target {
            Binding::Named(name) => name,
            Binding::Temp(_) | Binding::Discard => unreachable!("函数参数必须是源码名"),
        })
        .collect();
    let signature = signature_text(function);
    let closure = Arc::new(Closure::new(
        params,
        Arc::clone(function),
        captures,
        Arc::clone(functions),
        Arc::from(signature.as_str()),
        function.display_name,
    ));
    gc.register_closure(&closure);
    Value::Closure(closure)
}

/// 由函数签名拼出显示文本; 只用于闭包与 REPL 展示.
fn signature_text(function: &LinearFunction) -> String {
    let params = function
        .params
        .iter()
        .map(|binder| binder.ty.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    match function.kind {
        FunctionKind::Fn => format!("Fn[[{params}] : {}]", function.ret),
        FunctionKind::Handler => format!("Handler[[{params}] : {}]", function.ret),
    }
}

pub(super) fn describe(value: &Value) -> &'static str {
    match value {
        Value::I64(_) => "整数",
        Value::U8(_) => "u8",
        Value::U64(_) => "u64",
        Value::F64(_) => "浮点数",
        Value::Char(_) => "字符",
        Value::Bool(_) => "布尔值",
        Value::Str(_) => "字符串",
        Value::Nil => "nil",
        Value::List(_) => "列表",
        Value::Pair(_) => "配对",
        Value::Data(_) => "Data",
        Value::Dict(_) => "字典",
        Value::Set(_) => "集合",
        Value::Closure(_) => "闭包",
        Value::Builtin(_) => "内置函数",
        Value::Pid(_) => "Pid",
        Value::File(_) => "File",
        Value::DateTime(_) => "DateTime",
    }
}
