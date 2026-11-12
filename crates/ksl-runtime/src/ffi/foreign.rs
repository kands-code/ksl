//! 外来函数与不透明句柄.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Weak};

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, resolve};
use ksl_frontend::check::Type;
use ksl_frontend::error::RuntimeError as Error;
use ksl_plugin::{
    KslFunctionEntry, KslType, KslValue, STATUS_ERROR, THREAD_RUNTIME, TYPE_BOOL, TYPE_CHAR,
    TYPE_F64, TYPE_I64, TYPE_NIL, TYPE_OPAQUE, TYPE_STRING, TYPE_U8, TYPE_U64, VALUE_STRING,
};

use super::{Execution, PluginInstance, ReleaseQueue, plugin_call};
use crate::value::{Value, result_err, result_ok};

/// 一个参数或返回值在插件 ABI 中的类型.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FfiType {
    I64,
    U64,
    U8,
    F64,
    Bool,
    Char,
    Str,
    Nil,
    Opaque(u32),
}

impl FfiType {
    /// 由 ksl 类型与不透明类型映射得到; 不受支持时返回 None.
    pub(super) fn from_ksl(ty: &Type, types: &HashMap<Symbol, u32>) -> Option<Self> {
        match ty {
            Type::I64 => Some(Self::I64),
            Type::U64 => Some(Self::U64),
            Type::U8 => Some(Self::U8),
            Type::F64 => Some(Self::F64),
            Type::Bool => Some(Self::Bool),
            Type::Char => Some(Self::Char),
            Type::Str => Some(Self::Str),
            Type::Nil => Some(Self::Nil),
            Type::Opaque(name) => types.get(name).copied().map(Self::Opaque),
            _ => None,
        }
    }

    /// 由插件描述符中的类型标签得到.
    pub(super) fn from_abi(ty: KslType) -> Option<Self> {
        match ty.tag {
            TYPE_I64 => Some(Self::I64),
            TYPE_U64 => Some(Self::U64),
            TYPE_U8 => Some(Self::U8),
            TYPE_F64 => Some(Self::F64),
            TYPE_BOOL => Some(Self::Bool),
            TYPE_CHAR => Some(Self::Char),
            TYPE_STRING => Some(Self::Str),
            TYPE_NIL => Some(Self::Nil),
            TYPE_OPAQUE => Some(Self::Opaque(ty.opaque)),
            _ => None,
        }
    }
}

/// 已解析的外来函数.
pub struct ForeignFunction {
    pub(super) name: ArcStr,
    pub(super) source_name: ArcStr,
    pub(super) plugin: Arc<PluginInstance>,
    pub(super) declared: Type,
    pub(super) params: Vec<FfiType>,
    pub(super) ret: FfiType,
    pub(super) ret_declared: Type,
    pub(super) fallible: bool,
    pub(super) entry: KslFunctionEntry,
    pub(super) execution: Execution,
}

impl fmt::Debug for ForeignFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<foreign-fn {} ({})>", self.name, self.source_name)
    }
}

impl ForeignFunction {
    /// 链接名.
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// `@Link` 中声明的函数类型.
    #[must_use]
    pub(crate) fn declared(&self) -> &Type {
        &self.declared
    }

    /// 函数声明的执行类别.
    #[must_use]
    pub(crate) fn execution(&self) -> Execution {
        self.execution
    }

    /// 调用外来函数.
    ///
    /// # Errors
    ///
    /// 实参形态不符或插件返回失败时返回运行时错误.
    pub(crate) fn call(
        &self,
        args: &[Value],
        span: Span,
        releases: &Weak<ReleaseQueue>,
    ) -> Result<Value, Error> {
        if args.len() != self.params.len() {
            return Err(plugin_call("实参个数不符", span));
        }
        let mut abi_args = Vec::with_capacity(args.len());
        for (expected, value) in self.params.iter().zip(args) {
            abi_args.push(arg_to_abi(*expected, value, &self.plugin, span)?);
        }
        let mut out = KslValue::nil();
        let count = u32::try_from(abi_args.len()).map_err(|_| plugin_call("实参过多", span))?;
        let status =
            unsafe { (self.entry)(abi_args.as_ptr(), count, std::ptr::from_mut(&mut out)) };
        if status != 0 {
            let message = error_message(&out);
            if self.fallible && status == STATUS_ERROR {
                return Ok(result_err(Value::Str(ArcStr::from(message))));
            }
            return Err(plugin_call(message, span));
        }
        let value =
            value_from_abi(self.ret, &self.ret_declared, &self.plugin, &out, span, releases)?;
        if self.fallible {
            Ok(result_ok(value))
        } else {
            Ok(value)
        }
    }
}

/// 外来库返回的不透明句柄.
#[derive(Clone)]
pub struct ForeignHandle(Arc<ForeignObject>);

struct ForeignObject {
    ty: Symbol,
    type_index: u32,
    handle: u64,
    owned: bool,
    plugin: Arc<PluginInstance>,
    /// 声明调度线程释放时, 释放任务经此队列交给协调器.
    ///
    /// 只持弱引用, 避免工作线程上的实参在运行时关闭后成为队列的最后一个持有者.
    releases: Weak<ReleaseQueue>,
}

