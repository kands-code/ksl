//! CPS IR 的数据模型.

use std::fmt;
use std::sync::{Arc, LazyLock};

use arcstr::ArcStr;
use ksl_base::fail::FailKind;
use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::builtin::BuiltinId;
pub use ksl_plan::{Binder, Binding, OptionPrim, PatternIr, PrimOp, TempId, VarKey};

use crate::atom::{DataAtom, FStrPart as FStrPartGeneric, ListAtom, define_atom};

/// 降级后的模块.
#[derive(Debug, Clone)]
pub struct Module {
    pub data: Vec<DataDecl>,
    pub functions: Vec<Function>,
    /// 顶层语句体; 不是源码中的 `@Main`.
    pub main: Function,
    /// 源码 `@Main` 的入口 Handler; 只参与显示.
    pub entry: Option<Function>,
}

/// `@Data` 声明; 类型已在检查阶段校验, 仅保留显示所需信息.
#[derive(Debug, Clone)]
pub struct DataDecl {
    pub name: Name,
    pub variants: Vec<VariantDecl>,
}

/// Data 变体: 名字与字段类型.
#[derive(Debug, Clone)]
pub struct VariantDecl {
    pub name: Name,
    /// 字段类型; 由检查器类型表填入.
    pub fields: Vec<Type>,
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

/// 同步求值: 纯内置函数或同步原语.
///
/// 这类操作不挂起, 也不回调用户函数, 在 CPS 中直接绑定结果并继续,
/// 不需要续延节点; 线性化后成为块内指令.
#[derive(Debug, Clone)]
pub enum EvalKind {
    /// 纯内置函数.
    Builtin(BuiltinId),
    /// 同步原语; `ty` 为结果类型, 部分容器操作据此选择表示.
    Prim { op: PrimOp, ty: Type },
}

/// 函数种类; Handler 用 `handler` 关键字显示.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FunctionKind {
    Fn,
    Handler,
}

/// CPS 函数.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: Name,
    pub kind: FunctionKind,
    pub params: Vec<Binder>,
    pub ret: Type,
    pub body: Arc<Cps>,
    /// 具名函数显示用名字; 匿名函数与 Handler 为 None.
    pub display_name: Option<Name>,
}

/// 续延: 参数列表与续延体.
#[derive(Debug, Clone)]
pub struct Cont {
    pub id: ContId,
    pub params: Vec<Binder>,
    pub body: Arc<Cps>,
}

#[derive(Debug, Clone)]
pub enum Cps {
    /// 把值交给续延; 目标为函数自身 `$k` 时即返回.
    Jump {
        cont: ContId,
        value: Value,
    },
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
    /// 同步求值: 结果绑定到目标后继续, 不创建续延.
    Eval {
        op: EvalKind,
        args: Vec<Value>,
        target: Binder,
        body: Arc<Cps>,
        span: Span,
    },
    /// 内置原语: 列表/Option 操作与 Actor 操作.
    ///
    /// `ty` 由检查器类型表填入; 部分容器操作据此选择表示.
    Prim {
        op: PrimOp,
        args: Vec<Value>,
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
        name: Option<Name>,
        ty: Type,
        value: Value,
        body: Arc<Cps>,
    },
    /// 递归函数绑定; 同一组内的函数共享递归环境.
    LetRec {
        bindings: Vec<(Name, Name)>,
        body: Arc<Cps>,
    },
    /// 顶层递归函数绑定: 同时更新全局环境.
    GlobalLetRec {
        bindings: Vec<(Name, Name)>,
        body: Arc<Cps>,
    },
    /// 绑定一个命名续延, 供分支共享.
    LetCont {
        cont: Arc<Cont>,
        body: Arc<Cps>,
    },
    If {
        cond: Value,
        then: Arc<Cps>,
        els: Arc<Cps>,
    },
    Match {
        value: Value,
        arms: Vec<MatchArmIr>,
    },
    /// Handler 自循环.
    Continue {
        args: Vec<Value>,
    },
    /// 顶层退出.
    Exit(Value),
    /// Handler 主动结束.
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
/// 释放过程把子节点搬到显式栈上. 搬空的字段需要保持结构合法.
/// 共享的静态占位值不参与所有权转移.
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
        // 先把直接子节点搬到显式栈上, 再逐个展开.
        // 共享节点由最后的持有者释放.
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
        | Cps::GlobalLetRec { body, .. }
        | Cps::Eval { body, .. } => take_cps(body, pending),
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

#[derive(Debug, Clone)]
pub struct MatchArmIr {
    pub pattern: PatternIr,
    pub body: Arc<Cps>,
}

define_atom!(
    Value,
    FStrPartGeneric<Value>,
    /// 源码变量名.
    Var(Name),
    /// 合成临时值.
    Temp(TempId),
    /// 模块内函数引用.
    Func(Name),
);

pub type FStrPartIr = FStrPartGeneric<Value>;
