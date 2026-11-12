//! 运行时值表示.
//!
//! 大于指针的变体一律 `Arc` 包裹, 列表用 imbl 的持久化向量, 同样以 `Arc` 包裹.
//! 用户数据无内部可变性; Actor 与不透明值内部的 I/O 资源由运行时内部同步.
//! `Value` 是 `Send + Sync`, 支持跨线程传递消息. 值拷贝为 O(1).

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::name::Name;

pub use self::actor::{ActorHandle, ActorResult, ExitReason, join_result_value};
pub use self::closure::{Capture, Closure, FixCell};
pub use self::data::{
    DataFields, DataValue, data_value, data_value0, option_none, option_some, result_err,
    result_ok,
};
pub use self::list::List;

mod actor;
mod closure;
mod data;
mod list;

/// 运行时值.
#[derive(Debug, Clone)]
pub enum Value {
    I64(i64),
    I32(i32),
    U8(u8),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Char(char),
    Bool(bool),
    Nil,
    /// 字符串不可变, 透明共享; 单指针表示.
    Str(ArcStr),
    /// 列表: 持久化结构, 克隆 O(1).
    ///
    /// 向量以 `Arc` 包裹; 标量元素使用紧凑表示.
    List(Arc<List>),
    /// 配对不可变, 透明共享.
    Pair(Arc<(Value, Value)>),
    /// Data 值携带类型与变体名, 结构化打印与 `@Match` 分派靠它.
    Data(Arc<DataValue>),
    /// 字典: 持久化有序映射, 键限制为基础标量.
    Dict(Arc<imbl::OrdMap<Key, Value>>),
    /// 集合: 持久化有序集合, 元素限制为基础标量.
    Set(Arc<imbl::OrdSet<Key>>),
    /// 执行线性 IR 的闭包.
    Closure(Arc<Closure>),
    /// 内置函数的函数值, 按降级期分配的编号分派.
    Builtin(ksl_catalog::builtin::BuiltinId),
    /// Actor 句柄.
    Pid(Arc<ActorHandle>),
    /// 外来函数.
    ForeignFn(Arc<crate::ffi::ForeignFunction>),
    /// 宿主资源: 内置的文件, 套接字, 时间, 正则等与原生插件资源共用.
    NativeOpaque(NativeOpaqueValue),
}

/// 原生插件资源.
///
/// 插件实现该 trait, 用 `Value::native_opaque` 包装资源, 用 `Value::as_opaque::<T>` 取回.
pub trait NativeOpaque: Any + Send + Sync {
    /// ksl 类型名; 与 `@Link` 中声明的不透明类型名一致.
    fn type_name(&self) -> &'static str;
}

/// 原生插件资源值.
///
/// `Arc` 持有; 最后一个引用消失时在释放线程调用析构.
/// 外层 `Arc` 只指向固定大小的包装, 保持 `Value` 为 16 字节.
#[derive(Clone)]
pub struct NativeOpaqueValue(Arc<NativeOpaqueInner>);

struct NativeOpaqueInner {
    value: Box<dyn NativeOpaque>,
}

impl NativeOpaqueValue {
    #[must_use]
    pub fn new<T: NativeOpaque>(value: T) -> Self {
        Self(Arc::new(NativeOpaqueInner {
            value: Box::new(value),
        }))
    }

    #[must_use]
    pub fn type_name(&self) -> &'static str {
        self.0.value.type_name()
    }

    /// 按具体类型取回借用.
    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self.0.value.as_ref() as &dyn Any
    }
}

impl fmt::Debug for NativeOpaqueValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.type_name())
    }
}

