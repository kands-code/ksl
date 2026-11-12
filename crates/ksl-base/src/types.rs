//! 类型表示.
//!
//! 类型系统的全部形态.
//! 显示形态与 `docs/topics/language/types.md` 的记号一致, 用于错误消息.

use std::fmt;
use std::sync::Arc;

use crate::name::Name;

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

/// 定义一套类型形态; `Type` 与 `InferTy` 共用, 仅推断期额外带 `Var`.
///
/// 宏展开处需要 `Arc` 与 `Name` 在作用域内.
#[macro_export]
macro_rules! type_shape {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident { $($extra:tt)* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis enum $name {
            I64,
            I32,
            U8,
            U32,
            U64,
            F32,
            F64,
            Char,
            Bool,
            Str,
            Nil,
            Never,
            Fn(Arc<[$name]>, Arc<$name>),
            List(Arc<$name>),
            Dict(Arc<$name>, Arc<$name>),
            Set(Arc<$name>),
            Pair(Arc<$name>, Arc<$name>),
            Option(Arc<$name>),
            Result(Arc<$name>, Arc<$name>),
            Pid {
                mailboxes: Arc<[$name]>,
                ret: Arc<$name>,
            },
            Handler {
                pid: Arc<$name>,
                args: Arc<[$name]>,
            },
            Data(Name),
            Opaque(Name),
            $($extra)*
        }
    };
}

type_shape! {
    /// 类型系统的全部形态.
    pub enum Type {}
}

impl Type {
    #[must_use]
    pub fn function(params: Vec<Self>, ret: Self) -> Self {
        Self::Fn(params.into(), Arc::new(ret))
    }

    #[must_use]
    pub fn data(name: Name) -> Self {
        Self::Data(name)
    }

    #[must_use]
    pub fn opaque(name: Name) -> Self {
        Self::Opaque(name)
    }

    #[must_use]
    pub fn file() -> Self {
        Self::opaque(crate::name::keywords().file_.clone())
    }

    #[must_use]
    pub fn command_output() -> Self {
        Self::opaque(crate::name::keywords().command_output_.clone())
    }

    #[must_use]
    pub fn date_time() -> Self {
        Self::opaque(crate::name::keywords().date_time_.clone())
    }

    #[must_use]
    pub fn socket() -> Self {
        Self::opaque(crate::name::keywords().socket_.clone())
    }

    #[must_use]
    pub fn tcp_listener() -> Self {
        Self::opaque(crate::name::keywords().tcp_listener_.clone())
    }

    #[must_use]
    pub fn regex() -> Self {
        Self::opaque(crate::name::keywords().regex_.clone())
    }

    #[must_use]
    pub fn regex_match() -> Self {
        Self::opaque(crate::name::keywords().regex_match_.clone())
    }

    #[must_use]
    pub fn list(inner: Self) -> Self {
        Self::List(Arc::new(inner))
    }

    #[must_use]
    pub fn dict(key: Self, value: Self) -> Self {
        Self::Dict(Arc::new(key), Arc::new(value))
    }

    #[must_use]
    pub fn set(inner: Self) -> Self {
        Self::Set(Arc::new(inner))
    }

    #[must_use]
    pub fn pair(first: Self, second: Self) -> Self {
        Self::Pair(Arc::new(first), Arc::new(second))
    }

    #[must_use]
    pub fn option(inner: Self) -> Self {
        Self::Option(Arc::new(inner))
    }

    #[must_use]
    pub fn result(ok: Self, err: Self) -> Self {
        Self::Result(Arc::new(ok), Arc::new(err))
    }

    /// 构造 `Pid[信箱..., 返回类型]`.
    #[must_use]
    pub fn pid(mailboxes: Vec<Self>, ret: Self) -> Self {
        Self::Pid {
            mailboxes: mailboxes.into(),
            ret: Arc::new(ret),
        }
    }

    /// 构造 `Handler[Pid, 状态...]`.
    #[must_use]
    pub fn handler(pid: Self, args: Vec<Self>) -> Self {
        Self::Handler {
            pid: Arc::new(pid),
            args: args.into(),
        }
    }

    /// 是否为不可再分解的基础值类型.
    #[must_use]
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Self::I64
                | Self::I32
                | Self::U8
                | Self::U32
                | Self::U64
                | Self::F32
                | Self::F64
                | Self::Char
                | Self::Bool
                | Self::Str
                | Self::Nil
        )
    }

    /// 是否为整数类型.
    #[must_use]
    pub fn is_integer(&self) -> bool {
        matches!(self, Self::I64 | Self::I32 | Self::U8 | Self::U32 | Self::U64)
    }

    /// 是否为浮点类型.
    #[must_use]
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }

    /// 是否为文本类型.
    #[must_use]
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Str | Self::Char)
    }

    /// 取出无参数 Data 类型的名字.
    #[must_use]
    pub fn as_data(&self) -> Option<&Name> {
        match self {
            Self::Data(name) => Some(name),
            _ => None,
        }
    }

    /// 取出参数化构造器的子类型; Data 返回空, 标量与函数返回 None.
    #[must_use]
    pub fn type_args(&self) -> Option<Vec<&Self>> {
        match self {
            Self::List(inner) | Self::Set(inner) | Self::Option(inner) => Some(vec![inner]),
            Self::Dict(key, value) | Self::Pair(key, value) | Self::Result(key, value) => {
                Some(vec![key, value])
            },
            Self::Pid { mailboxes, ret } => {
                let mut args: Vec<&Self> = mailboxes.iter().collect();
                args.push(ret);
                Some(args)
            },
            Self::Handler { pid, args } => {
                let mut all = Vec::with_capacity(args.len() + 1);
                all.push(&**pid);
                all.extend(args.iter());
                Some(all)
            },
            Self::Data(_) => Some(Vec::new()),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_list(&self) -> Option<&Self> {
        match self {
            Self::List(inner) => Some(inner),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_dict(&self) -> Option<(&Self, &Self)> {
        match self {
            Self::Dict(key, value) => Some((key, value)),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_set(&self) -> Option<&Self> {
        match self {
            Self::Set(inner) => Some(inner),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_pair(&self) -> Option<(&Self, &Self)> {
        match self {
            Self::Pair(first, second) => Some((first, second)),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_option(&self) -> Option<&Self> {
        match self {
            Self::Option(inner) => Some(inner),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_result(&self) -> Option<(&Self, &Self)> {
        match self {
            Self::Result(ok, err) => Some((ok, err)),
            _ => None,
        }
    }

    /// 取出 `Pid` 的信箱类型与返回类型. 信箱类型数为 1 至 16.
    #[must_use]
    pub fn as_pid(&self) -> Option<(&[Self], &Self)> {
        let Self::Pid { mailboxes, ret } = self else {
            return None;
        };
        if !(1..=16).contains(&mailboxes.len()) {
            return None;
        }
        Some((mailboxes, ret))
    }

    #[must_use]
    pub fn as_handler(&self) -> Option<(&Self, &[Self])> {
        let Self::Handler { pid, args } = self else {
            return None;
        };
        pid.as_pid()?;
        Some((pid, args))
    }

    #[must_use]
    pub fn is_pid(&self) -> bool {
        self.as_pid().is_some()
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", crate::display::TypeFormatter::new(self, None))
    }
}
