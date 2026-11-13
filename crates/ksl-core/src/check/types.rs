//! 类型表示.
//!
//! 类型系统的全部形态; 显示形态与 `docs/design.md` 第 5 节的记号一致,
//! 直接用于错误消息.

use std::fmt;
use std::sync::{Arc, LazyLock};

use crate::symbol::{Symbol, keywords};

/// Actor 句柄的负载: 信箱消息类型与返回类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidType {
    pub mailboxes: Vec<Type>,
    pub ret: Box<Type>,
}

impl PidType {
    /// 从 `Pid[...]` 类型取出信箱与返回类型; 形态不符时返回 None.
    pub(crate) fn from_type(ty: &Type) -> Option<Self> {
        let (mailboxes, ret) = ty.as_pid()?;
        Some(Self {
            mailboxes: mailboxes.to_vec(),
            ret: Box::new(ret.clone()),
        })
    }
}

/// Actor 蓝图类型: 首参数必须是 Pid 类型, 其余为状态/启动参数类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerType {
    pub pid: Box<Type>,
    pub args: Vec<Type>,
}

impl HandlerType {
    /// 构造对应的 Handler 类型.
    pub(crate) fn to_type(&self) -> Type {
        Type::handler((*self.pid).clone(), self.args.clone())
    }
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
    /// 函数类型; 参数与返回类型都是结构的一部分.
    Fn(Vec<Type>, Box<Type>),
    /// 参数化类型应用: 内置容器, Actor 类型与用户 Data 同形.
    ///
    /// 参数用 `Arc<[Type]>`: 类型不可变, 克隆只增加引用计数.
    App {
        name: Symbol,
        args: Arc<[Type]>,
    },
    /// 不透明句柄或值: 文件, 套接字与时间等; 以驻留名区分.
    Opaque(Symbol),
}

impl Type {
    /// 构造参数化类型应用.
    pub(crate) fn app(name: Symbol, args: Vec<Self>) -> Self {
        Self::App {
            name,
            args: if args.is_empty() {
                empty_args()
            } else {
                Arc::from(args)
            },
        }
    }

    /// 构造无参数的 Data 类型.
    pub(crate) fn data(name: Symbol) -> Self {
        Self::app(name, Vec::new())
    }

    /// 构造不透明类型.
    pub(crate) fn opaque(name: Symbol) -> Self {
        Self::Opaque(name)
    }

    /// `File` 类型.
    pub(crate) fn file() -> Self {
        Self::opaque(crate::symbol::intern("File"))
    }

    /// `DateTime` 类型.
    pub(crate) fn date_time() -> Self {
        Self::opaque(crate::symbol::intern("DateTime"))
    }

    /// `Socket` 类型.
    pub(crate) fn socket() -> Self {
        Self::opaque(crate::symbol::intern("Socket"))
    }

    /// `TcpListener` 类型.
    pub(crate) fn tcp_listener() -> Self {
        Self::opaque(crate::symbol::intern("TcpListener"))
    }

    /// 构造 `List[T]`.
    pub(crate) fn list(inner: Self) -> Self {
        Self::app(keywords().list_, vec![inner])
    }

    /// 构造 `Dict[K, V]`.
    pub(crate) fn dict(key: Self, value: Self) -> Self {
        Self::app(intern_dict(), vec![key, value])
    }

    /// 构造 `Set[T]`.
    pub(crate) fn set(inner: Self) -> Self {
        Self::app(intern_set(), vec![inner])
    }

    /// 构造 `Pair[A, B]`.
    pub(crate) fn pair(first: Self, second: Self) -> Self {
        Self::app(keywords().pair_, vec![first, second])
    }

    /// 构造 `Option[T]`.
    pub(crate) fn option(inner: Self) -> Self {
        Self::app(keywords().option_, vec![inner])
    }

    /// 构造 `Result[T, E]`.
    pub(crate) fn result(ok: Self, err: Self) -> Self {
        Self::app(keywords().result_, vec![ok, err])
    }

    /// 构造 `Pid[信箱..., 返回类型]`.
    pub(crate) fn pid(mailboxes: Vec<Self>, ret: Self) -> Self {
        let mut args = mailboxes;
        args.push(ret);
        Self::app(keywords().pid_, args)
    }

    /// 构造 `Handler[Pid, 状态...]`.
    pub(crate) fn handler(pid: Self, args: Vec<Self>) -> Self {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(pid);
        all.extend(args);
        Self::app(keywords().handler_, all)
    }

    /// 取出参数化类型应用的名字与参数.
    pub(crate) fn as_app(&self) -> Option<(Symbol, &[Self])> {
        match self {
            Self::App { name, args } => Some((*name, args)),
            _ => None,
        }
    }

    /// 取出无参数 Data 类型的名字.
    pub(crate) fn as_data(&self) -> Option<Symbol> {
        match self {
            Self::App { name, args } if args.is_empty() => Some(*name),
            _ => None,
        }
    }

    /// 取出 `List[T]` 的元素类型.
    pub(crate) fn as_list(&self) -> Option<&Self> {
        self.app_args(keywords().list_).and_then(|args| args.first())
    }

    /// 取出 `Dict[K, V]` 的键与值类型.
    pub(crate) fn as_dict(&self) -> Option<(&Self, &Self)> {
        let [key, value] = self.app_args(intern_dict())? else {
            return None;
        };
        Some((key, value))
    }

    /// 取出 `Option[T]` 的元素类型.
    pub(crate) fn as_option(&self) -> Option<&Self> {
        self.app_args(keywords().option_).and_then(|args| args.first())
    }

    /// 取出 `Pid` 的信箱类型与返回类型.
    pub(crate) fn as_pid(&self) -> Option<(&[Self], &Self)> {
        let args = self.app_args(keywords().pid_)?;
        if !(2..=17).contains(&args.len()) {
            return None;
        }
        let (ret, mailboxes) = args.split_last()?;
        Some((mailboxes, ret))
    }

    /// 取出 `Handler` 的 Pid 与状态参数.
    pub(crate) fn as_handler(&self) -> Option<(&Self, &[Self])> {
        let args = self.app_args(keywords().handler_)?;
        let (pid, rest) = args.split_first()?;
        pid.as_pid()?;
        Some((pid, rest))
    }

    /// 是否为 Pid 类型.
    pub(crate) fn is_pid(&self) -> bool {
        self.as_pid().is_some()
    }

    fn app_args(&self, name: Symbol) -> Option<&[Self]> {
        match self {
            Self::App { name: actual, args } if *actual == name => Some(args),
            _ => None,
        }
    }
}

fn empty_args() -> Arc<[Type]> {
    static EMPTY: LazyLock<Arc<[Type]>> = LazyLock::new(|| Arc::from(Vec::new()));
    Arc::clone(&EMPTY)
}

fn intern_dict() -> Symbol {
    crate::symbol::intern("Dict")
}

fn intern_set() -> Symbol {
    crate::symbol::intern("Set")
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", super::display::TypeFormatter::new(self, None))
    }
}
