//! 类型检查内部状态测试.
//!
//! 类型表, 类型显示与模块上下文在 ksl 源码层面不可见, 因此用 Rust 测试校验.

use ksl_base::source::{SourceId, Span};
use ksl_base::symbol::intern;

use super::*;
use crate::ast::{ExprKind, Special, TypeExpr};

#[test]
fn session_type_table_merges_inputs() {
    let mut sources = ksl_base::source::Sources::default();
    let first = sources.add("<first>", "@Let[x, 1];");
    let second = sources.add("<second>", "@Let[y, 2];");
    let first = crate::parser::parse_text(sources.text(first), first).expect("首段应可解析");
    let second = crate::parser::parse_text(sources.text(second), second).expect("次段应可解析");
    let first_key = first.stmts[0].key();
    let mut session = Session::new();
    session.check(&first).expect("首段应通过检查");
    session.check(&second).expect("次段应通过检查");
    assert!(session.types().contains_key(&first_key), "历史类型表应保留");
}

fn checked_types(source: &str) -> (crate::ast::Program, TypeTable) {
    let program = crate::testutil::parse(source).expect("脚本应可解析");
    let mut session = Session::new();
    session.check(&program).expect("脚本应通过检查");
    (program, session.types().clone())
}

#[test]
fn type_table_records_bindings_and_calls() {
    let (program, types) = checked_types(
        r"
@Let[x : i64, 1];
@Let[f, @Fn[[n : i64] : i64, AddI64[n, 1]]];
@Let[y, f[x]];
",
    );

    let ExprKind::Special(Special::Let { target, value }) = &program.stmts[0].node else {
        panic!("首条语句应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    assert_eq!(types.get(&name.key()), Some(&Type::I64));
    assert_eq!(types.get(&value.key()), Some(&Type::I64));

    let ExprKind::Special(Special::Let { target, value }) = &program.stmts[1].node else {
        panic!("第二条语句应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    let Type::Fn(params, ret) = types.get(&name.key()).expect("函数名应有类型") else {
        panic!("函数名应为函数类型");
    };
    assert_eq!(params.as_slice(), &[Type::I64]);
    assert_eq!(**ret, Type::I64);
    assert_eq!(types.get(&value.key()), types.get(&name.key()));

    let ExprKind::Special(Special::Let { target, value }) = &program.stmts[2].node else {
        panic!("第三条语句应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    assert_eq!(types.get(&name.key()), Some(&Type::I64));
    assert_eq!(types.get(&value.key()), Some(&Type::I64));
}

#[test]
fn type_table_records_inferred_types() {
    let (program, types) = checked_types("@Let[x, {Option/None[], Option/Some[1]}];");
    let ExprKind::Special(Special::Let { target, .. }) = &program.stmts[0].node else {
        panic!("语句应为 @Let");
    };
    let name = target.name.as_ref().expect("绑定名应存在");
    assert_eq!(
        types.get(&name.key()),
        Some(&Type::list(Type::option(Type::I64)))
    );
}

#[test]
fn type_table_records_data_declarations() {
    let (program, types) =
        checked_types("@Data[D, {C[Pair[i64, u8]]}]; @Let[x : List[u8], {1u}];");

    let ExprKind::Special(Special::Data { name, variants }) = &program.stmts[0].node else {
        panic!("首条语句应为 @Data");
    };
    let data_ty = Type::data(name.node);
    assert_eq!(types.get(&name.key()), Some(&data_ty));

    let variant = &variants[0];
    let field_pair = Type::pair(Type::I64, Type::U8);
    let Type::Fn(fields, ret) = types.get(&variant.name.key()).expect("变体应有签名")
    else {
        panic!("变体应记录为函数签名");
    };
    assert_eq!(fields.as_slice(), &[field_pair]);
    assert_eq!(**ret, data_ty);

    let TypeExpr::Generic { name, args } = &variant.fields[0] else {
        panic!("字段应为泛型类型");
    };
    assert_eq!(types.get(&name.key()), Some(&Type::pair(Type::I64, Type::U8)));
    let TypeExpr::Named(inner) = &args[0] else {
        panic!("字段参数应为具名类型");
    };
    assert_eq!(types.get(&inner.key()), Some(&Type::I64));
    let TypeExpr::Named(inner) = &args[1] else {
        panic!("字段参数应为具名类型");
    };
    assert_eq!(types.get(&inner.key()), Some(&Type::U8));

    let ExprKind::Special(Special::Let { target, .. }) = &program.stmts[1].node else {
        panic!("第二条语句应为 @Let");
    };
    let TypeExpr::Generic { name, args } = target.ty.as_ref().expect("应有类型标注")
    else {
        panic!("标注应为泛型类型");
    };
    assert_eq!(types.get(&name.key()), Some(&Type::list(Type::U8)));
    let TypeExpr::Named(inner) = &args[0] else {
        panic!("标注参数应为具名类型");
    };
    assert_eq!(types.get(&inner.key()), Some(&Type::U8));
}

#[test]
fn type_display_covers_applications() {
    let cases = [
        (Type::list(Type::I64), "List[i64]"),
        (Type::dict(Type::Str, Type::U64), "Dict[string, u64]"),
        (Type::set(Type::Bool), "Set[bool]"),
        (Type::pair(Type::I64, Type::U8), "Pair[i64, u8]"),
        (Type::option(Type::I64), "Option[i64]"),
        (Type::result(Type::I64, Type::Str), "Result[i64, string]"),
        (
            Type::pid(vec![Type::I64, Type::Str], Type::Nil),
            "Pid[i64, string, nil]",
        ),
        (
            Type::handler(
                Type::pid(vec![Type::I64], Type::Nil),
                vec![Type::I64, Type::Str],
            ),
            "Handler[Pid[i64, nil], i64, string]",
        ),
        (Type::data(intern("Shape")), "Shape"),
        (Type::file(), "File"),
        (Type::date_time(), "DateTime"),
        (Type::socket(), "Socket"),
        (Type::tcp_listener(), "TcpListener"),
    ];
    for (ty, expected) in cases {
        assert_eq!(ty.to_string(), expected);
    }
}

#[test]
fn merged_module_context_keeps_opaque_types() {
    let widget = intern("Widget");
    let full = intern("$plugin/demo/Widget");
    let mut context = ModuleContext::default();
    context.add_type_name(
        widget,
        crate::rewrite::TypeName {
            full,
            span: Span::new(SourceId::default(), 0, 0),
        },
    );
    context.add_opaque_type(full);

    let mut session = Session::new();
    session.merge_module_context(context).expect("合并应成功");
    let mut program =
        crate::testutil::parse("@Let[w : Widget, @Panic[\"x\"]];").expect("脚本应可解析");
    apply_type_names(&mut session, &mut program);
    session.check(&program).expect("导入的不透明类型应可解析");
}
