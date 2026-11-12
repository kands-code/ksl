//! 求值辅助: 闭包构造与值描述.

use std::sync::Arc;

use crate::cps::linear::{Function as LinearFunction, Functions as LinearFunctions};
use crate::runtime::gc::GcRegistry;
use crate::value::{Capture, Closure, Value};

pub(super) fn closure_linear(
    functions: &Arc<LinearFunctions>,
    function: &Arc<LinearFunction>,
    captures: Arc<[Capture]>,
    gc: &GcRegistry,
) -> Value {
    let closure = Arc::new(Closure::new(
        function.params.len(),
        Arc::clone(function),
        captures,
        Arc::clone(functions),
        function.display_name,
    ));
    gc.register_closure(&closure);
    Value::Closure(closure)
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
        Value::CommandOutput(_) => "CommandOutput",
        Value::Socket(_) => "Socket",
        Value::TcpListener(_) => "TcpListener",
        Value::DateTime(_) => "DateTime",
        Value::Regex(_) => "Regex",
        Value::RegexMatch(_) => "RegexMatch",
        Value::ForeignFn(_) => "外来函数",
        Value::ForeignHandle(_) => "外来句柄",
    }
}