impl Value {
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::I64(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::I32(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_u8(&self) -> Option<u8> {
        match self {
            Self::U8(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::U64(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::F32(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::F64(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_char(&self) -> Option<char> {
        match self {
            Self::Char(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(text) => Some(text),
            _ => None,
        }
    }

    /// 列表长度; 不是列表时为 None.
    #[must_use]
    pub fn list_len(&self) -> Option<usize> {
        match self {
            Self::List(list) => Some(list.len()),
            _ => None,
        }
    }

    /// 按位置读取列表元素; 不是列表或越界时为 None.
    #[must_use]
    pub fn list_get(&self, index: usize) -> Option<Value> {
        match self {
            Self::List(list) => list.get_value(index),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_pair(&self) -> Option<(&Value, &Value)> {
        match self {
            Self::Pair(pair) => Some((&pair.0, &pair.1)),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_data(&self) -> Option<&DataValue> {
        match self {
            Self::Data(data) => Some(data),
            _ => None,
        }
    }

    #[must_use]
    pub fn native_opaque<T: NativeOpaque>(value: T) -> Self {
        Self::NativeOpaque(NativeOpaqueValue::new(value))
    }

    /// 按具体类型读取原生插件资源.
    #[must_use]
    pub fn as_opaque<T: NativeOpaque>(&self) -> Option<&T> {
        match self {
            Self::NativeOpaque(value) => value.as_any().downcast_ref::<T>(),
            _ => None,
        }
    }

    #[must_use]
    pub fn string(text: &str) -> Self {
        Self::Str(ArcStr::from(text))
    }

    #[must_use]
    pub fn list(items: Vec<Value>) -> Self {
        Self::List(Arc::new(List::from_values(items)))
    }

    /// 构造紧凑的 `List[f32]`; 供插件返回样本序列.
    #[must_use]
    pub fn list_f32(items: impl IntoIterator<Item = f32>) -> Self {
        Self::List(Arc::new(List::f32s(items)))
    }

    #[must_use]
    pub fn pair(first: Value, second: Value) -> Self {
        Self::Pair(Arc::new((first, second)))
    }

    #[must_use]
    pub fn some(value: Value) -> Self {
        option_some(value)
    }

    #[must_use]
    pub fn none() -> Self {
        option_none()
    }

    #[must_use]
    pub fn ok(value: Value) -> Self {
        result_ok(value)
    }

    #[must_use]
    pub fn err(error: Value) -> Self {
        result_err(error)
    }
}

/// Dict 与 Set 的键/元素.
///
/// 只接受基础标量: 有序, 可比较, 不形成环.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    I64(i64),
    I32(i32),
    U8(u8),
    U32(u32),
    U64(u64),
    Char(char),
    Bool(bool),
    Str(ArcStr),
}

impl Key {
    /// 由值转换; 不支持的类型返回 None.
    #[must_use]
    pub fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::I64(value) => Some(Self::I64(*value)),
            Value::I32(value) => Some(Self::I32(*value)),
            Value::U8(value) => Some(Self::U8(*value)),
            Value::U32(value) => Some(Self::U32(*value)),
            Value::U64(value) => Some(Self::U64(*value)),
            Value::Char(value) => Some(Self::Char(*value)),
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::Str(value) => Some(Self::Str(value.clone())),
            _ => None,
        }
    }

    #[must_use]
    pub fn to_value(&self) -> Value {
        match self {
            Self::I64(value) => Value::I64(*value),
            Self::I32(value) => Value::I32(*value),
            Self::U8(value) => Value::U8(*value),
            Self::U32(value) => Value::U32(*value),
            Self::U64(value) => Value::U64(*value),
            Self::Char(value) => Value::Char(*value),
            Self::Bool(value) => Value::Bool(*value),
            Self::Str(value) => Value::Str(value.clone()),
        }
    }
}

#[must_use]
pub fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::List(Arc::new(List::from_values(items)))
}

/// 取出列表存储: 引用计数为一时直接取走, 否则复制一份.
#[must_use]
pub fn list_owned(list: Arc<List>) -> List {
    Arc::unwrap_or_clone(list)
}

/// 运行时环境: 名字到不可变值.
pub type Env = imbl::HashMap<Name, Value>;
