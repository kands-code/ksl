//! 线性 CPS IR: 基本块, 槽位与操作数.
//!
//! 该形态由 CPS 树线性化并完成闭包转换得到.
//! 变量引用在解析后只指向当前块的槽位或闭包捕获下标.
//! 运行时按槽位或捕获下标取值.
//! 续延以目标基本块表示, 跳转时显式传递所需参数.
//!
//! 线性化分两步: 先建立带唯一变量身份与未解析引用的基本块,
//! 再做活跃变量分析与槽位分配. 只有解析后的形态会进入执行器.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_base::symbol::Symbol;
use ksl_frontend::ast::FailKind;
use ksl_frontend::check::Type;

use crate::builtin::BuiltinId;
use crate::cps::atom::{FStrPart as FStrPartGeneric, define_atom};
use crate::cps::ir::{Binder, DataDecl, FunctionKind, PatternIr, PrimOp};

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

impl fmt::Display for VarId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// 变量引用: 未解析的变量身份, 当前块槽位, 闭包捕获下标或未绑定源码名.
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
    /// 融合后的纯内置调用; 结果写入槽位.
    Builtin {
        slot: u32,
        callee: BuiltinId,
        args: Vec<Operand>,
        span: Span,
    },
    /// 融合后的同步原语; 结果写入槽位.
    ///
    /// `dead` 列出该指令执行后不再被读取的槽位, 运行时在调用原语前清空,
    /// 使容器实参保持唯一, 原地修改不退化为整树复制.
    Prim {
        slot: u32,
        op: PrimOp,
        args: Vec<Operand>,
        arg_types: Vec<Type>,
        ty: Type,
        span: Span,
        dead: Vec<u32>,
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

pub type FStrPart = FStrPartGeneric<Operand>;

/// 槽位在指令中的角色; 终止槽位是写入目标, `dead` 是之后可清理的槽位.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlotRole {
    Assigned,
    Dead,
}

/// 遍历操作数的直接子项; `$iter` 选择只读或可变迭代.
macro_rules! walk_operand_children {
    ($operand:expr, $visit:expr, $iter:ident) => {
        match $operand {
            Operand::Func { captures, .. } => {
                for capture in captures.$iter() {
                    $visit(capture);
                }
            },
            Operand::List { items, .. } => {
                for item in items.$iter() {
                    $visit(item);
                }
            },
            Operand::Pair { first, second, .. } => {
                $visit(first);
                $visit(second);
            },
            Operand::Data { fields, .. } => {
                for field in fields.$iter() {
                    $visit(field);
                }
            },
            Operand::FStr(parts) => {
                for part in parts.$iter() {
                    if let FStrPart::Interp { value, .. } = part {
                        $visit(value);
                    }
                }
            },
            Operand::I64(_)
            | Operand::U8(_)
            | Operand::U64(_)
            | Operand::F64(_)
            | Operand::Char(_)
            | Operand::Str(_)
            | Operand::Bool(_)
            | Operand::Nil
            | Operand::Builtin(_)
            | Operand::Var(_) => {},
        }
    };
}

impl Operand {
    /// 遍历复合操作数的直接子项; 原子操作数不产生访问.
    pub(super) fn for_each_child(&self, visit: &mut impl FnMut(&Operand)) {
        walk_operand_children!(self, visit, iter);
    }

    /// 可变版本; 遍历范围与 [`Self::for_each_child`] 相同.
    pub(super) fn for_each_child_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        walk_operand_children!(self, visit, iter_mut);
    }

    /// 深度优先遍历自身与全部子项; 访问函数返回 false 时不进入子项.
    pub(super) fn walk_mut(&mut self, visit: &mut impl FnMut(&mut Operand) -> bool) {
        walk_operand_mut(self, visit);
    }
}

fn walk_operand_mut(operand: &mut Operand, visit: &mut impl FnMut(&mut Operand) -> bool) {
    if visit(operand) {
        operand.for_each_child_mut(&mut |child| walk_operand_mut(child, visit));
    }
}

/// 遍历指令引用的操作数; `$iter` 选择只读或可变迭代.
macro_rules! walk_instr_operands {
    ($instr:expr, $visit:expr, $iter:ident) => {
        match $instr {
            Instr::Let { value, .. } | Instr::GlobalLet { value, .. } => $visit(value),
            Instr::Builtin { args, .. } | Instr::Prim { args, .. } => {
                for arg in args.$iter() {
                    $visit(arg);
                }
            },
            Instr::LetRec { bindings } | Instr::GlobalLetRec { bindings } => {
                for binding in bindings.$iter() {
                    for capture in binding.captures.$iter() {
                        $visit(capture);
                    }
                }
            },
        }
    };
}

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

