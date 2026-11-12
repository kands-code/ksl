//! 线性 CPS IR: 基本块, 槽位与操作数.
//!
//! 该形态由 CPS 树线性化并完成闭包转换得到. 变量引用在解析后只指向
//! 当前块的槽位或闭包捕获下标, 运行期按槽位或捕获下标取值. 续延以目标
//! 基本块表示, 跳转时显式传递所需参数.
//!
//! 线性化分两步: 先建立带唯一变量身份与未解析引用的基本块, 再做活跃变量
//! 分析与槽位分配. 只有解析后的形态会进入执行器.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::ast::FailKind;
use crate::builtin::BuiltinId;
use crate::check::Type;
use crate::cps::atom::{FStrPart as FStrPartGeneric, define_atom};
use crate::cps::ir::{Binder, DataDecl, FunctionKind, PatternIr, PrimOp};
use crate::source::Span;
use crate::symbol::Symbol;

/// 函数内唯一的变量身份; 由线性化器分配.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarId(u32);

impl VarId {
    /// 由序号构造.
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

impl fmt::Display for VarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// 变量引用.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VarRef {
    /// 未解析的函数内变量身份; 只出现在线性化中间态.
    Id(VarId),
    /// 当前块的槽位.
    Local(u32),
    /// 函数闭包的捕获下标.
    Capture(u32),
    /// 未在本函数内绑定的源码名; 解析后变为 `Capture`.
    Free(Symbol),
}

/// 基本块编号.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(u32);

impl BlockId {
    /// 由块序号构造.
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

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b{}", self.0)
    }
}

/// 线性化后的模块.
#[derive(Debug, Clone)]
pub struct Module {
    pub data: Vec<DataDecl>,
    pub functions: Vec<Function>,
    pub main: Function,
}

/// 运行时函数表: 函数名到线性函数.
pub type Functions = HashMap<Symbol, Arc<Function>>;

/// 线性函数: 基本块列表, 参数与自由变量.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: Symbol,
    pub kind: FunctionKind,
    /// 入口块参数, 即函数形参.
    pub params: Vec<Binder>,
    /// 闭包捕获的源码名; 下标即 `VarRef::Capture`.
    pub free_vars: Vec<Symbol>,
    pub ret: Type,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
    pub display_name: Option<Symbol>,
}

/// 基本块: 槽位, 顺序指令与唯一终止指令.
#[derive(Debug, Clone)]
pub struct Block {
    pub id: BlockId,
    /// 全部槽位; 前 `param_count` 个是参数.
    pub slots: Vec<Binder>,
    /// 参数个数; 参数占据连续槽位.
    pub param_count: usize,
    pub instrs: Vec<Instr>,
    pub term: Terminator,
}

/// 不改变控制流的指令.
#[derive(Debug, Clone)]
pub enum Instr {
    Let {
        slot: u32,
        value: Operand,
    },
    GlobalLet {
        slot: Option<u32>,
        name: Option<Symbol>,
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

/// 递归绑定: 名字槽位, 函数与捕获值.
#[derive(Debug, Clone)]
pub struct RecBinding {
    pub name: Symbol,
    pub slot: u32,
    pub function: Symbol,
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
        arg_types: Vec<Type>,
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

/// 分支目标.
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
    /// 模块内函数引用; `captures` 在创建点求值.
    Func {
        name: Symbol,
        captures: Vec<Operand>,
    },
);

/// 线性 IR 格式化字符串片段.
pub type FStrPart = FStrPartGeneric<Operand>;

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
