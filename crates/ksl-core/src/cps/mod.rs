//! CPS 后端.
//!
//! CPS 即续延传递风格, 是本项目的执行模型; IR 即中间表示, 是类型检查后的
//! 降级目标.
//!
//! 类型检查后的 AST 先降级为函数级 CPS IR, 再线性化为基本块后由抽象机执行;
//! 调度器在单线程上协作推进主脚本与 Actor, 挂起即保存执行状态.

pub(crate) mod atom;
pub mod exec;
pub(crate) mod gc;
pub mod ir;
pub mod linear;
pub mod scheduler;
