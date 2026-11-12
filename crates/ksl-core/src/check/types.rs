//! 类型表示.
//!
//! 类型系统的全部形态; 显示形态与 `docs/design.md` 第 5 节的记号一致,
//! 直接用于错误消息.

use std::fmt;

/// Actor 句柄的负载: 信箱消息类型与返回类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidType {
    pub mailboxes: Vec<Type>,
    pub ret: Box<Type>,
}

/// Actor 蓝图类型: 首参数必须是 Pid 类型, 其余为状态/启动参数类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerType {
    pub pid: Box<Type>,
    pub args: Vec<Type>,
}

/// 语言中的类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    I64,
    U8,
    U64,
    F64,
    Char,
    Bool,
    Str,
    /// nil 是独立类型, 仅与自身兼容.
    Nil,
    /// 底类型: 不返回的表达式, 如 `@Exit[]`, 的类型; 与任何类型兼容,
    /// 取类型时让位于另一侧.
    Never,
    List(Box<Type>),
    Dict(Box<Type>, Box<Type>),
    Set(Box<Type>),
    Pair(Box<Type>, Box<Type>),
    OptionT(Box<Type>),
    ResultT(Box<Type>, Box<Type>),
    /// Actor 句柄: 各信箱消息类型加返回类型.
    PidT(Box<PidType>),
    /// Actor 蓝图.
    HandlerT(Box<HandlerType>),
    Fn(Vec<Type>, Box<Type>),
    /// Data 类型, 以驻留名区分; 内置 Data 与用户定义同形.
    Data(crate::symbol::Symbol),
    /// 不透明文件句柄.
    File,
    /// 不透明时间值: 绝对时刻加时区.
    DateTime,
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", super::display::TypeFormatter::new(self, None))
    }
}
