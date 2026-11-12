//! 测试辅助: 脚本装载与检查入口.

use ksl_base::source::Sources;
pub use ksl_base::testutil::TempDir;

use crate::ast::Program;
use crate::error::KslError;

/// 解析脚本源码.
///
/// # Errors
///
/// 词法或语法错误.
pub fn parse(source: &str) -> Result<Program, KslError> {
    let mut sources = Sources::default();
    let id = sources.add("<test>", source);
    crate::parser::parse_text(sources.text(id), id)
}

/// 类型检查脚本源码.
///
/// # Errors
///
/// 词法, 语法或类型检查错误.
///
/// # Panics
///
/// 解析失败时 panic; 只用于测试脚本应可解析的前提.
pub fn check(source: &str) -> Result<(), KslError> {
    let program = parse(source).expect("脚本应可解析");
    crate::check::check_program(&program)
}
