//! 运行时值表示.
//!
//! 大于指针的变体一律 `Arc` 包裹, 列表用 imbl 的持久化向量, 同样以 `Arc` 包裹.
//! 用户数据无内部可变性; Actor 与 File 句柄由运行时内部同步.
//! `Value` 是 `Send + Sync`, 支持跨线程传递消息. 值拷贝为 O(1).

use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::symbol::Symbol;

pub use self::actor::ActorHandle;
pub(crate) use self::actor::{ActorResult, ExitReason, exit_reason_value, join_result_value};
pub use self::closure::Closure;
pub(crate) use self::closure::{Capture, FixCell};
pub use self::data::DataValue;
pub(crate) use self::data::{
    data_value, data_value0, option_none, option_some, result_err, result_ok,
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
    U8(u8),
    U64(u64),
    F64(f64),
    Char(char),
    Bool(bool),
    Nil,
    /// 字符串不可变, 透明共享; 单指针表示.
    Str(ArcStr),
    /// 列表: 持久化结构, 克隆 O(1), 修改不影响旧版本.
    ///
    /// 向量以 `Arc` 包裹, 避免 imbl 的向量头放大 `Value`; 标量元素使用紧凑表示.
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
    Builtin(crate::builtin::BuiltinId),
    /// Actor 句柄.
    Pid(Arc<ActorHandle>),
    /// 已打开的文件句柄.
    File(Arc<crate::io::FileHandle>),
    /// 命令执行结果.
    CommandOutput(Arc<crate::io::CommandOutput>),
    /// 已连接的字节流句柄.
    Socket(Arc<crate::io::SocketHandle>),
    /// TCP 监听器句柄.
    TcpListener(Arc<crate::io::TcpListenerHandle>),
    /// 时间值: 绝对时刻加时区.
    DateTime(Arc<jiff::ZonedDateTime>),
    /// 编译后的正则.
    Regex(Arc<crate::regex::CompiledRegex>),
    /// 一处正则匹配的结果.
    RegexMatch(Arc<crate::regex::MatchRecord>),
    /// 外来函数.
    ForeignFn(Arc<crate::ffi::ForeignFunction>),
    /// 外来库返回的不透明句柄.
    ForeignHandle(crate::ffi::ForeignHandle),
}

/// Dict 与 Set 的键/元素.
///
/// 只接受基础标量, 以保证有序, 可比较且不形成环.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    I64(i64),
    U8(u8),
    U64(u64),
    Char(char),
    Bool(bool),
    Str(ArcStr),
}

impl Key {
    /// 由值转换; 不支持的类型返回 None.
    #[must_use]
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        match value {
            Value::I64(value) => Some(Self::I64(*value)),
            Value::U8(value) => Some(Self::U8(*value)),
            Value::U64(value) => Some(Self::U64(*value)),
            Value::Char(value) => Some(Self::Char(*value)),
            Value::Bool(value) => Some(Self::Bool(*value)),
            Value::Str(value) => Some(Self::Str(value.clone())),
            _ => None,
        }
    }

    #[must_use]
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Self::I64(value) => Value::I64(*value),
            Self::U8(value) => Value::U8(*value),
            Self::U64(value) => Value::U64(*value),
            Self::Char(value) => Value::Char(*value),
            Self::Bool(value) => Value::Bool(*value),
            Self::Str(value) => Value::Str(value.clone()),
        }
    }
}

/// 构造列表值.
#[must_use]
pub(crate) fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::List(Arc::new(List::from_values(items)))
}

/// 取出列表存储: 引用计数为一时直接取走, 否则复制一份.
#[must_use]
pub(crate) fn list_owned(list: Arc<List>) -> List {
    Arc::try_unwrap(list).unwrap_or_else(|shared| (*shared).clone())
}

/// 运行时环境: 名字到不可变值.
pub type Env = imbl::HashMap<Symbol, Value>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Value>();
        assert_send_sync::<Closure>();
        assert_send_sync::<ActorHandle>();
        assert_send_sync::<crate::io::FileHandle>();
    }

    /// 值表示的体积不变量.
    ///
    /// 全部载荷不超过 8 字节; 判别位与载荷对齐后共 16 字节.
    /// string 的 `ArcStr` 与 `Arc` 包裹的句柄都是单指针.
    #[test]
    fn value_fits_target_size() {
        assert_eq!(
            size_of::<Value>(),
            16,
            "Value 应为 16 字节, 实际 {}",
            size_of::<Value>(),
        );
    }
}
