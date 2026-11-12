//! CPS IR 的数据模型.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use crate::ast::{FailKind, ListOpKind, OptionOpKind};
use crate::builtin::BuiltinId;
use crate::check::Type;
use crate::cps::atom::{FStrPart as FStrPartGeneric, define_atom};
use crate::source::Span;
use crate::symbol::Symbol;

/// 降级后的模块.
#[derive(Debug, Clone)]
pub struct Module {
    pub data: Vec<DataDecl>,
    pub functions: Vec<Function>,
    pub main: Function,
}

/// 函数表: 函数名到 IR 函数.
pub type Functions = HashMap<Symbol, Arc<Function>>;

/// `@Data` 声明; 类型已在检查阶段校验, 这里只保留显示所需信息.
#[derive(Debug, Clone)]
pub struct DataDecl {
    pub name: Symbol,
    pub variants: Vec<VariantDecl>,
}

/// Data 变体声明.
#[derive(Debug, Clone)]
pub struct VariantDecl {
    pub name: Symbol,
    /// 字段类型; 由检查器类型表填入.
    pub fields: Vec<Type>,
}

/// 合成临时值编号; 显示为 `%N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TempId(pub(crate) u32);

impl TempId {
    /// 内部编号, 不含显示形态的 `%` 前缀.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Display for TempId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

/// 合成续延编号; `RETURN` 显示为 `$k`, 其余显示为 `$kN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContId(pub(crate) u32);

impl ContId {
    /// 函数返回续延; 跳转到此即返回.
    pub const RETURN: Self = Self(0);

    /// 内部编号, 不含显示形态的 `$k` 前缀.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ContId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Self::RETURN {
            f.write_str("$k")
        } else {
            write!(f, "$k{}", self.0 - 1)
        }
    }
}

/// IR 绑定目标: 源码名, 合成临时值, 或丢弃.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    Named(Symbol),
    Temp(TempId),
    Discard,
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

/// 运行时作用域键: 统一源码名与合成临时值的查找.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarKey {
    Named(Symbol),
    Temp(TempId),
}

/// 带类型的绑定者.
///
/// 类型在降级期由检查器类型表填入; 抽象机绑定值时忽略类型, 后续分档与
/// 代码生成读取它.
#[derive(Debug, Clone)]
pub struct Binder {
    pub target: Binding,
    pub ty: Type,
}

impl fmt::Display for Binder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} : {}", self.target, self.ty)
    }
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

/// 内置原语.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimOp {
    List(ListOpKind),
    Pair(crate::ast::PairOpKind),
    Option(OptionOpKind),
    Dict(crate::ast::DictOpKind),
    Set(crate::ast::SetOpKind),
    DictLiteral,
    SetLiteral,
    Registry(crate::ast::RegistryOp),
    Spawn { detached: bool },
    Send,
    Join,
    Kill,
    Monitor,
    Unmonitor,
    PidSerial,
    SetTimer,
    CancelTimer,
    Sleep,
}

/// 函数种类; Handler 用 `handler` 关键字显示.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionKind {
    Fn,
    Handler,
}

/// 函数级 CPS 函数; `cont` 通常是 `$k`.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: Symbol,
    pub kind: FunctionKind,
    pub params: Vec<Binder>,
    pub ret: Type,
    pub cont: ContId,
    pub body: Arc<Cps>,
    /// 具名函数显示用名字; 匿名函数与 Handler 为 None.
    pub display_name: Option<Symbol>,
}

/// 续延: 参数列表与续延体.
#[derive(Debug, Clone)]
pub struct Cont {
    pub id: ContId,
    pub params: Vec<Binder>,
    pub body: Arc<Cps>,
}

/// CPS 项.
#[derive(Debug, Clone)]
pub enum Cps {
    /// 把值交给续延; 目标为函数自身 `$k` 时即返回.
    Jump { cont: ContId, value: Value },
    /// 尾调用: 复用当前续延.
    TailCall {
        callee: Value,
        args: Vec<Value>,
        span: Span,
    },
    /// 普通调用: 新建续延.
    Call {
        callee: Value,
        args: Vec<Value>,
        cont: Arc<Cont>,
        span: Span,
    },
    /// 内置原语: 列表/Option 操作与 Actor 操作.
    ///
    /// `arg_types` 与 `ty` 由检查器类型表填入; 抽象机忽略它们, 分档与代码
    /// 生成据此选择具体表示.
    Prim {
        op: PrimOp,
        args: Vec<Value>,
        arg_types: Vec<Type>,
        ty: Type,
        cont: Arc<Cont>,
        span: Span,
    },
    /// 接收消息; 挂起后把 `Option[消息]` 交给续延.
    Receive {
        index: usize,
        timeout: Value,
        cont: Arc<Cont>,
        span: Span,
    },
    /// 纯值绑定.
    Let {
        target: Binder,
        value: Value,
        body: Arc<Cps>,
    },
    /// 顶层绑定: 同时更新全局环境, 尾调用后仍可被后续输入看到.
    GlobalLet {
        name: Option<Symbol>,
        ty: Type,
        value: Value,
        body: Arc<Cps>,
    },
    /// 递归函数绑定; 同一组内的函数共享递归环境.
    LetRec {
        bindings: Vec<(Symbol, Symbol)>,
        body: Arc<Cps>,
    },
    /// 顶层递归函数绑定: 同时更新全局环境.
    GlobalLetRec {
        bindings: Vec<(Symbol, Symbol)>,
        body: Arc<Cps>,
    },
    /// 绑定一个命名续延, 供分支共享.
    LetCont { cont: Arc<Cont>, body: Arc<Cps> },
    /// 条件分支.
    If {
        cond: Value,
        then: Arc<Cps>,
        els: Arc<Cps>,
    },
    /// 模式匹配.
    Match { value: Value, arms: Vec<MatchArmIr> },
    /// Handler 自循环.
    Continue { args: Vec<Value> },
    /// 顶层退出.
    Exit(Value),
    /// Handler 主动停机.
    Halt,
    /// 用户主动中断: 断言失败, 主动中止, 尚未实现或不该到达的分支.
    ///
    /// 信息字符串已求值; 该节点不返回, 抽象机在此终止当前任务.
    Fail {
        kind: FailKind,
        message: Value,
        span: Span,
    },
}

