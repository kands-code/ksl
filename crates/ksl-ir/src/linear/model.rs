//! 线性 CPS IR: 基本块, 槽位与操作数.
//!
//! 该形态由 CPS 树线性化并完成闭包转换得到.
//! 变量引用在解析后只指向当前块的槽位或闭包捕获下标.
//! 运行时按槽位或捕获下标取值.
//! 续延以目标基本块表示, 跳转时显式传递所需参数.
//!
//! 线性化分两步: 先建立带唯一变量身份与未解析引用的基本块,
//! 再做活跃变量分析与槽位分配. 只有解析后的形态会进入执行器.

use ksl_base::name::Name;
use ksl_base::types::Type;
pub use ksl_plan::{
    BlockId, FStrPart, GlobalTarget, Instr, MatchArm, Operand, RecBinding, Target, Terminator,
    VarId, VarRef,
};

use crate::cps::{Binder, DataDecl, FunctionKind};

/// 线性化后的模块.
#[derive(Debug, Clone)]
pub struct Module {
    pub data: Vec<DataDecl>,
    pub functions: Vec<Function>,
    /// 顶层语句体; 不是源码中的 `@Main`.
    pub main: Function,
    /// 源码 `@Main` 的入口 Handler; 只参与显示.
    pub entry: Option<Function>,
}

/// 线性函数: 基本块列表, 参数与自由变量.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: Name,
    pub kind: FunctionKind,
    /// 入口块参数, 即函数形参.
    pub params: Vec<Binder>,
    /// 闭包捕获的源码名; 下标即 `VarRef::Capture`.
    pub free_vars: Vec<Name>,
    pub ret: Type,
    pub blocks: Vec<Block>,
    pub entry: BlockId,
    pub display_name: Option<Name>,
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
