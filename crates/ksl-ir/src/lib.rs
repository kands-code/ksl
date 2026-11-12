//! ksl 中间表示: 把 typed AST 降级为函数级 CPS IR, 再线性化为基本块.
//!
//! 本层不依赖运行时值, 抽象机与调度器.
//! 输入是 `ksl_typed::CheckedProgram`; 可供查看的产物是 [`cps::Module`] 与 [`linear::Module`],
//! 执行器接受 [`linear::plan::Program`].

pub(crate) use ksl_plan::atom;

pub mod cps;
pub(crate) mod display;
pub mod linear;
pub mod lower;

/// 把 typed 程序降级并线性化为执行器输入.
///
/// 这是编译器对执行层暴露的唯一入口; 需要分别查看 CPS 与线性形态时,
/// 调用 [`lower::to_cps`] 与 [`linear::linearize`].
#[must_use]
pub fn compile(program: &ksl_typed::CheckedProgram) -> linear::plan::Program {
    linear::linearize(&lower::to_cps(program)).into_plan()
}
