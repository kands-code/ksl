//! `check::infer` 模块测试.
//!
//! 统一器的内部状态在 ksl 源码层面不可见, 因此用 Rust 测试校验变量链,
//! occurs 检查与最终化行为.

use ksl_base::source::{SourceId, Span};

use super::*;
use crate::check::types::Type;

fn span() -> Span {
    Span::new(SourceId::default(), 0, 0)
}

#[test]
fn unification_resolves_variable_chains() {
    let mut ctx = InferCtx::default();
    let var = ctx.fresh(span(), "测试变量");
    ctx.unify(&var, &InferTy::I64, span()).expect("统一应成功");
    assert_eq!(ctx.finalize(&var).expect("应可解析"), Type::I64);

    let a = ctx.fresh(span(), "变量 a");
    let b = ctx.fresh(span(), "变量 b");
    ctx.unify(&a, &b, span()).expect("变量合并应成功");
    ctx.unify(&b, &InferTy::list(InferTy::Char), span())
        .expect("绑定应成功");
    assert_eq!(ctx.finalize(&a).expect("应可解析"), Type::list(Type::Char));
}

#[test]
fn occurs_check_rejects_recursive_types() {
    let mut ctx = InferCtx::default();
    let var = ctx.fresh(span(), "测试变量");
    let list = InferTy::list(var.clone());
    let err = ctx.unify(&var, &list, span()).expect_err("递归类型应被拒绝");
    assert!(matches!(err.kind, ErrorKind::UndeterminedType { .. }));
}

#[test]
fn finalization_reports_unresolved_variable_span() {
    let mut ctx = InferCtx::default();
    let at = Span::new(SourceId::default(), 4, 8);
    let unresolved = ctx.fresh(at, "测试变量");
    let err = ctx.finalize(&unresolved).expect_err("未解析变量应报错");
    assert_eq!(err.span, Some(at));
}

#[test]
fn render_shows_placeholder_and_resolved_type() {
    let mut ctx = InferCtx::default();
    let var = ctx.fresh(span(), "测试变量");
    assert_eq!(ctx.render(&var), "_");
    ctx.unify(&var, &InferTy::list(InferTy::I64), span())
        .expect("统一应成功");
    assert_eq!(ctx.render(&var), "List[i64]");
}
