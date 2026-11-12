//! 测试辅助; 实现位于前端与基础层.

use std::fs;
use std::path::PathBuf;

pub use ksl_base::testutil::TempDir;
use ksl_frontend::ast::Program;
use ksl_frontend::error::RuntimeError as Error;
pub use ksl_frontend::testutil::parse;

use crate::runtime::scheduler::Scheduler;
use crate::value::Value;

/// 运行测试程序; 程序尾部是 `@Main` 时按入口声明显式运行根 Actor.
///
/// 类型检查仍由调用方完成; 本函数只把入口从程序中抽出并交给调度器.
///
/// # Errors
///
/// 程序执行, 入口求值或根 Actor 运行失败时返回运行时错误.
pub fn run_program(
    scheduler: &mut Scheduler,
    session: &ksl_frontend::check::Session,
    program: &Program,
) -> Result<Value, Error> {
    let mut program = program.clone();
    let main = crate::module::take_main(&mut program);
    let mailboxes = main.as_ref().and_then(|main| {
        session
            .types()
            .get(&main.key())
            .and_then(ksl_frontend::check::Type::as_handler)
            .and_then(|(pid, _)| pid.as_pid())
            .map(|(mailboxes, _)| mailboxes.len())
    });
    scheduler.run_program_with_main(&program, session.types(), main.as_ref(), mailboxes)
}

/// 在临时目录中写入文件, 自动创建父目录.
pub fn write(root: &TempDir, relative: &str, text: &str) {
    let _ = write_path(root, relative, text);
}

/// 在临时目录中写入文件并返回路径, 自动创建父目录.
///
/// # Panics
///
/// 目录或文件无法创建时 panic.
#[must_use]
pub fn write_path(root: &TempDir, relative: &str, text: &str) -> PathBuf {
    let path = root.path().join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("模块目录应可创建");
    }
    fs::write(&path, text).expect("模块文件应可写入");
    path
}
