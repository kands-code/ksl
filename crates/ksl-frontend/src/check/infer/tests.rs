//! `check::infer` 模块测试.
//!
//! 统一器的内部状态在 ksl 源码层面不可见: 变量链, occurs 检查与最终化行为.

use ksl_base::display::TypeNames;
use ksl_base::name::Name;
use ksl_base::source::{SourceId, Span};
use ksl_base::types::Type;

use super::*;

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
    assert!(matches!(err.kind(), Some(ErrorKind::UndeterminedType { .. })));
}

#[test]
fn finalization_reports_unresolved_variable_span() {
    let mut ctx = InferCtx::default();
    let at = Span::new(SourceId::default(), 4, 8);
    let unresolved = ctx.fresh(at, "测试变量");
    let err = ctx.finalize(&unresolved).expect_err("未解析变量应报错");
    assert_eq!(err.span(), Some(at));
}

#[test]
fn infer_mismatch_uses_type_aliases() {
    let mut ctx =
        InferCtx::with_type_names(TypeNames::new([(Name::new("lib/a/A"), Name::new("B"))]));
    let data = InferTy::data(Name::new("lib/a/A"));
    let unresolved = ctx.fresh(span(), "List 元素");
    let list = InferTy::list(unresolved);
    let err = ctx.mismatch_error(&data, &list, span());
    assert!(
        err.title().contains('B'),
        "推断期错误应使用别名: {}",
        err.title()
    );
    assert!(
        !err.title().contains("lib/a/A"),
        "推断期错误不应泄漏真实类型名: {}",
        err.title()
    );
}
