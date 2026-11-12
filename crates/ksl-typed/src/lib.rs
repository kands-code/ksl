//! 带类型的 AST 接口.
//!
//! 由检查器在检查过程中按节点构建, 检查结束时把推断类型解析为具体类型.
//! 类型直接挂在表达式与绑定目标上, 供降级阶段读取.
//! 本层只定义数据结构, 不依赖检查器, 语法分析或运行时.

use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::builtin::BuiltinId;
use ksl_catalog::ops::{DictOpKind, ListOpKind, OptionOpKind, PairOpKind, SetOpKind};

/// 程序的顶层结构.
#[derive(Debug, Clone)]
pub struct Program<T> {
    pub stmts: Vec<Expr<T>>,
    pub tail: Option<Expr<T>>,
    /// 模块入口声明; 依赖模块只检查, 根模块执行.
    pub main: Option<Expr<T>>,
}

/// 带类型的表达式.
#[derive(Debug, Clone)]
pub struct Expr<T> {
    pub kind: ExprKind<T>,
    pub span: Span,
    pub ty: T,
}

/// 表达式形态.
#[derive(Debug, Clone)]
pub enum ExprKind<T> {
    I64(i64),
    I32(i32),
    U8(u8),
    U32(u32),
    U64(u64),
    F32(f32),
    F64(f64),
    Char(char),
    Str(String),
    Bool(bool),
    Nil,
    Ident(Name),
    List(Vec<Expr<T>>),
    Pair(Box<Expr<T>>, Box<Expr<T>>),
    FStr(Vec<FStrPart<T>>),
    Call {
        callee: Callee,
        args: Vec<Expr<T>>,
    },
    Apply {
        callee: Box<Expr<T>>,
        args: Vec<Expr<T>>,
    },
    Which {
        branches: Vec<(Expr<T>, Expr<T>)>,
        fallback: Box<Expr<T>>,
    },
    ListOp {
        op: ListOpKind,
        args: Vec<Expr<T>>,
    },
    PairOp {
        op: PairOpKind,
        args: Vec<Expr<T>>,
    },
    /// 读取单变体 Data 的字段.
    DataGet {
        value: Box<Expr<T>>,
        index: u8,
    },
    DictOp {
        op: DictOpKind,
        args: Vec<Expr<T>>,
    },
    SetOp {
        op: SetOpKind,
        args: Vec<Expr<T>>,
    },
    OptionOp {
        op: OptionOpKind,
        args: Vec<Expr<T>>,
    },
    DictLiteral(Vec<Expr<T>>),
    SetLiteral(Vec<Expr<T>>),
    Assert {
        cond: Box<Expr<T>>,
        message: Box<Expr<T>>,
    },
    Panic {
        message: Box<Expr<T>>,
    },
    Todo {
        message: Box<Expr<T>>,
    },
    Unreachable {
        message: Box<Expr<T>>,
    },
    Fn {
        params: Vec<Binder<T>>,
        ret: T,
        body: Box<Expr<T>>,
    },
    Def(Def<T>),
    Fix(Vec<Def<T>>),
    Data(DataDecl<T>),
    Link,
    Match {
        value: Box<Expr<T>>,
        arms: Vec<MatchArm<T>>,
    },
    Handler {
        params: Vec<Binder<T>>,
        ret: T,
        body: Box<Expr<T>>,
    },
    Main {
        params: Vec<Binder<T>>,
        ret: T,
        body: Box<Expr<T>>,
    },
    Spawn {
        handler: Box<Expr<T>>,
        args: Vec<Expr<T>>,
        detached: bool,
    },
    Send {
        pid: Box<Expr<T>>,
        index: u8,
        message: Box<Expr<T>>,
    },
    Receive {
        index: u8,
        timeout: Box<Expr<T>>,
    },
    Join {
        pid: Box<Expr<T>>,
    },
    Kill {
        pid: Box<Expr<T>>,
    },
    PidSerial {
        pid: Box<Expr<T>>,
    },
    SetTimer {
        pid: Box<Expr<T>>,
        index: u8,
        message: Box<Expr<T>>,
        millis: Box<Expr<T>>,
    },
    CancelTimer {
        id: Box<Expr<T>>,
    },
    Halt,
    Sleep {
        millis: Box<Expr<T>>,
    },
    Continue {
        args: Vec<Expr<T>>,
    },
    Exit {
        code: Box<Expr<T>>,
    },
    Let {
        target: Binder<T>,
        value: Box<Expr<T>>,
    },
    Block(Vec<Expr<T>>),
    If {
        cond: Box<Expr<T>>,
        then: Box<Expr<T>>,
        els: Box<Expr<T>>,
    },
}

