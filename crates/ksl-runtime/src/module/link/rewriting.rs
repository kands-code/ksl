//! 模块程序的类型名改写.

use ksl_frontend::ast::{Expr, ExprKind, Program, Special};
use ksl_frontend::rewrite::{self, TypeNames};

/// 改写模块程序: 数据类型声明名换为完全限定名, 类型引用按改写表替换.
///
/// 导入与导出声明不是可执行语句, 改写前先行剔除.
pub(super) fn rewrite_program(program: &mut Program, types: &TypeNames) {
    program.stmts.retain_mut(|stmt| {
        if matches!(
            stmt.node,
            ExprKind::Special(Special::Export { .. } | Special::Use { .. })
        ) {
            return false;
        }
        rewrite_data_names(stmt, types);
        true
    });
    if program.tail.as_ref().is_some_and(|expr| {
        matches!(
            &expr.node,
            ExprKind::Special(Special::Export { .. } | Special::Use { .. })
        )
    }) {
        program.tail = None;
    } else if let Some(tail) = &mut program.tail {
        rewrite_data_names(tail, types);
    }
    rewrite::rewrite_type_refs(program, types);
}

/// 改写数据类型声明名: 本地名换成完全限定名.
///
/// 声明名只在模块内部可见, 故与通用类型引用改写分开处理.
fn rewrite_data_names(stmt: &mut Expr, types: &TypeNames) {
    let ExprKind::Special(Special::Data { name, .. }) = &mut stmt.node else {
        return;
    };
    if let Some(alias) = types.get(&name.node) {
        name.node = alias.full;
    }
}
