//! 执行计划数据.
//!
//! 线性 IR 与执行视图共用的类型; 值表示与抽象机只依赖本模块.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::fail::FailKind;
use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::builtin::BuiltinId;
use ksl_catalog::ops::{DictOpKind, ListOpKind, OptionOpKind, PairOpKind, SetOpKind};

use crate::atom::{DataAtom, FStrPart as FStrPartGeneric, FuncAtom, ListAtom, define_atom};

/// 合成临时值编号; 显示为 `%N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TempId(u32);

impl TempId {
    /// 由编号构造; 供降级器使用.
    #[must_use]
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    /// 内部编号, 不含显示形态的 `%` 前缀.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// IR 绑定目标: 源码名, 合成临时值, 或丢弃.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Binding {
    Named(Name),
    Temp(TempId),
    Discard,
}

/// 运行时作用域键: 统一源码名与合成临时值的查找.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VarKey {
    Named(Name),
    Temp(TempId),
}

/// 带类型的绑定者.
///
/// 类型在降级期由检查器类型表填入.
/// 抽象机绑定值时忽略类型, 线性化保留它供函数签名显示.
#[derive(Debug, Clone)]
pub struct Binder {
    pub target: Binding,
    pub ty: Type,
}

impl From<Binding> for Option<VarKey> {
    fn from(binding: Binding) -> Self {
        match binding {
            Binding::Named(name) => Some(VarKey::Named(name)),
            Binding::Temp(id) => Some(VarKey::Temp(id)),
            Binding::Discard => None,
        }
    }
}

/// 运行时支持的 Option/Result 原语.
///
/// 取值类操作在降级期展开为分支, 不进入原语分派.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionPrim {
    IsSome,
    IsNone,
    IsOk,
    IsErr,
    OptionMap,
    ResultMap,
    OptionAndThen,
    ResultAndThen,
    ResultOrElse,
}

impl OptionPrim {
    /// 由语法层操作名转换; 取值类操作返回 None.
    #[must_use]
    pub fn from_kind(kind: OptionOpKind) -> Option<Self> {
        use OptionOpKind as K;
        Some(match kind {
            K::IsSome => Self::IsSome,
            K::IsNone => Self::IsNone,
            K::IsOk => Self::IsOk,
            K::IsErr => Self::IsErr,
            K::OptionMap => Self::OptionMap,
            K::ResultMap => Self::ResultMap,
            K::OptionAndThen => Self::OptionAndThen,
            K::ResultAndThen => Self::ResultAndThen,
            K::ResultOrElse => Self::ResultOrElse,
            K::OptionGet | K::ResultGet => return None,
        })
    }