/// 调用目标: 纯内置函数, 源码名字或 Data 构造器.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callee {
    Builtin(BuiltinId),
    Named(Name),
    Data { ty: Name, variant: Name },
}

/// 具名函数定义; 名字在定义作用域内可见.
#[derive(Debug, Clone)]
pub struct Def<T> {
    pub name: Name,
    pub params: Vec<Binder<T>>,
    pub ret: T,
    pub body: Box<Expr<T>>,
}

/// Data 类型声明.
#[derive(Debug, Clone)]
pub struct DataDecl<T> {
    pub name: Name,
    pub variants: Vec<VariantDecl<T>>,
}

/// Data 变体声明; 字段只有类型.
#[derive(Debug, Clone)]
pub struct VariantDecl<T> {
    pub name: Name,
    pub fields: Vec<T>,
}

/// `@Match` 的一个分支.
#[derive(Debug, Clone)]
pub struct MatchArm<T> {
    pub pattern: Pattern<T>,
    pub body: Box<Expr<T>>,
}

/// 模式: 通配或构造器.
#[derive(Debug, Clone)]
pub enum Pattern<T> {
    Wildcard,
    Variant {
        /// 源码写明限定名时记录的真实类型身份.
        qualifier: Option<Name>,
        name: Name,
        fields: Vec<Option<Binder<T>>>,
    },
}

/// 绑定目标: 名字或丢弃, 类型已解析.
#[derive(Debug, Clone)]
pub struct Binder<T> {
    pub name: Option<Name>,
    pub ty: T,
}

/// 格式化字符串片段; 插值已带类型.
#[derive(Debug, Clone)]
pub enum FStrPart<T> {
    Text(String),
    Interp {
        value: Box<Expr<T>>,
        spec: Option<String>,
    },
}

/// 已解析类型的程序; 降级阶段的输入.
pub type CheckedProgram = Program<Type>;

/// 把单个表达式包装为程序入口; 用于求值 `@Main` 这类不作为值参与语句的表达式.
#[must_use]
pub fn entry_program(tail: CheckedExpr) -> CheckedProgram {
    Program {
        stmts: Vec::new(),
        tail: Some(tail),
        main: None,
    }
}

/// `@Main` 的入口程序与 self 信箱数.
#[derive(Debug, Clone)]
pub struct MainEntry {
    /// 单独求值入口声明得到的程序.
    pub program: CheckedProgram,
    /// `@Main` 的 self Pid 信箱数.
    pub mailboxes: usize,
}

impl Program<Type> {
    /// 取 `@Main` 的入口程序与 self 信箱数; 无入口或签名无效时为 None.
    #[must_use]
    pub fn main_entry(&self) -> Option<MainEntry> {
        let main = self.main.as_ref()?;
        let (pid, _) = main.ty.as_handler()?;
        let (mailboxes, _) = pid.as_pid()?;
        Some(MainEntry {
            program: entry_program(main.clone()),
            mailboxes: mailboxes.len(),
        })
    }
}
/// 已解析类型的表达式.
pub type CheckedExpr = Expr<Type>;
/// 已解析类型的绑定目标.
pub type CheckedBinder = Binder<Type>;
/// 已解析类型的具名函数定义.
pub type CheckedDef = Def<Type>;
/// 已解析类型的 Data 声明.
pub type CheckedDataDecl = DataDecl<Type>;
/// 已解析类型的 Data 变体.
pub type CheckedVariantDecl = VariantDecl<Type>;
/// 已解析类型的匹配分支.
pub type CheckedMatchArm = MatchArm<Type>;
/// 已解析类型的模式.
pub type CheckedPattern = Pattern<Type>;
