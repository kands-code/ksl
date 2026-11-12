//! 模块链接: 导出, 别名与类型可见性.

use ksl_base::name::Name;
use ksl_diagnostics::error::ErrorKind;

mod common;
use common::*;

#[test]
fn links_exports_and_renames_types() {
    let graph = load_ok(
        "link",
        &[("lib/a.ksl", LIB_A_DATA)],
        r#"
@Use[{^(A, NA), ^(A, NB)}, "lib/a"];
@Let[x : NA, NA/C[1]];
@Let[y : NB, NB/C[2]];
@Export[x, y, ^NA, ^NB];
"#,
    );
    let main = main_module(&graph);
    for name in ["NA", "NB"] {
        let exported = main.interface.types.get(&Name::new(name)).expect("别名应导出");
        assert_eq!(exported.full.as_str(), "lib/a/A");
    }
    for name in ["x", "y"] {
        let ty = main
            .interface
            .value_types
            .get(&Name::new(name))
            .expect("导出值类型应可确定");
        assert!(ty.as_data().is_some_and(|name| name.as_str() == "lib/a/A"));
    }
}

#[test]
fn rejects_missing_export() {
    let error = load_error(
        "missing_export",
        &[("lib/a.ksl", "@Data[A, {C[]}];")],
        r#"@Use[{^A}, "lib/a"];"#,
    );
    assert_error_kind!(error, ErrorKind::ImportNotExported { .. });
}

#[test]
fn rejects_private_export_type() {
    let error = load_error(
        "private_export",
        &[(
            "lib/a.ksl",
            r"
@Data[Hidden, {Secret[i64]}];
@Let[get, @Fn[[] : Hidden, Hidden/Secret[7]]];
@Export[get];
",
        )],
        r#"@Use[{get}, "lib/a"];"#,
    );
    assert_error_kind!(error, ErrorKind::ExportPrivateType { .. });
}

#[test]
fn builtin_time_types_in_export_signatures() {
    load_ok(
        "builtin_export",
        &[(
            "lib/a.ksl",
            r"
@Def[parse, [s : string] : Result[DateTime, TimeError],
  ParseDateTime[s]];
@Export[parse];
",
        )],
        r#"@Use[{parse}, "lib/a"];"#,
    );
}

#[test]
fn tail_data_declaration_is_linked() {
    let graph = load_ok(
        "tail_data",
        &[("lib/a.ksl", r"@Export[^D]; @Data[D, {C[i64]}]")],
        r#"@Use[{^D}, "lib/a"]; @Let[x : D, D/C[1]]; @Export[x, ^D];"#,
    );
    let main = main_module(&graph);
    let x_ty = main
        .interface
        .value_types
        .get(&Name::new("x"))
        .expect("x 类型应可确定");
    assert!(x_ty.as_data().is_some_and(|name| name.as_str() == "lib/a/D"));
}

#[test]
fn rejects_conflicting_import_names() {
    let error = load_error(
        "import_conflict",
        &[
            ("lib/a.ksl", MAKE_ONE),
            ("lib/b.ksl", r"@Let[make, @Fn[[] : i64, 2]]; @Export[make];"),
        ],
        r#"@Use[{make}, "lib/a"]; @Use[{make}, "lib/b"];"#,
    );
    assert_error_kind!(error, ErrorKind::DuplicateImport { .. });
}

#[test]
fn type_errors_use_imported_alias() {
    let error = load_error(
        "alias_error",
        &[("lib/a.ksl", LIB_A_DATA)],
        r#"@Use[{^(A, B)}, "lib/a"]; @Let[x : B, 1i64];"#,
    );
    assert!(
        error.title().contains("期望 B, 实际 i64"),
        "错误标题应使用本地别名: {}",
        error.title()
    );
    assert!(
        !error.title().contains("lib/a/A"),
        "错误标题不应泄漏真实类型名: {}",
        error.title()
    );
}

#[test]
fn rejects_unknown_exports() {
    let error = load_error(
        "export_unknown",
        &[("lib/a.ksl", "@Export[nope];")],
        r#"@Use[{A}, "lib/a"];"#,
    );
    assert!(matches!(
        error.kind(),
        Some(ErrorKind::ExportUnknown { name }) if name == "nope"
    ));
}

#[test]
fn rejects_duplicate_exports() {
    let error = load_error(
        "export_duplicate",
        &[("lib/a.ksl", "@Let[x, 1]; @Export[x, x];")],
        r#"@Use[{A}, "lib/a"];"#,
    );
    assert!(matches!(
        error.kind(),
        Some(ErrorKind::DuplicateExport { name }) if name == "x"
    ));
}

#[test]
fn matches_visible_type_without_qualifier() {
    load_ok(
        "bare_pattern",
        &[("lib/m.ksl", BOX_MODULE)],
        r#"
@Use[{get, ^Box}, "lib/m"];
@Let[n, @Match[get[], { (/Wrap[v], v) }]];
"#,
    );
}

#[test]
fn rejects_invisible_type_in_pattern() {
    let error = load_error(
        "invisible_pattern",
        &[("lib/m.ksl", BOX_MODULE)],
        r#"
@Use[{get}, "lib/m"];
@Let[n, @Match[get[], { (/Wrap[v], v) }]];
"#,
    );
    assert_error_kind!(error, ErrorKind::TypeNotVisible);
}

#[test]
fn rejects_invisible_type_in_data_get() {
    let error = load_error(
        "invisible_data_get",
        &[("lib/m.ksl", BOX_MODULE)],
        r#"
@Use[{get}, "lib/m"];
@Let[n, @DataGet[get[], 0u8]];
"#,
    );
    assert_error_kind!(error, ErrorKind::TypeNotVisible);
}

#[test]
fn matches_type_declaration_through_reexport() {
    load_ok(
        "reexport_decl",
        &[
            (
                "lib/b.ksl",
                r"
@Data[B, {Wrap[i64]}];
@Let[get, @Fn[[] : B, B/Wrap[7]]];
@Export[get, ^B];
",
            ),
            (
                "lib/a.ksl",
                r#"
@Use[{get, ^B}, "b"];
@Let[relay, @Fn[[] : B, get[]]];
@Export[relay, ^B];
"#,
            ),
        ],
        r#"
@Use[{relay, ^B}, "lib/a"];
@Let[n, @Match[relay[], { (/Wrap[v], v) }]];
"#,
    );
}