/// 释放长 CPS 链时使用的占位节点.
///
/// 释放过程把子节点搬到显式栈上; 搬空的字段需要保持结构合法, 共享的
/// 静态占位值不参与所有权转移.
static HALT_CPS: LazyLock<Arc<Cps>> = LazyLock::new(|| Arc::new(Cps::Halt));

/// 续延字段的占位值; 续延体在释放时被搬到显式栈.
static HALT_CONT: LazyLock<Arc<Cont>> = LazyLock::new(|| {
    Arc::new(Cont {
        id: ContId::RETURN,
        params: Vec::new(),
        body: Arc::clone(&HALT_CPS),
    })
});

impl Drop for Cps {
    fn drop(&mut self) {
        // CPS 树按语句序列嵌套; 直接递归释放长链会耗尽调用栈.
        // 先把直接子节点搬到显式栈上, 再逐个展开, 共享节点留给最后的
        // 持有者释放.
        let mut pending = Vec::new();
        take_cps_children(self, &mut pending);
        while let Some(mut cps) = pending.pop() {
            take_cps_children(&mut cps, &mut pending);
            // 子节点已搬到 `pending`; 此处的 `cps` 只剩占位值与载荷.
        }
    }
}

/// 把直接子节点搬到 `pending`, 只展开唯一持有的 `Arc`.
fn take_cps_children(cps: &mut Cps, pending: &mut Vec<Cps>) {
    match cps {
        Cps::Call { cont, .. } | Cps::Prim { cont, .. } | Cps::Receive { cont, .. } => {
            take_cont(cont, pending);
        },
        Cps::Let { body, .. }
        | Cps::GlobalLet { body, .. }
        | Cps::LetRec { body, .. }
        | Cps::GlobalLetRec { body, .. } => take_cps(body, pending),
        Cps::LetCont { cont, body } => {
            take_cont(cont, pending);
            take_cps(body, pending);
        },
        Cps::If { then, els, .. } => {
            take_cps(then, pending);
            take_cps(els, pending);
        },
        Cps::Match { arms, .. } => {
            for arm in arms {
                take_cps(&mut arm.body, pending);
            }
        },
        Cps::Jump { .. }
        | Cps::TailCall { .. }
        | Cps::Continue { .. }
        | Cps::Exit(_)
        | Cps::Halt
        | Cps::Fail { .. } => {},
    }
}

fn take_cps(arc: &mut Arc<Cps>, pending: &mut Vec<Cps>) {
    let child = std::mem::replace(arc, Arc::clone(&*HALT_CPS));
    if let Ok(cps) = Arc::try_unwrap(child) {
        pending.push(cps);
    }
}

fn take_cont(arc: &mut Arc<Cont>, pending: &mut Vec<Cps>) {
    let cont = std::mem::replace(arc, Arc::clone(&*HALT_CONT));
    if let Ok(mut cont) = Arc::try_unwrap(cont) {
        take_cps(&mut cont.body, pending);
    }
}

/// 匹配分支.
#[derive(Debug, Clone)]
pub struct MatchArmIr {
    pub pattern: PatternIr,
    pub body: Arc<Cps>,
}

/// 模式.
#[derive(Debug, Clone)]
pub enum PatternIr {
    /// 通配模式.
    Wildcard,
    /// 构造器模式; 字段绑定为 None 表示 `_`.
    Variant {
        qualifier: Option<Symbol>,
        name: Symbol,
        fields: Vec<Option<Binder>>,
    },
}

define_atom!(
    Value,
    FStrPartGeneric<Value>,
    /// 源码变量名.
    Var(Symbol),
    /// 合成临时值.
    Temp(TempId),
    /// 模块内函数引用.
    Func(Symbol),
);

/// CPS 格式化字符串片段.
pub type FStrPartIr = FStrPartGeneric<Value>;

impl Module {
    /// 拆出函数表与主函数; Data 声明不参与运行时.
    #[must_use]
    pub fn into_parts(self) -> (Arc<Functions>, Arc<Function>) {
        let mut table = Functions::new();
        for function in self.functions {
            table.insert(function.name, Arc::new(function));
        }
        (Arc::new(table), Arc::new(self.main))
    }
}