    /// 操作名; IR 显示使用.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::IsSome => "IsSome",
            Self::IsNone => "IsNone",
            Self::IsOk => "IsOk",
            Self::IsErr => "IsErr",
            Self::OptionMap => "OptionMap",
            Self::ResultMap => "ResultMap",
            Self::OptionAndThen => "OptionAndThen",
            Self::ResultAndThen => "ResultAndThen",
            Self::ResultOrElse => "ResultOrElse",
        }
    }

    /// 是否直接产出结果, 不回调用户函数.
    #[must_use]
    pub fn is_synchronous(&self) -> bool {
        matches!(self, Self::IsSome | Self::IsNone | Self::IsOk | Self::IsErr)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimOp {
    List(ListOpKind),
    Pair(PairOpKind),
    /// 读取单变体 Data 的字段; 下标由检查阶段确定.
    DataGet {
        index: u8,
    },
    Option(OptionPrim),
    Dict(DictOpKind),
    Set(SetOpKind),
    DictLiteral,
    SetLiteral,
    Spawn {
        detached: bool,
        mailboxes: usize,
    },
    Send,
    Join,
    Kill,
    PidSerial,
    SetTimer,
    CancelTimer,
    Sleep,
}

impl PrimOp {
    /// 原语是否同步产出值; 需要回调用户函数或调度器效果的原语返回 false.
    #[must_use]
    pub fn is_synchronous(&self) -> bool {
        match self {
            Self::List(op) => !op.is_higher_order(),
            Self::Pair(_)
            | Self::DataGet { .. }
            | Self::DictLiteral
            | Self::SetLiteral
            | Self::Send
            | Self::PidSerial => true,
            Self::Dict(op) => op.is_synchronous(),
            Self::Set(op) => op.is_synchronous(),
            Self::Option(op) => op.is_synchronous(),
            Self::Spawn { .. }
            | Self::Join
            | Self::Kill
            | Self::SetTimer
            | Self::CancelTimer
            | Self::Sleep => false,
        }
    }
}

#[derive(Debug, Clone)]
pub enum PatternIr {
    Wildcard,
    /// 构造器模式; 字段绑定为 None 表示 `_`.
    Variant {
        qualifier: Option<Name>,
        name: Name,
        fields: Vec<Option<Binder>>,
    },
}

/// 函数内唯一的变量身份; 由线性化器分配.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarId(u32);

impl VarId {
    /// 由编号构造; 供线性化器与优化器使用.
    #[must_use]
    pub fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// 内部编号.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// 变量引用: 未解析的变量身份, 当前块槽位, 闭包捕获下标或未绑定源码名.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VarRef {
    /// 未解析的函数内变量身份; 只出现在线性化中间态.
    Id(VarId),
    /// 当前块的槽位.
    Local(u32),
    /// 函数闭包的捕获下标.
    Capture(u32),
    /// 未在本函数内绑定的源码名; 解析后变为 `Capture`.
    ///
    /// 装箱避免名字宽度撑大每个变量引用; 该形态只出现在线性化中间态.
    Free(Box<Name>),
}

/// 基本块编号; 在同一函数内唯一.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(u32);

impl BlockId {
    /// 由编号构造; 供线性化器与优化器使用.
    #[must_use]
    pub fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// 内部编号.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

/// 不改变控制流的指令.
#[derive(Debug, Clone)]
pub enum Instr {
    Let {
        slot: u32,
        value: Operand,
    },
    /// 同步求值的纯内置调用; 结果写入槽位.
    Builtin {
        slot: u32,
        callee: BuiltinId,
        args: Vec<Operand>,
        span: Span,
    },
    /// 同步求值的原语; 结果写入槽位.
    ///
    /// `dead` 列出该指令执行后不再被读取的槽位, 运行时在调用原语前清空.
    Prim {
        slot: u32,
        op: PrimOp,
        args: Vec<Operand>,
        ty: Type,
        span: Span,
        dead: Vec<u32>,
    },
    /// 顶层绑定: 写入槽位与全局环境, 或只求值后丢弃.
    GlobalLet {
        target: GlobalTarget,
        ty: Type,
        value: Operand,
    },
    LetRec {
        bindings: Vec<RecBinding>,
    },
    GlobalLetRec {
        bindings: Vec<RecBinding>,
    },
}

/// 顶层绑定的写入目标.
#[derive(Debug, Clone)]
pub enum GlobalTarget {
    /// 只求值, 结果丢弃.
    Discard,
    /// 同时写入本地槽位与全局环境.
    Named { slot: u32, name: Name },
}

/// 递归绑定: 名字槽位, 函数与捕获值.
#[derive(Debug, Clone)]
pub struct RecBinding {
    pub name: Name,
    pub slot: u32,
    pub function: Name,
    pub captures: Vec<Operand>,
}

/// 终止指令; 每个基本块恰好一条.
#[derive(Debug, Clone)]
pub enum Terminator {
    /// 返回调用方.
    Return {
        value: Operand,
    },
    /// 跳转到同一函数内的基本块, 参数包括该块全部形参.
    Jump {
        target: BlockId,
        args: Vec<Operand>,
    },
    TailCall {
        callee: Operand,
        args: Vec<Operand>,
        span: Span,
    },
    Call {
        callee: Operand,
        args: Vec<Operand>,
        target: BlockId,
        /// 目标块除结果参数外的其余参数; 结果由被调函数返回时填入.
        live: Vec<Operand>,
        span: Span,
    },
    Prim {
        op: PrimOp,
        args: Vec<Operand>,
        ty: Type,
        target: BlockId,
        live: Vec<Operand>,
        span: Span,
    },
    Receive {
        index: usize,
        timeout: Operand,
        target: BlockId,
        live: Vec<Operand>,
        span: Span,
    },
    If {
        cond: Operand,
        then: Target,
        els: Target,
    },
    Match {
        value: Operand,
        arms: Vec<MatchArm>,
    },
    Continue {
        args: Vec<Operand>,
    },
    Exit {
        value: Operand,
    },
    Halt,
    /// 用户主动中断: 断言失败, 主动中止, 尚未实现或不该到达的分支.
    Fail {
        kind: FailKind,
        message: Operand,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct Target {
    pub block: BlockId,
    /// 目标块的参数, 与 `Block::param_count` 一致.
    pub args: Vec<Operand>,
}

/// 匹配分支: 模式绑定目标块的首批参数, `live` 提供其余参数.
#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: PatternIr,
    pub target: BlockId,
    pub live: Vec<Operand>,
}

define_atom!(
    Operand,
    FStrPartGeneric<Operand>,
    Var(VarRef),
    /// 模块内函数引用.
    Func(Box<FuncAtom<Operand>>),
);

pub type FStrPart = FStrPartGeneric<Operand>;

/// 执行器输入: 函数表, 主函数与顶层捕获名.
#[derive(Debug)]
pub struct Program {
    /// 模块内函数; 按名字索引, 供 `Operand::Func` 建闭包与递归绑定查找.
    pub functions: Arc<Functions>,
    /// 顶层语句体.
    pub main: Arc<Function>,
    /// 顶层函数需要从全局环境播种的捕获.
    pub main_free_vars: Box<[Name]>,
}

/// 运行时函数表.
pub type Functions = HashMap<Name, Arc<Function>>;

/// 执行函数: 入口块与基本块.
#[derive(Debug)]
pub struct Function {
    pub entry: BlockId,
    pub blocks: Box<[Block]>,
}

/// 基本块: 参数个数, 槽位个数, 指令与终止指令.
#[derive(Debug)]
pub struct Block {
    pub param_count: usize,
    pub slot_count: usize,
    pub instrs: Box<[Instr]>,
    pub term: Terminator,
}

impl fmt::Display for TempId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

impl fmt::Display for VarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b{}", self.0)
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => write!(f, "{name}"),
            Self::Temp(id) => write!(f, "{id}"),
            Self::Discard => f.write_str("_"),
        }
    }
}