impl ForeignHandle {
    fn new(
        ty: Symbol,
        type_index: u32,
        handle: u64,
        owned: bool,
        plugin: Arc<PluginInstance>,
        releases: Weak<ReleaseQueue>,
    ) -> Self {
        Self(Arc::new(ForeignObject {
            ty,
            type_index,
            handle,
            owned,
            plugin,
            releases,
        }))
    }

    /// ksl 侧的不透明类型.
    #[must_use]
    pub fn ty(&self) -> Symbol {
        self.0.ty
    }

    fn type_index(&self) -> u32 {
        self.0.type_index
    }

    fn belongs_to(&self, plugin: &Arc<PluginInstance>) -> bool {
        Arc::ptr_eq(&self.0.plugin, plugin)
    }

    fn handle(&self) -> u64 {
        self.0.handle
    }

    fn owned(&self) -> bool {
        self.0.owned
    }
}

impl fmt::Debug for ForeignHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<foreign-handle {}>", resolve(self.0.ty))
    }
}

impl Drop for ForeignObject {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        let plugin = Arc::clone(&self.plugin);
        let type_index = self.type_index;
        let handle = self.handle;
        let release = move || plugin.release(type_index, handle);
        if self.plugin.release_thread(self.type_index) == THREAD_RUNTIME {
            if let Some(queue) = self.releases.upgrade() {
                queue.push(Box::new(release));
            } else {
                // 运行时已关闭, 没有可交回的调度线程.
                release();
            }
        } else {
            release();
        }
    }
}

fn arg_to_abi(
    expected: FfiType,
    value: &Value,
    plugin: &Arc<PluginInstance>,
    span: Span,
) -> Result<KslValue, Error> {
    match (expected, value) {
        (FfiType::I64, Value::I64(value)) => Ok(KslValue::i64(*value)),
        (FfiType::U64, Value::U64(value)) => Ok(KslValue::u64(*value)),
        (FfiType::U8, Value::U8(value)) => Ok(KslValue::u8(*value)),
        (FfiType::F64, Value::F64(value)) => Ok(KslValue::f64(*value)),
        (FfiType::Bool, Value::Bool(value)) => Ok(KslValue::bool(*value)),
        (FfiType::Char, Value::Char(value)) => Ok(KslValue::char(*value)),
        (FfiType::Str, Value::Str(value)) => Ok(KslValue::string_borrowed(value)),
        (FfiType::Nil, Value::Nil) => Ok(KslValue::nil()),
        (FfiType::Opaque(expected), Value::ForeignHandle(handle)) => {
            if handle.type_index() != expected {
                return Err(plugin_call("不透明类型不符", span));
            }
            // 同名类型可能来自另一份动态库; 句柄只能交还给创建它的实例.
            if !handle.belongs_to(plugin) {
                return Err(plugin_call("不透明句柄不属于当前插件", span));
            }
            Ok(KslValue::opaque(expected, handle.handle(), handle.owned()))
        },
        _ => Err(plugin_call("外来函数实参类型不符", span)),
    }
}

pub(super) fn value_from_abi(
    ty: FfiType,
    declared: &Type,
    plugin: &Arc<PluginInstance>,
    value: &KslValue,
    span: Span,
    releases: &Weak<ReleaseQueue>,
) -> Result<Value, Error> {
    match ty {
        FfiType::I64 => Ok(Value::I64(
            ksl_plugin::value::as_i64(value).map_err(|message| plugin_call(message, span))?,
        )),
        FfiType::U64 => Ok(Value::U64(
            ksl_plugin::value::as_u64(value).map_err(|message| plugin_call(message, span))?,
        )),
        FfiType::U8 => Ok(Value::U8(
            ksl_plugin::value::as_u8(value).map_err(|message| plugin_call(message, span))?,
        )),
        FfiType::F64 => Ok(Value::F64(
            ksl_plugin::value::as_f64(value).map_err(|message| plugin_call(message, span))?,
        )),
        FfiType::Bool => Ok(Value::Bool(
            ksl_plugin::value::as_bool(value).map_err(|message| plugin_call(message, span))?,
        )),
        FfiType::Char => Ok(Value::Char(
            ksl_plugin::value::as_char(value).map_err(|message| plugin_call(message, span))?,
        )),
        FfiType::Str => {
            let text = ksl_plugin::value::as_str(value)
                .map_err(|message| plugin_call(message, span))?;
            Ok(Value::Str(ArcStr::from(text)))
        },
        FfiType::Nil => {
            if ksl_plugin::value::is_nil(value) {
                Ok(Value::Nil)
            } else {
                Err(plugin_call("外来函数应返回 nil", span))
            }
        },
        FfiType::Opaque(expected) => {
            let opaque = ksl_plugin::value::as_opaque(value)
                .map_err(|message| plugin_call(message, span))?;
            if opaque.type_index != expected {
                return Err(plugin_call("不透明类型不符", span));
            }
            let Type::Opaque(ty) = declared else {
                return Err(Error::internal("不透明返回缺少 ksl 类型"));
            };
            Ok(Value::ForeignHandle(ForeignHandle::new(
                *ty,
                expected,
                opaque.handle,
                opaque.owned != 0,
                Arc::clone(plugin),
                Weak::clone(releases),
            )))
        },
    }
}

pub(super) fn error_message(value: &KslValue) -> String {
    if value.tag == VALUE_STRING {
        ksl_plugin::value::as_str(value)
            .map_or_else(|_| "插件返回了非法错误信息".to_string(), ToString::to_string)
    } else {
        "插件返回失败但没有错误信息".to_string()
    }
}
