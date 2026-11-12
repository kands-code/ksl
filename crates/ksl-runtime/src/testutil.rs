//! 测试辅助; 实现位于前端与基础层.

pub use ksl_base::testutil::TempDir;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_frontend::ast::Program;
pub use ksl_frontend::testutil::parse;
use ksl_value::value::Value;

use crate::runtime::scheduler::Scheduler;

/// 运行测试程序; 程序尾部是 `@Main` 时按入口声明显式运行根 Actor.
///
/// 类型检查在本函数内完成.
///
/// # Errors
///
/// 类型检查, 程序执行, 入口求值或根 Actor 运行失败时返回运行时错误.
pub fn run_program(
    scheduler: &mut Scheduler,
    session: &mut ksl_frontend::check::Session,
    program: &Program,
) -> Result<Value, Error> {
    let mut program = program.clone();
    let main = ksl_compile::take_main(&mut program);
    let checked = session
        .check_typed(&program, main.as_ref())
        .map_err(|err| Error::internal(format!("类型检查失败: {}", err.title())))?;
    run_checked(scheduler, &checked)
}

/// 运行一个 typed 程序; 含 `@Main` 时随后启动根 Actor.
///
/// 失败时回滚全局绑定. 本函数供测试与脚本用例复用, 不属于运行时执行边界.
///
/// # Errors
///
/// 顶层 Prelude, 根 Actor 的运行时错误或 `@Exit` 在此返回.
pub fn run_checked(
    scheduler: &mut Scheduler,
    program: &ksl_typed::CheckedProgram,
) -> Result<Value, Error> {
    let checkpoint = scheduler.snapshot_globals();
    let mut result = scheduler.run_linear(ksl_ir::compile(program));
    if result.is_ok() && program.main.is_some() {
        let entry = program
            .main_entry()
            .ok_or_else(|| Error::internal("@Main 缺少有效入口"));
        result = entry.and_then(|entry| {
            scheduler
                .run_linear(ksl_ir::compile(&entry.program))
                .and_then(|handler| scheduler.run_main(handler, entry.mailboxes))
        });
    }
    if result.is_err() {
        scheduler.restore_globals(checkpoint);
    }
    result
}
