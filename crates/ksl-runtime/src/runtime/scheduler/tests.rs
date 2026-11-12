//! `runtime::scheduler` 模块测试.
//!
//! 按被测行为分为 Actor 生命周期, 调度公平性, 文件 I/O 与会话复用.

use ksl_frontend::check::Session;

use super::*;

mod actor;
mod file_io;
mod repl;
mod scheduling;

fn run_on(
    scheduler: &mut Scheduler,
    session: &mut Session,
    source: &str,
) -> Result<Value, Error> {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    crate::testutil::run_program(scheduler, session, &program)
}

fn run_with(scheduler: &mut Scheduler, source: &str) -> Result<Value, Error> {
    run_on(scheduler, &mut Session::new(), source)
}

fn run(source: &str) -> Result<Value, Error> {
    let mut scheduler = Scheduler::new();
    let result = run_with(&mut scheduler, source);
    scheduler.shutdown();
    result
}
