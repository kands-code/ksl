//! 集成测试公共代码: 解析, 检查与执行.
//!
//! 各集成测试目标只使用部分辅助, 模块整体放宽未使用项检查.

#![allow(dead_code)]

use ksl_compile::take_main;
use ksl_diagnostics::error::{KslError, RuntimeError as Error};
use ksl_frontend::ast::Program;
use ksl_frontend::check::Session;
use ksl_runtime::Scheduler;
use ksl_typed::CheckedProgram;
use ksl_value::value::Value;

/// 解析一段测试源码.
pub(crate) fn parse(source: &str) -> Result<Program, KslError> {
    ksl_frontend::testutil::parse(source)
}

/// 运行测试程序; 程序尾部是 `@Main` 时按入口声明显式运行根 Actor.
pub(crate) fn run_program(
    scheduler: &mut Scheduler,
    session: &mut Session,
    program: &Program,
) -> Result<Value, Error> {
    let mut program = program.clone();
    let main = take_main(&mut program);
    let checked = session
        .check_typed(&program, main.as_ref())
        .map_err(|err| Error::internal(format!("类型检查失败: {}", err.title())))?;
    run_checked(scheduler, &checked)
}

/// 运行一个已检查的程序; 含 `@Main` 时随后启动根 Actor.
///
/// 失败时回滚全局绑定.
pub(crate) fn run_checked(
    scheduler: &mut Scheduler,
    program: &CheckedProgram,
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
