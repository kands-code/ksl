//! 类型检查内部状态测试.
//!
//! 类型显示与模块上下文在 ksl 源码层面不可见.

use ksl_base::name::Name;
use ksl_base::source::Span;

use super::*;

#[test]
fn type_display_uses_source_syntax() {
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
        (Type::data(Name::new("Shape")), "Shape"),
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
fn type_aliases_follow_introduction_position() {
    let full = Name::new("lib/a/A");
    let mut bindings = TypeBindings::new();
    bindings.insert(
        Name::new("Later"),
        TypeBinding::new(
            TypeTarget::Data(full.clone()),
            Span {
                start: 20,
                end: 25,
                ..Span::default()
            },
        ),
    );
    bindings.insert(
        Name::new("Earlier"),
        TypeBinding::new(
            TypeTarget::Data(full.clone()),
            Span {
                start: 5,
                end: 10,
                ..Span::default()
            },
        ),
    );

    let ty = Type::data(full.clone());
    assert_eq!(display_type(&ty, &bindings), "Earlier");
    assert_eq!(bindings.type_names().resolve(&full), Name::new("Earlier"));
}

#[test]
fn merged_module_context_rejects_conflicting_type() {
    let name = Name::new("Widget");
    let first_full = Name::new("$plugin/first/Widget");
    let second_full = Name::new("$plugin/second/Widget");
    let mut first = ModuleContext::default();
    first.type_env.bind(
        name.clone(),
        TypeTarget::Opaque(first_full.clone()),
        Span::default(),
    );
    let mut second = ModuleContext::default();
    second.type_env.bind(
        name.clone(),
        TypeTarget::Opaque(second_full.clone()),
        Span::default(),
    );

    let mut session = Session::new();
    session.merge_module_context(first).expect("首次合并应成功");
    let error = session
        .merge_module_context(second)
        .expect_err("同名不同类型应被拒绝");
    assert!(matches!(
        error.kind(),
        Some(ksl_diagnostics::error::ErrorKind::DuplicateImport { .. })
    ));
    assert_eq!(
        session.type_env().resolve(&name),
        Some(TypeTarget::Opaque(first_full.clone())),
        "冲突时不应修改已有绑定"
    );
}

#[test]
fn merged_module_context_keeps_opaque_types() {
    let widget = Name::new("Widget");
    let full = Name::new("$plugin/demo/Widget");
    let mut context = ModuleContext::default();
    context
        .type_env
        .bind(widget, TypeTarget::Opaque(full), Span::default());

    let mut session = Session::new();
    session.merge_module_context(context).expect("合并应成功");
    let program =
        crate::testutil::parse("@Let[w : Widget, @Panic[\"x\"]];").expect("脚本应可解析");
    session.check(&program).expect("导入的不透明类型应可解析");
}
