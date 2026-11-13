//! 类型名改写: 把源程序中的类型名按改写表换成目标名.
//!
//! 模块链接把本地名换成完全限定名, 交互会话把导入别名换成真实类型名;
//! 两处改写点相同: 类型标注, 构造器限定名与模式限定名. 数据类型声明名
//! 的改写属于模块内部规则, 由链接阶段另行处理.

use indexmap::IndexMap;

use crate::ast::{
    BindingTarget, Def, Expr, ExprKind, MatchArm, Param, Pattern, Program, Special, TypeExpr,
};
use crate::source::Span;
use crate::symbol::Symbol;

/// 类型名: 真实类型名与可见名的引入位置.
#[derive(Debug, Clone, Copy)]
pub struct TypeName {
    /// 真实类型名, 即类型身份.
    pub full: Symbol,
    /// 可见名的引入位置; 导入时为导入项的位置.
    pub span: Span,
}

/// 类型名表: 可见名 -> 类型名.
pub type TypeNames = IndexMap<Symbol, TypeName>;

/// 改写程序中的类型引用.
pub fn rewrite_type_refs(program: &mut Program, types: &TypeNames) {
    for stmt in &mut program.stmts {
        rewrite_expr(stmt, types);
    }
    if let Some(tail) = &mut program.tail {
        rewrite_expr(tail, types);
    }
}

/// 改写单个表达式中的类型引用.
fn rewrite_expr(expr: &mut Expr, types: &TypeNames) {
    match &mut expr.node {
        ExprKind::FStr(parts) => {
            for part in parts {
                if let crate::ast::FStrAstPart::Interp { expr, .. } = part {
                    rewrite_expr(expr, types);
                }
            }
        },
        ExprKind::List(items) => {
            for item in items {
                rewrite_expr(item, types);
            }
        },
        ExprKind::Pair(first, second) => {
            rewrite_expr(first, types);
            rewrite_expr(second, types);
        },
        ExprKind::Call { head, args } => {
            if let Some(qualifier) = &mut head.qualifier
                && let Some(alias) = types.get(&qualifier.node)
            {
                qualifier.node = alias.full;
            }
            for arg in args {
                rewrite_expr(arg, types);
            }
        },
        ExprKind::Apply { callee, args } => {
            rewrite_expr(callee, types);
            for arg in args {
                rewrite_expr(arg, types);
            }
        },
        ExprKind::Special(special) => rewrite_special(special, types),
        _ => {},
    }
}

/// 改写特殊形式中的类型引用.
#[allow(clippy::too_many_lines)]
fn rewrite_special(special: &mut Special, types: &TypeNames) {
    match special {
        Special::Let { target, value } => {
            rewrite_binding_target(target, types);
            rewrite_expr(value, types);
        },
        Special::Block(stmts) => {
            for stmt in stmts {
                rewrite_expr(stmt, types);
            }
        },
        Special::If { cond, then, els } => {
            rewrite_expr(cond, types);
            rewrite_expr(then, types);
            rewrite_expr(els, types);
        },
        Special::Fn { params, ret, body }
        | Special::Handler { params, ret, body }
        | Special::Main { params, ret, body } => {
            rewrite_params(params, types);
            rewrite_type_expr(ret, types);
            rewrite_expr(body, types);
        },
        Special::Def(def) => rewrite_def(def, types),
        Special::Exit { code } => rewrite_expr(code, types),
        Special::Assert { cond, message } => {
            rewrite_expr(cond, types);
            rewrite_expr(message, types);
        },
        Special::Panic { message }
        | Special::Todo { message }
        | Special::Unreachable { message } => {
            rewrite_expr(message, types);
        },
        Special::Continue { args }
        | Special::ListOp { args, .. }
        | Special::PairOp { args, .. }
        | Special::OptionOp { args, .. }
        | Special::Dict { entries: args }
        | Special::Set { items: args }
        | Special::DictOp { args, .. }
        | Special::SetOp { args, .. }
        | Special::Registry { args, .. } => {
            for arg in args {
                rewrite_expr(arg, types);
            }
        },
        Special::Spawn { handler, args, .. } => {
            rewrite_expr(handler, types);
            for arg in args {
                rewrite_expr(arg, types);
            }
        },
        Special::Send {
            pid,
            index,
            message,
        } => {
            rewrite_expr(pid, types);
            rewrite_expr(index, types);
            rewrite_expr(message, types);
        },
        Special::Monitor { pid } => {
            rewrite_expr(pid, types);
        },
        Special::Unmonitor { id }
        | Special::PidSerial { pid: id }
        | Special::CancelTimer { id } => {
            rewrite_expr(id, types);
        },
        Special::SetTimer {
            pid,
            index,
            message,
            millis,
        } => {
            rewrite_expr(pid, types);
            rewrite_expr(index, types);
            rewrite_expr(message, types);
            rewrite_expr(millis, types);
        },
        Special::Receive { index, timeout } => {
            rewrite_expr(index, types);
            rewrite_expr(timeout, types);
        },
        Special::Join { pid } | Special::Kill { pid } => rewrite_expr(pid, types),
        Special::Sleep { millis } => rewrite_expr(millis, types),
        Special::Fix(defs) => {
            for def in defs {
                rewrite_def(def, types);
            }
        },
        // 数据类型声明名由模块链接另行改写, 这里只改字段类型.
        Special::Data { variants, .. } => {
            for variant in variants {
                for field in &mut variant.fields {
                    rewrite_type_expr(field, types);
                }
            }
        },
        Special::Match { value, arms } => {
            rewrite_expr(value, types);
            for arm in arms {
                rewrite_match_arm(arm, types);
            }
        },
        // 导出与导入声明不含类型引用.
        Special::Export { .. } | Special::Use { .. } | Special::Halt => {},
    }
}

/// 改写绑定目标中的类型标注.
fn rewrite_binding_target(target: &mut BindingTarget, types: &TypeNames) {
    if let Some(ty) = &mut target.ty {
        rewrite_type_expr(ty, types);
    }
}

/// 改写形参表中的类型标注.
fn rewrite_params(params: &mut [Param], types: &TypeNames) {
    for param in params {
        rewrite_type_expr(&mut param.ty, types);
    }
}

/// 改写具名定义.
fn rewrite_def(def: &mut Def, types: &TypeNames) {
    rewrite_params(&mut def.params, types);
    rewrite_type_expr(&mut def.ret, types);
    rewrite_expr(&mut def.body, types);
}

/// 改写匹配分支.
fn rewrite_match_arm(arm: &mut MatchArm, types: &TypeNames) {
    rewrite_pattern(&mut arm.pattern, types);
    rewrite_expr(&mut arm.body, types);
}

/// 改写模式中的类型限定名.
fn rewrite_pattern(pattern: &mut Pattern, types: &TypeNames) {
    if let Some(qualifier) = &mut pattern.qualifier
        && let Some(alias) = types.get(&qualifier.node)
    {
        qualifier.node = alias.full;
    }
}

/// 改写类型表达式中的具名类型.
fn rewrite_type_expr(ty: &mut TypeExpr, types: &TypeNames) {
    match ty {
        TypeExpr::Named(name) => {
            if let Some(alias) = types.get(&name.node) {
                name.node = alias.full;
            }
        },
        TypeExpr::Generic { args, .. } => {
            for arg in args {
                rewrite_type_expr(arg, types);
            }
        },
        TypeExpr::Fn { params, ret, .. } => {
            for param in params {
                rewrite_type_expr(param, types);
            }
            rewrite_type_expr(ret, types);
        },
    }
}
