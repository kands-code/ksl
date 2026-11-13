//! 运行时值表示.
//!
//! 大于指针的变体一律 `Arc` 包裹, 列表用 imbl 的持久化向量并同样以 `Arc`
//! 包裹; 用户数据无内部可变性, Actor 与 File 句柄由运行时内部同步. `Value`
//! 是 `Send + Sync`, 支持 Actor 消息跨线程传递. 值拷贝为 O(1); 变体载荷以
//! 8 字节瘦指针为主, string 的 `Arc<str>` 是胖指针占 16 字节, 加判别位即
//! 24 字节.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::{Mutex, RwLock};

use crate::builtin::BuiltinId;
use crate::cps::linear::{Function as LinearFunction, Functions};
use crate::io::FileHandle;
use crate::symbol::{Symbol, intern};

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
    /// 字符串不可变, 透明共享.
    Str(Arc<str>),
    /// 列表: 持久化结构, 克隆 O(1), 修改 O(log n) 且不影响旧版本.
    ///
    /// 向量以 `Arc` 包裹: imbl 的向量头本身就有 64 字节, 内联会让 `Value`
    /// 涨到 64 字节; 装箱后向量头成为共享的堆对象, 克隆也只剩一次原子递增.
    /// 标量元素使用窄表示, 其余元素存 `Value`.
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
    Builtin(BuiltinId),
    /// Actor 句柄.
    Pid(Arc<ActorHandle>),
    /// 已打开的文件句柄.
    File(Arc<FileHandle>),
    /// 已连接的字节流句柄.
    Socket(Arc<crate::io::SocketHandle>),
    /// TCP 监听器句柄.
    TcpListener(Arc<crate::io::TcpListenerHandle>),
    /// 时间值: 绝对时刻加时区.
    DateTime(Arc<jiff::Zoned>),
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
    Str(Arc<str>),
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
            Value::Str(value) => Some(Self::Str(Arc::clone(value))),
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
            Self::Str(value) => Value::Str(Arc::clone(value)),
        }
    }
}

/// 列表存储: 标量元素使用窄表示, 其余类型回退到 `Value`.
#[derive(Debug, Clone)]
pub enum List {
    I64(imbl::Vector<i64>),
    U8(imbl::Vector<u8>),
    U64(imbl::Vector<u64>),
    F64(imbl::Vector<f64>),
    Char(imbl::Vector<char>),
    Bool(imbl::Vector<bool>),
    Values(imbl::Vector<Value>),
}

impl List {
    #[must_use]
    pub(crate) fn i64s(items: impl IntoIterator<Item = i64>) -> Self {
        Self::I64(items.into_iter().collect())
    }

    #[must_use]
    pub(crate) fn u8s(items: impl IntoIterator<Item = u8>) -> Self {
        Self::U8(items.into_iter().collect())
    }

    #[must_use]
    pub(crate) fn u64s(items: impl IntoIterator<Item = u64>) -> Self {
        Self::U64(items.into_iter().collect())
    }

    #[must_use]
    pub(crate) fn f64s(items: impl IntoIterator<Item = f64>) -> Self {
        Self::F64(items.into_iter().collect())
    }

    #[must_use]
    pub(crate) fn chars(items: impl IntoIterator<Item = char>) -> Self {
        Self::Char(items.into_iter().collect())
    }

    #[must_use]
    pub(crate) fn bools(items: impl IntoIterator<Item = bool>) -> Self {
        Self::Bool(items.into_iter().collect())
    }

    #[must_use]
    pub(crate) fn from_values(items: impl IntoIterator<Item = Value>) -> Self {
        Self::Values(items.into_iter().collect())
    }