impl Instr {
    /// 遍历指令引用的操作数; 目标槽位与 `dead` 槽位不在其中.
    pub(super) fn for_each_operand(&self, visit: &mut impl FnMut(&Operand)) {
        walk_instr_operands!(self, visit, iter);
    }

    /// 可变版本; 遍历范围与 [`Self::for_each_operand`] 相同.
    pub(super) fn for_each_operand_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        walk_instr_operands!(self, visit, iter_mut);
    }

    /// 遍历指令写入或标记可清理的槽位.
    pub(super) fn for_each_slot_mut(&mut self, visit: &mut impl FnMut(&mut u32, SlotRole)) {
        match self {
            Self::Let { slot, .. } | Self::Builtin { slot, .. } => {
                visit(slot, SlotRole::Assigned);
            },
            Self::Prim { slot, dead, .. } => {
                visit(slot, SlotRole::Assigned);
                for slot in dead {
                    visit(slot, SlotRole::Dead);
                }
            },
            Self::GlobalLet { slot, .. } => {
                if let Some(slot) = slot {
                    visit(slot, SlotRole::Assigned);
                }
            },
            Self::LetRec { bindings } | Self::GlobalLetRec { bindings } => {
                for binding in bindings {
                    visit(&mut binding.slot, SlotRole::Assigned);
                }
            },
        }
    }
}

/// 遍历终止指令引用的操作数; `$iter` 选择只读或可变迭代.
macro_rules! walk_term_operands {
    ($term:expr, $visit:expr, $iter:ident) => {
        match $term {
            Terminator::Return { value } | Terminator::Exit { value } => $visit(value),
            Terminator::Fail { message, .. } => $visit(message),
            Terminator::Jump { args, .. } | Terminator::Continue { args } => {
                for arg in args.$iter() {
                    $visit(arg);
                }
            },
            Terminator::TailCall { callee, args, .. } => {
                $visit(callee);
                for arg in args.$iter() {
                    $visit(arg);
                }
            },
            Terminator::Call {
                callee, args, live, ..
            } => {
                $visit(callee);
                for arg in args.$iter().chain(live.$iter()) {
                    $visit(arg);
                }
            },
            Terminator::Prim { args, live, .. } => {
                for arg in args.$iter().chain(live.$iter()) {
                    $visit(arg);
                }
            },
            Terminator::Receive { timeout, live, .. } => {
                $visit(timeout);
                for arg in live.$iter() {
                    $visit(arg);
                }
            },
            Terminator::If { cond, then, els } => {
                $visit(cond);
                for arg in then.args.$iter().chain(els.args.$iter()) {
                    $visit(arg);
                }
            },
            Terminator::Match { value, arms } => {
                $visit(value);
                for arm in arms.$iter() {
                    for arg in arm.live.$iter() {
                        $visit(arg);
                    }
                }
            },
            Terminator::Halt => {},
        }
    };
}

impl Terminator {
    /// 遍历终止指令引用的操作数.
    pub(super) fn for_each_operand(&self, visit: &mut impl FnMut(&Operand)) {
        walk_term_operands!(self, visit, iter);
    }

    /// 可变版本; 遍历范围与 [`Self::for_each_operand`] 相同.
    pub(super) fn for_each_operand_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        walk_term_operands!(self, visit, iter_mut);
    }

    /// 遍历终止指令的目标块.
    pub(super) fn for_each_target(&self, visit: &mut impl FnMut(BlockId)) {
        match self {
            Self::Jump { target, .. }
            | Self::Call { target, .. }
            | Self::Prim { target, .. }
            | Self::Receive { target, .. } => visit(*target),
            Self::If { then, els, .. } => {
                visit(then.block);
                visit(els.block);
            },
            Self::Match { arms, .. } => {
                for arm in arms {
                    visit(arm.target);
                }
            },
            Self::Return { .. }
            | Self::TailCall { .. }
            | Self::Continue { .. }
            | Self::Exit { .. }
            | Self::Halt
            | Self::Fail { .. } => {},
        }
    }

    /// 可变版本; 遍历范围与 [`Self::for_each_target`] 相同.
    pub(super) fn for_each_target_mut(&mut self, visit: &mut impl FnMut(&mut BlockId)) {
        match self {
            Self::Jump { target, .. }
            | Self::Call { target, .. }
            | Self::Prim { target, .. }
            | Self::Receive { target, .. } => visit(target),
            Self::If { then, els, .. } => {
                visit(&mut then.block);
                visit(&mut els.block);
            },
            Self::Match { arms, .. } => {
                for arm in arms {
                    visit(&mut arm.target);
                }
            },
            Self::Return { .. }
            | Self::TailCall { .. }
            | Self::Continue { .. }
            | Self::Exit { .. }
            | Self::Halt
            | Self::Fail { .. } => {},
        }
    }
}
