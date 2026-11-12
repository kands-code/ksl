//! 类型表示.
//!
//! 类型系统的全部形态.
//! 显示形态与 `docs/topics/language/types.md` 的记号一致, 直接用于错误消息.

use std::fmt;
use std::sync::{Arc, LazyLock};

use crate::symbol::{Symbol, keywords};

/// Pid 类型的负载: 信箱消息类型与返回类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PidType {
    pub mailboxes: Vec<Type>,
    pub ret: Box<Type>,
}

impl PidType {
    /// 从 `Pid[...]` 类型取出信箱与返回类型; 形态不符时返回 None.
    #[must_use]
    pub fn from_type(ty: &Type) -> Option<Self> {
        let (mailboxes, ret) = ty.as_pid()?;
        Some(Self {
            mailboxes: mailboxes.to_vec(),
            ret: Box::new(ret.clone()),
        })
    }
}

/// Actor 蓝图类型: 首参数必须是 Pid 类型, 其余为状态与启动参数类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerType {
    pub pid: Box<Type>,
    pub args: Vec<Type>,
}

impl HandlerType {
    #[must_use]
    pub fn to_type(&self) -> Type {
        Type::handler((*self.pid).clone(), self.args.clone())
    }
}

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
    /// 底类型: 不返回的表达式的类型, 如 `@Exit[]`. 与任何类型兼容,
    /// 合并两侧类型时采用另一侧的类型.
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
    /// 构造类型构造器应用; 无参数时共用空参数切片.
    #[must_use]
    pub fn app(name: Symbol, args: Vec<Self>) -> Self {
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
    #[must_use]
    pub fn data(name: Symbol) -> Self {
        Self::app(name, Vec::new())
    }

    #[must_use]
    pub fn opaque(name: Symbol) -> Self {
        Self::Opaque(name)
    }

    #[must_use]
    pub fn file() -> Self {
        Self::opaque(crate::symbol::keywords().file_)
    }

    #[must_use]
    pub fn command_output() -> Self {
        Self::opaque(crate::symbol::keywords().command_output_)
    }

    #[must_use]
    pub fn date_time() -> Self {
        Self::opaque(crate::symbol::keywords().date_time_)
    }

    #[must_use]
    pub fn socket() -> Self {
        Self::opaque(crate::symbol::keywords().socket_)
    }

    #[must_use]
    pub fn tcp_listener() -> Self {
        Self::opaque(crate::symbol::keywords().tcp_listener_)
    }

    #[must_use]
    pub fn regex() -> Self {
        Self::opaque(crate::symbol::keywords().regex_)
    }

    #[must_use]
    pub fn regex_match() -> Self {
        Self::opaque(crate::symbol::keywords().regex_match_)
    }

    /// 构造 `List[T]`.
    #[must_use]
    pub fn list(inner: Self) -> Self {
        Self::app(keywords().list_, vec![inner])
    }

    /// 构造 `Dict[K, V]`.
    #[must_use]
    pub fn dict(key: Self, value: Self) -> Self {
        Self::app(keywords().dict_, vec![key, value])
    }

    /// 构造 `Set[T]`.
    #[must_use]
    pub fn set(inner: Self) -> Self {
        Self::app(keywords().set_, vec![inner])
    }

    /// 构造 `Pair[A, B]`.
    #[must_use]
    pub fn pair(first: Self, second: Self) -> Self {
        Self::app(keywords().pair_, vec![first, second])
    }

    /// 构造 `Option[T]`.
    #[must_use]
    pub fn option(inner: Self) -> Self {
        Self::app(keywords().option_, vec![inner])
    }

    /// 构造 `Result[T, E]`.
    #[must_use]
    pub fn result(ok: Self, err: Self) -> Self {
        Self::app(keywords().result_, vec![ok, err])
    }

    /// 构造 `Pid[信箱..., 返回类型]`.
    #[must_use]
    pub fn pid(mailboxes: Vec<Self>, ret: Self) -> Self {
        let mut args = mailboxes;
        args.push(ret);
        Self::app(keywords().pid_, args)
    }

    /// 构造 `Handler[Pid, 状态...]`.
    #[must_use]
    pub fn handler(pid: Self, args: Vec<Self>) -> Self {
        let mut all = Vec::with_capacity(args.len() + 1);
        all.push(pid);
        all.extend(args);
        Self::app(keywords().handler_, all)
    }

    /// 取出参数化类型应用的名字与参数.
    #[must_use]
    pub fn as_app(&self) -> Option<(Symbol, &[Self])> {
        match self {
            Self::App { name, args } => Some((*name, args)),
            _ => None,
        }
    }

    /// 取出无参数 Data 类型的名字.
    #[must_use]
    pub fn as_data(&self) -> Option<Symbol> {
        match self {
            Self::App { name, args } if args.is_empty() => Some(*name),
            _ => None,
        }
    }

    /// 取出 `List[T]` 的元素类型.
    #[must_use]
    pub fn as_list(&self) -> Option<&Self> {
        self.app_args(keywords().list_).and_then(|args| args.first())
    }

    /// 取出 `Dict[K, V]` 的键与值类型.
    #[must_use]
    pub fn as_dict(&self) -> Option<(&Self, &Self)> {
        let [key, value] = self.app_args(keywords().dict_)? else {
            return None;
        };
        Some((key, value))
    }

    /// 取出 `Option[T]` 的元素类型.
    #[must_use]
    pub fn as_option(&self) -> Option<&Self> {
        self.app_args(keywords().option_).and_then(|args| args.first())
    }

    /// 取出 `Pid` 的信箱类型与返回类型. 信箱类型数为 1 至 16.
    #[must_use]
    pub fn as_pid(&self) -> Option<(&[Self], &Self)> {
        let args = self.app_args(keywords().pid_)?;
        if !(2..=17).contains(&args.len()) {
            return None;
        }
        let (ret, mailboxes) = args.split_last()?;
        Some((mailboxes, ret))
    }

    /// 取出 `Handler` 的 Pid 与状态参数.
    #[must_use]
    pub fn as_handler(&self) -> Option<(&Self, &[Self])> {
        let args = self.app_args(keywords().handler_)?;
        let (pid, rest) = args.split_first()?;
        pid.as_pid()?;
        Some((pid, rest))
    }

    #[must_use]
    pub fn is_pid(&self) -> bool {
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

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", crate::display::TypeFormatter::new(self, None))
    }
}