    #[must_use]
    pub(crate) fn empty_i64() -> Self {
        Self::I64(imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn empty_u8() -> Self {
        Self::U8(imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn empty_u64() -> Self {
        Self::U64(imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn empty_f64() -> Self {
        Self::F64(imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn empty_char() -> Self {
        Self::Char(imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn empty_bool() -> Self {
        Self::Bool(imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn empty_values() -> Self {
        Self::Values(imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn same_shape(&self) -> Self {
        match self {
            Self::I64(_) => Self::empty_i64(),
            Self::U8(_) => Self::empty_u8(),
            Self::U64(_) => Self::empty_u64(),
            Self::F64(_) => Self::empty_f64(),
            Self::Char(_) => Self::empty_char(),
            Self::Bool(_) => Self::empty_bool(),
            Self::Values(_) => Self::empty_values(),
        }
    }

    #[must_use]
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::I64(items) => items.len(),
            Self::U8(items) => items.len(),
            Self::U64(items) => items.len(),
            Self::F64(items) => items.len(),
            Self::Char(items) => items.len(),
            Self::Bool(items) => items.len(),
            Self::Values(items) => items.len(),
        }
    }

    #[must_use]
    pub(crate) fn get_value(&self, index: usize) -> Option<Value> {
        match self {
            Self::I64(items) => items.get(index).copied().map(Value::I64),
            Self::U8(items) => items.get(index).copied().map(Value::U8),
            Self::U64(items) => items.get(index).copied().map(Value::U64),
            Self::F64(items) => items.get(index).copied().map(Value::F64),
            Self::Char(items) => items.get(index).copied().map(Value::Char),
            Self::Bool(items) => items.get(index).copied().map(Value::Bool),
            Self::Values(items) => items.get(index).cloned(),
        }
    }

    #[must_use]
    pub(crate) fn front_value(&self) -> Option<Value> {
        match self {
            Self::I64(items) => items.front().copied().map(Value::I64),
            Self::U8(items) => items.front().copied().map(Value::U8),
            Self::U64(items) => items.front().copied().map(Value::U64),
            Self::F64(items) => items.front().copied().map(Value::F64),
            Self::Char(items) => items.front().copied().map(Value::Char),
            Self::Bool(items) => items.front().copied().map(Value::Bool),
            Self::Values(items) => items.front().cloned(),
        }
    }

    #[must_use]
    pub(crate) fn skip(&self, count: usize) -> Self {
        let count = count.min(self.len());
        match self {
            Self::I64(items) => Self::I64(items.skip(count)),
            Self::U8(items) => Self::U8(items.skip(count)),
            Self::U64(items) => Self::U64(items.skip(count)),
            Self::F64(items) => Self::F64(items.skip(count)),
            Self::Char(items) => Self::Char(items.skip(count)),
            Self::Bool(items) => Self::Bool(items.skip(count)),
            Self::Values(items) => Self::Values(items.skip(count)),
        }
    }

    #[must_use]
    pub(crate) fn take(&self, count: usize) -> Self {
        let count = count.min(self.len());
        match self {
            Self::I64(items) => Self::I64(items.take(count)),
            Self::U8(items) => Self::U8(items.take(count)),
            Self::U64(items) => Self::U64(items.take(count)),
            Self::F64(items) => Self::F64(items.take(count)),
            Self::Char(items) => Self::Char(items.take(count)),
            Self::Bool(items) => Self::Bool(items.take(count)),
            Self::Values(items) => Self::Values(items.take(count)),
        }
    }

    #[must_use]
    pub(crate) fn slice(&self, start: usize, count: usize) -> Self {
        self.skip(start).take(count)
    }

    #[must_use]
    pub(crate) fn reverse(&self) -> Self {
        match self {
            Self::I64(items) => Self::I64(items.iter().rev().copied().collect()),
            Self::U8(items) => Self::U8(items.iter().rev().copied().collect()),
            Self::U64(items) => Self::U64(items.iter().rev().copied().collect()),
            Self::F64(items) => Self::F64(items.iter().rev().copied().collect()),
            Self::Char(items) => Self::Char(items.iter().rev().copied().collect()),
            Self::Bool(items) => Self::Bool(items.iter().rev().copied().collect()),
            Self::Values(items) => Self::Values(items.iter().rev().cloned().collect()),
        }
    }

    pub(crate) fn push_back(&mut self, value: Value) {
        match self {
            Self::I64(items) => {
                if let Value::I64(value) = value {
                    items.push_back(value);
                    return;
                }
            },
            Self::U8(items) => {
                if let Value::U8(value) = value {
                    items.push_back(value);
                    return;
                }
            },
            Self::U64(items) => {
                if let Value::U64(value) = value {
                    items.push_back(value);
                    return;
                }
            },
            Self::F64(items) => {
                if let Value::F64(value) = value {
                    items.push_back(value);
                    return;
                }
            },
            Self::Char(items) => {
                if let Value::Char(value) = value {
                    items.push_back(value);
                    return;
                }
            },
            Self::Bool(items) => {
                if let Value::Bool(value) = value {
                    items.push_back(value);
                    return;
                }
            },
            Self::Values(items) => {
                items.push_back(value);
                return;
            },
        }
        self.convert_to_values();
        self.push_back(value);
    }

    pub(crate) fn push_front(&mut self, value: Value) {
        match self {
            Self::I64(items) => {
                if let Value::I64(value) = value {
                    items.push_front(value);
                    return;
                }
            },
            Self::U8(items) => {
                if let Value::U8(value) = value {
                    items.push_front(value);
                    return;
                }
            },
            Self::U64(items) => {
                if let Value::U64(value) = value {
                    items.push_front(value);
                    return;
                }
            },
            Self::F64(items) => {
                if let Value::F64(value) = value {
                    items.push_front(value);
                    return;
                }
            },
            Self::Char(items) => {
                if let Value::Char(value) = value {
                    items.push_front(value);
                    return;
                }
            },
            Self::Bool(items) => {
                if let Value::Bool(value) = value {
                    items.push_front(value);
                    return;
                }
            },
            Self::Values(items) => {
                items.push_front(value);
                return;
            },
        }
        self.convert_to_values();
        self.push_front(value);
    }

    pub(crate) fn set(&mut self, index: usize, value: Value) {
        match self {
            Self::I64(items) => {
                if let Value::I64(value) = value {
                    items.set(index, value);
                    return;
                }
            },
            Self::U8(items) => {
                if let Value::U8(value) = value {
                    items.set(index, value);
                    return;
                }
            },
            Self::U64(items) => {
                if let Value::U64(value) = value {
                    items.set(index, value);
                    return;
                }
            },
            Self::F64(items) => {
                if let Value::F64(value) = value {
                    items.set(index, value);
                    return;
                }
            },
            Self::Char(items) => {
                if let Value::Char(value) = value {
                    items.set(index, value);
                    return;
                }
            },
            Self::Bool(items) => {
                if let Value::Bool(value) = value {
                    items.set(index, value);
                    return;
                }
            },
            Self::Values(items) => {
                items.set(index, value);
                return;
            },
        }
        self.convert_to_values();
        self.set(index, value);
    }

    #[must_use]
    pub(crate) fn concat(self, other: Self) -> Self {
        // 右操作数较小时逐元素追加, 避免为少量元素重平衡整棵 RRB 树;
        // 反复把短片段拼到长列表上时, 这是字符串构建的常见路径.
        const SMALL_CONCAT: usize = 128;
        if other.len() <= SMALL_CONCAT {
            return self.append(other);
        }
        match (self, other) {
            (Self::I64(first), Self::I64(second)) => Self::I64(first + second),
            (Self::U8(first), Self::U8(second)) => Self::U8(first + second),
            (Self::U64(first), Self::U64(second)) => Self::U64(first + second),
            (Self::F64(first), Self::F64(second)) => Self::F64(first + second),
            (Self::Char(first), Self::Char(second)) => Self::Char(first + second),
            (Self::Bool(first), Self::Bool(second)) => Self::Bool(first + second),
            (Self::Values(first), Self::Values(second)) => Self::Values(first + second),
            (first, second) => Self::Values(first.into_values() + second.into_values()),
        }
    }

    /// 逐元素追加到同形态列表; 混合形态回退为 `Value` 列表.
    fn append(self, other: Self) -> Self {
        match (self, other) {
            (Self::I64(mut first), Self::I64(second)) => {
                first.extend(second);
                Self::I64(first)
            },
            (Self::U8(mut first), Self::U8(second)) => {
                first.extend(second);
                Self::U8(first)
            },
            (Self::U64(mut first), Self::U64(second)) => {
                first.extend(second);
                Self::U64(first)
            },
            (Self::F64(mut first), Self::F64(second)) => {
                first.extend(second);
                Self::F64(first)
            },
            (Self::Char(mut first), Self::Char(second)) => {
                first.extend(second);
                Self::Char(first)
            },
            (Self::Bool(mut first), Self::Bool(second)) => {
                first.extend(second);
                Self::Bool(first)
            },
            (Self::Values(mut first), Self::Values(second)) => {
                first.extend(second);
                Self::Values(first)
            },
            (first, second) => Self::Values(first.into_values() + second.into_values()),
        }
    }

    #[must_use]
    pub(crate) fn into_values(self) -> imbl::Vector<Value> {
        match self {
            Self::I64(items) => items.into_iter().map(Value::I64).collect(),
            Self::U8(items) => items.into_iter().map(Value::U8).collect(),
            Self::U64(items) => items.into_iter().map(Value::U64).collect(),
            Self::F64(items) => items.into_iter().map(Value::F64).collect(),
            Self::Char(items) => items.into_iter().map(Value::Char).collect(),
            Self::Bool(items) => items.into_iter().map(Value::Bool).collect(),
            Self::Values(items) => items,
        }
    }

    #[must_use]
    pub(crate) fn to_values(&self) -> imbl::Vector<Value> {
        match self {
            Self::I64(items) => items.iter().copied().map(Value::I64).collect(),
            Self::U8(items) => items.iter().copied().map(Value::U8).collect(),
            Self::U64(items) => items.iter().copied().map(Value::U64).collect(),
            Self::F64(items) => items.iter().copied().map(Value::F64).collect(),
            Self::Char(items) => items.iter().copied().map(Value::Char).collect(),
            Self::Bool(items) => items.iter().copied().map(Value::Bool).collect(),
            Self::Values(items) => items.clone(),
        }
    }

    fn convert_to_values(&mut self) {
        *self = Self::Values(self.to_values());
    }
}

/// Actor 终止原因.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExitReason {
    Halted,
    Killed,
    Failed { code: i64, message: String },
}

/// Actor 的最终结局.
pub(crate) type ActorResult = Result<Value, ExitReason>;

/// Actor 状态; 终止后保留结局供 Join 读取.
#[derive(Debug, Clone)]
enum ActorStatus {
    Running,
    Finished(ActorResult),
}

/// Actor 句柄: 信箱, 状态与父子关系.
#[derive(Debug)]
pub struct ActorHandle {
    pub(crate) id: u64,
    /// 父 Actor 句柄; Detached Actor 为 None.
    pub(crate) parent: Option<Arc<ActorHandle>>,
    mailboxes: Vec<Mutex<VecDeque<Value>>>,
    status: Mutex<ActorStatus>,
    children: Mutex<Vec<u64>>,
    kill: AtomicBool,
    /// 是否已通过 `@Join` 或 `@Monitor` 观察; 已观察的失败不再重复诊断.
    ///
    /// 用 `Arc` 共享给错误记录: 句柄释放后, 观察标记仍能决定该错误是否
    /// 已被观察, 不依赖句柄存活.
    observed: Arc<AtomicBool>,
}

impl ActorHandle {
    #[must_use]
    pub(crate) fn new(id: u64, mailboxes: usize, parent: Option<Arc<ActorHandle>>) -> Self {
        Self {
            id,
            parent,
            mailboxes: (0..mailboxes).map(|_| Mutex::new(VecDeque::new())).collect(),
            status: Mutex::new(ActorStatus::Running),
            children: Mutex::new(Vec::new()),
            kill: AtomicBool::new(false),
            observed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 标记该 Actor 的失败已被 `@Join` 或 `@Monitor` 观察.
    pub(crate) fn mark_observed(&self) {
        self.observed.store(true, Ordering::Relaxed);
    }

    /// 与错误记录共享的观察标记.
    #[must_use]
    pub(crate) fn observation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.observed)
    }

    /// 最后一个信箱的下标; 控制信箱固定放在这里.
    ///
    /// `@Spawn` 的信箱数由检查器保证在 1 到 16 之间, 因此下标一定存在.
    pub(crate) fn last_mailbox_index(&self) -> usize {
        self.mailboxes.len() - 1
    }

    /// 投递消息; 索引越界返回 false.
    pub(crate) fn send(&self, index: usize, message: Value) -> bool {
        let Some(mailbox) = self.mailboxes.get(index) else {
            return false;
        };
        mailbox.lock().push_back(message);
        true
    }

    /// 尝试从指定信箱取一条消息.
    #[must_use]
    pub(crate) fn try_recv(&self, index: usize) -> Option<Value> {
        self.mailboxes.get(index)?.lock().pop_front()
    }

    /// 标记终止; 已终止时返回 false, 保留最初结局.
    pub(crate) fn finish(&self, result: ActorResult) -> bool {
        let mut status = self.status.lock();
        if matches!(*status, ActorStatus::Finished(_)) {
            return false;
        }
        *status = ActorStatus::Finished(result);
        true
    }

    /// 读取终止结局; 未终止时为 None.
    #[must_use]
    pub(crate) fn result(&self) -> Option<ActorResult> {
        match &*self.status.lock() {
            ActorStatus::Running => None,
            ActorStatus::Finished(result) => Some(result.clone()),
        }
    }

    /// 是否已终止.
    ///
    /// 只判断状态, 不复制结局; `@Send`, `@Kill` 与注册表仅需知道目标是否
    /// 存活, 不应为一个大结果付出克隆成本.
    #[must_use]
    pub(crate) fn is_finished(&self) -> bool {
        matches!(*self.status.lock(), ActorStatus::Finished(_))
    }

    /// 已终止时的退出原因; 正常返回记为 `Halted`.
    ///
    /// 监视通知只关心原因, 不需要成功结果, 故不复制完整结局.
    #[must_use]
    pub(crate) fn exit_reason(&self) -> Option<ExitReason> {
        match &*self.status.lock() {
            ActorStatus::Running => None,
            ActorStatus::Finished(Ok(_)) => Some(ExitReason::Halted),
            ActorStatus::Finished(Err(reason)) => Some(reason.clone()),
        }
    }

    pub(crate) fn request_kill(&self) {
        self.kill.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn kill_requested(&self) -> bool {
        self.kill.load(Ordering::Relaxed)
    }

    /// 父句柄; 环回收遍历用.
    #[must_use]
    pub(crate) fn parent_handle(&self) -> Option<&Arc<ActorHandle>> {
        self.parent.as_ref()
    }

    /// 遍历信箱与终止结果中的值; 环回收标记用.
    pub(crate) fn for_each_stored(&self, visit: &mut dyn FnMut(&Value)) {
        for mailbox in &self.mailboxes {
            for value in mailbox.lock().iter() {
                visit(value);
            }
        }
        if let ActorStatus::Finished(Ok(value)) = &*self.status.lock() {
            visit(value);
        }
    }

    /// 断开不可达句柄的强引用; 只能由环回收在安全点调用.
    pub(crate) fn clear_for_gc(&self) {
        for mailbox in &self.mailboxes {
            mailbox.lock().clear();
        }
        // 终止结果中的值一并丢弃; 不可达句柄不会再有合法读取.
        *self.status.lock() = ActorStatus::Running;
    }

    pub(crate) fn add_child(&self, id: u64) {
        self.children.lock().push(id);
    }

    pub(crate) fn take_children(&self) -> Vec<u64> {
        std::mem::take(&mut *self.children.lock())
    }
}

/// 把终止原因转为内置 Data 值.
#[must_use]
pub(crate) fn exit_reason_value(reason: &ExitReason) -> Value {
    let ty = intern("ExitReason");
    match reason {
        ExitReason::Halted => data_value0(ty, intern("Halted")),
        ExitReason::Killed => data_value0(ty, intern("Killed")),
        ExitReason::Failed { code, message } => data_value(
            ty,
            intern("Failed"),
            [Value::I64(*code), Value::Str(Arc::from(message.as_str()))],
        ),
    }
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

/// 构造列表值; 向量以 `Arc` 包裹, 克隆列表值只需一次原子递增.
#[must_use]
pub(crate) fn list(items: impl IntoIterator<Item = Value>) -> Value {
    Value::List(Arc::new(List::from_values(items)))
}

/// 取出列表存储: 引用计数为一时直接取走, 否则复制一份.
#[must_use]
pub(crate) fn list_owned(list: Arc<List>) -> List {
    Arc::try_unwrap(list).unwrap_or_else(|shared| (*shared).clone())
}

#[must_use]
pub(crate) fn option_some(value: Value) -> Value {
    data_value(intern("Option"), intern("Some"), vec![value])
}

#[must_use]
pub(crate) fn option_none() -> Value {
    data_value0(intern("Option"), intern("None"))
}

#[must_use]
pub(crate) fn result_ok(value: Value) -> Value {
    data_value(intern("Result"), intern("Ok"), vec![value])
}

#[must_use]
pub(crate) fn result_err(error: Value) -> Value {
    data_value(intern("Result"), intern("Err"), vec![error])
}

#[must_use]
pub(crate) fn result_err_str(message: String) -> Value {
    result_err(Value::Str(Arc::from(message)))
}

/// 把 Actor 结局转为 `Result[R, ExitReason]`.
#[must_use]
pub(crate) fn join_result_value(result: &ActorResult) -> Value {
    match result {
        Ok(value) => result_ok(value.clone()),
        Err(reason) => result_err(exit_reason_value(reason)),
    }
}

/// Data 值: 类型名, 变体名与字段.
#[derive(Debug)]
pub struct DataValue {
    pub ty: Symbol,
    pub variant: Symbol,
    pub fields: Box<[Value]>,
}

/// 线性 IR 闭包: 指向函数体, 携带捕获与函数表.
#[derive(Debug)]
pub struct Closure {
    pub params: Vec<Symbol>,
    pub function: Arc<LinearFunction>,
    /// 捕获值; 递归定义使用写一次单元, 其余按值捕获.
    captures: RwLock<Arc<[Capture]>>,
    pub functions: Arc<Functions>,
    /// 显示用签名, 例如 `Fn[[i64] : i64]`.
    pub signature: Arc<str>,
    /// 具名函数显示用名字; 匿名函数与 Handler 为 None.
    pub name: Option<Symbol>,
}

impl Closure {
    #[must_use]
    pub(crate) fn new(
        params: Vec<Symbol>,
        function: Arc<LinearFunction>,
        captures: Arc<[Capture]>,
        functions: Arc<Functions>,
        signature: Arc<str>,
        name: Option<Symbol>,
    ) -> Self {
        Self {
            params,
            function,
            captures: RwLock::new(captures),
            functions,
            signature,
            name,
        }
    }

    /// 读取捕获值.
    #[must_use]
    pub(crate) fn captures(&self) -> Arc<[Capture]> {
        Arc::clone(&self.captures.read())
    }

    /// 断开不可达闭包的捕获引用; 只能由环回收在安全点调用.
    pub(crate) fn clear_captures(&self) {
        *self.captures.write() = Arc::from(Vec::new());
    }
}

/// 闭包捕获: 普通绑定按值捕获, 递归绑定共享写一次单元.
#[derive(Debug, Clone)]
pub(crate) enum Capture {
    Value(Value),
    Fix(FixCell),
    /// 全局环境缺少该名字; 读取时报告内部错误.
    Missing(Symbol),
}

impl Capture {
    pub(crate) fn get(&self) -> Option<Value> {
        match self {
            Self::Value(value) => Some(value.clone()),
            Self::Fix(cell) => cell.get().cloned(),
            Self::Missing(_) => None,
        }
    }
}

/// 运行时环境: 名字到不可变值.
pub type Env = imbl::HashMap<Symbol, Value>;

/// 递归定义的写一次单元; 建好闭包后回填.
pub(crate) type FixCell = Arc<OnceLock<Value>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Value>();
        assert_send_sync::<Closure>();
        assert_send_sync::<ActorHandle>();
        assert_send_sync::<FileHandle>();
    }

    /// 值表示的体积是运行时表示的不变量, 在 ksl 源码层面看不见, 故由本测试守住.
    ///
    /// 24 字节上限的构成: 载荷以 8 字节瘦指针为主, string 的 `Arc<str>` 是
    /// 胖指针占 16 字节, 加判别位即 24.
    #[test]
    fn value_fits_target_size() {
        assert!(
            size_of::<Value>() <= 24,
            "Value 应不超过 24 字节, 实际 {}",
            size_of::<Value>(),
        );
    }
}
