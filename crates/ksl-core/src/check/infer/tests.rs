//! `check::infer` 模块测试.
//!
//! 统一器的内部状态在 ksl 源码层面不可见, 故用 Rust 测试守住变量链,
//! occurs 检查与最终化行为.

use super::*;
use crate::check::types::Type;
use crate::source::{SourceId, Span};

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
    ctx.unify(&b, &InferTy::List(Box::new(InferTy::Char)), span())
        .expect("绑定应成功");
    assert_eq!(
        ctx.finalize(&a).expect("应可解析"),
        Type::List(Box::new(Type::Char))
    );
}

#[test]
fn occurs_check_and_finalization_follow_spans() {
    let mut ctx = InferCtx::default();
    let var = ctx.fresh(span(), "测试变量");
    let list = InferTy::List(Box::new(var.clone()));
    let err = ctx.unify(&var, &list, span()).expect_err("递归类型应被拒绝");
    assert!(matches!(err.kind, ErrorKind::UndeterminedType { .. }));

    let at = Span::new(SourceId::default(), 4, 8);
    let unresolved = ctx.fresh(at, "测试变量");
    let err = ctx.finalize(&unresolved).expect_err("未解析变量应报错");
    assert_eq!(err.span, Some(at));

    assert_eq!(ctx.render(&unresolved), "_");
    let list = InferTy::List(Box::new(InferTy::I64));
    ctx.unify(&unresolved, &list, span()).expect("统一应成功");
    assert_eq!(ctx.render(&unresolved), "List[i64]");
}
