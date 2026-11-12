//! 错误标题与内部错误渲染测试.

use ksl_base::display::TypeNames;
use ksl_base::name::Name;
use ksl_base::source::{Sources, Span};
use ksl_base::types::Type;

use super::{ErrorKind, KslError, RuntimeError};

#[test]
fn subject_is_not_repeated_in_detail() {
    let error = KslError::with_subject(
        ErrorKind::DuplicateUse {
            path: "lib/a".to_string(),
        },
        Span::default(),
        "lib/a",
    );
    assert_eq!(error.title(), "重复 @Use 同一模块 `lib/a`");

    let error = KslError::with_subject(
        ErrorKind::ImportNotExported {
            module: "lib/a".to_string(),
            name: "Missing".to_string(),
        },
        Span::default(),
        "Missing",
    );
    assert_eq!(error.title(), "导入的名字未导出 `Missing`: lib/a");
}

#[test]
fn error_is_a_thin_handle() {
    assert_eq!(std::mem::size_of::<KslError>(), std::mem::size_of::<usize>());
}

#[test]
fn internal_errors_have_no_code_or_kind() {
    let error = KslError::internal("模块顺序不一致", None);
    assert!(error.kind().is_none());
    assert!(error.code().is_none());
    assert_eq!(error.title(), "内部错误: 模块顺序不一致");
    let rendered = crate::diagnostic::render_error(&Sources::default(), &error);
    assert!(rendered.contains("内部错误: 模块顺序不一致"));
    assert!(!rendered.contains("error["));

    let runtime = RuntimeError::internal("抽象机不变量");
    let rendered = crate::diagnostic::render_runtime_error(&Sources::default(), &runtime);
    assert!(rendered.contains("内部错误: 抽象机不变量"));
    assert!(!rendered.contains("error["));
}

#[test]
fn type_names_render_aliases() {
    let error = KslError::new(
        ErrorKind::TypeMismatch {
            expected: Type::data(Name::new("lib/a/A")),
            found: Type::I64,
        },
        Span::default(),
    )
    .with_type_names(TypeNames::new([(Name::new("lib/a/A"), Name::new("B"))]));
    assert_eq!(error.title(), "类型不符: 期望 B, 实际 i64");
}
