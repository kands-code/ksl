//! `@Main` 的归一化与提取.

use ksl_diagnostics::error::KslError;
use ksl_frontend::ast::{Expr, ExprKind, Program, Special};

/// 取走程序尾部的 `@Main` 声明.
///
/// 调用前须先经 [`normalize_main`] 把声明移动到尾部.
pub(crate) fn take_main(program: &mut Program) -> Option<Expr> {
    if program
        .tail
        .as_ref()
        .is_some_and(|tail| matches!(&tail.node, ExprKind::Special(Special::Main { .. })))
    {
        program.tail.take()
    } else {
        None
    }
}

/// 把 `@Main` 从顶层语句中取出并放到程序末尾.
///
/// `@Main` 的逻辑位置在全部顶层语句之后, 物理位置不影响语义.
/// 依赖模块的 `@Main` 同样保留在末尾参与检查, 但执行结果只由根模块使用.
pub(crate) fn normalize_main(program: &mut Program, module: &str) -> Result<(), KslError> {
    let mut main = None;
    let mut stmts = Vec::with_capacity(program.stmts.len());
    for stmt in std::mem::take(&mut program.stmts) {
        if matches!(&stmt.node, ExprKind::Special(Special::Main { .. })) {
            if main.replace(stmt).is_some() {
                return Err(KslError::multiple_main_statements(
                    module,
                    main.as_ref().expect("刚写入").span,
                ));
            }
        } else {
            stmts.push(stmt);
        }
    }
    program.stmts = stmts;
    if let Some(tail) = program.tail.take() {
        if matches!(&tail.node, ExprKind::Special(Special::Main { .. })) {
            if main.replace(tail).is_some() {
                return Err(KslError::multiple_main_statements(
                    module,
                    main.as_ref().expect("刚写入").span,
                ));
            }
        } else {
            program.tail = Some(tail);
        }
    }
    if let Some(main) = main {
        if let Some(tail) = program.tail.take() {
            program.stmts.push(tail);
        }
        program.tail = Some(main);
    }
    Ok(())
}
