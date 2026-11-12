//! 外来函数值.
//!
//! 函数值持有插件库的保活句柄; 描述符解析与加载失败由宿主报告.

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::RuntimeError as Error;

use super::{Execution, plugin_call};
use crate::native::NativeEntry;
use crate::value::{Value, result_err, result_ok};

/// 已解析的外来函数.
pub struct ForeignFunction {
    name: ArcStr,
    source_name: ArcStr,
    /// 保持插件库加载; 类型擦除使运行时不必依赖加载器.
    _keepalive: Arc<dyn Any + Send + Sync>,
    entry: NativeEntry,
    fallible: bool,
    execution: Execution,
}

impl ForeignFunction {
    /// `keepalive` 在函数值存活期间保持插件库加载; `declared` 决定失败返回的映射.
    #[must_use]
    pub fn new(
        name: &str,
        source_name: &str,
        keepalive: Arc<dyn Any + Send + Sync>,
        declared: &Type,
        entry: NativeEntry,
        execution: Execution,
    ) -> Self {
        let fallible = match declared {
            Type::Fn(_, ret) => result_inner(ret).is_some(),
            _ => false,
        };
        Self {
            name: ArcStr::from(name),
            source_name: ArcStr::from(source_name),
            _keepalive: keepalive,
            entry,
            fallible,
            execution,
        }
    }

    /// 函数声明的执行类别.
    #[must_use]
    pub fn execution(&self) -> Execution {
        self.execution
    }

    /// 调用外来函数.
    ///
    /// # Errors
    ///
    /// 插件返回失败且声明不可恢复时返回运行时错误.
    pub fn call(&self, args: &[Value], span: Span) -> Result<Value, Error> {
        match (self.entry)(args) {
            Ok(value) if self.fallible => Ok(result_ok(value)),
            Ok(value) => Ok(value),
            Err(message) if self.fallible => Ok(result_err(Value::string(&message))),
            Err(message) => Err(plugin_call(message, span)),
        }
    }
}

impl fmt::Debug for ForeignFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<foreign-fn {} ({})>", self.name, self.source_name)
    }
}

/// 取 `Result[T, string]` 的内层类型 `T`; 不是该形态时返回 None.
fn result_inner(ty: &Type) -> Option<&Type> {
    let Type::Result(ok, err) = ty else {
        return None;
    };
    if **err != Type::Str {
        return None;
    }
    Some(ok.as_ref())
}