impl fmt::Display for Binder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} : {}", self.target, self.ty)
    }
}

impl fmt::Display for PrimOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List(op) => f.write_str(op.name()),
            Self::Pair(op) => f.write_str(op.name()),
            Self::DataGet { index } => write!(f, "DataGet[{index}]"),
            Self::Option(op) => f.write_str(op.clone().name()),
            Self::Dict(op) => f.write_str(op.name()),
            Self::Set(op) => f.write_str(op.name()),
            Self::DictLiteral => f.write_str("DictLiteral"),
            Self::SetLiteral => f.write_str("SetLiteral"),
            Self::Spawn {
                detached: false, ..
            } => f.write_str("Spawn"),
            Self::Spawn { detached: true, .. } => f.write_str("SpawnDetached"),
            Self::Send => f.write_str("Send"),
            Self::Join => f.write_str("Join"),
            Self::Kill => f.write_str("Kill"),
            Self::PidSerial => f.write_str("PidSerial"),
            Self::SetTimer => f.write_str("SetTimer"),
            Self::CancelTimer => f.write_str("CancelTimer"),
            Self::Sleep => f.write_str("Sleep"),
        }
    }
}

impl fmt::Display for PatternIr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wildcard => write!(f, "_"),
            Self::Variant {
                qualifier,
                name,
                fields,
            } => {
                if let Some(qualifier) = qualifier {
                    write!(f, "{qualifier}/")?;
                }
                write!(f, "{name}[")?;
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    match field {
                        Some(binder) => write!(f, "{binder}")?,
                        None => write!(f, "_")?,
                    }
                }
                write!(f, "]")
            },
        }
    }
}
