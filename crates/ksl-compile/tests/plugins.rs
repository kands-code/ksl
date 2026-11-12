//! 插件声明: 路径, 类型身份, 签名与命名冲突.

use ksl_base::name::Name;
use ksl_base::testutil::{write, write_path};
use ksl_compile::ModuleId;
use ksl_diagnostics::error::ErrorKind;

mod common;
use common::*;

#[test]
fn rejects_invalid_plugin_name() {
    let error = load_error(
        "plugin_name",
        &[],
        r#"@Link["../x", { ^T }, { f : Fn[[] : nil] }];"#,
    );
    assert_error_kind!(error, ErrorKind::PluginNameInvalid { .. });
}

#[test]
fn link_declarations_export_types_and_values() {
    let graph = load_ok_with_plugin(
        "link_decl",
        "lib/",
        "raylib",
        &[(
            "lib/ray.ksl",
            r#"@Link["raylib", { ^Texture }, { LoadTexture : Fn[[string] : Texture] }];"#,
        )],
        r#"
@Use[{LoadTexture, ^(Texture, Tex)}, "lib/ray"];
@Let[t : Tex, LoadTexture["x"]];
@Export[t, ^Tex];
"#,
    );
    let lib = graph
        .get(&ModuleId::new("lib/ray".to_string()))
        .expect("依赖模块应存在");
    assert!(
        lib.interface
            .link_values
            .contains_key(&Name::new("LoadTexture"))
    );
    assert!(lib.interface.link_types.contains_key(&Name::new("Texture")));
    let main = graph
        .get(&ModuleId::new("main".to_string()))
        .expect("主模块应存在");
    let ty = main
        .interface
        .value_types
        .get(&Name::new("t"))
        .expect("Link 返回值类型应可确定");
    let ksl_base::types::Type::Opaque(name) = ty else {
        panic!("Link 类型应为不透明类型: {ty}");
    };
    assert_eq!(name.as_str(), "$plugin/lib/plugin/libraylib/Texture");
}

#[test]
fn link_types_follow_plugin_path() {
    let root = temp_root("link_identity");
    write(&root, "lib/a/mod.ksl", r#"@Link["demo", { ^T }, { }];"#);
    write(&root, "lib/b/mod.ksl", r#"@Link["demo", { ^T }, { }];"#);
    let file = ksl_base::plugin::plugin_file("demo").expect("测试插件名应合法");
    write(&root, &format!("lib/a/plugin/{file}"), "");
    write(&root, &format!("lib/b/plugin/{file}"), "");
    let main = r#"
@Use[{^(T, TA)}, "lib/a/mod"];
@Use[{^(T, TB)}, "lib/b/mod"];
nil
"#;
    let path = write_path(&root, "main.ksl", main);
    let graph = checked_text(&path, main);

    let full = |id: &str| {
        let module = graph
            .get(&ModuleId::new(id.to_string()))
            .expect("依赖模块应存在");
        let target = module
            .interface
            .link_types
            .get(&Name::new("T"))
            .expect("Link 类型应导出");
        target.full.as_str()
    };
    assert_eq!(full("lib/a/mod"), "$plugin/lib/a/plugin/libdemo/T");
    assert_eq!(full("lib/b/mod"), "$plugin/lib/b/plugin/libdemo/T");
}

#[test]
fn link_opaque_types_must_be_exported() {
    let error = load_error_with_plugin(
        "link_private_opaque",
        "lib/",
        "raylib",
        &[(
            "lib/ray.ksl",
            r#"@Link["raylib", { ^Texture }, { LoadTexture : Fn[[string] : Texture] }];"#,
        )],
        r#"
@Use[{^(Texture, Tex)}, "lib/ray"];
@Let[f, @Fn[[t : Tex] : nil, nil]];
@Export[f];
"#,
    );
    assert_error_kind!(error, ErrorKind::ExportPrivateType { .. });
}

#[test]
fn link_names_conflict_with_bindings() {
    let error = load_error_with_plugin(
        "link_let",
        "",
        "raylib",
        &[],
        r#"
@Link["raylib", { }, { Foo : Fn[[] : nil] }];
@Let[Foo, 1];
"#,
    );
    assert_error_kind!(error, ErrorKind::LinkNameConflict { .. });

    let error = load_error_with_plugin(
        "link_data",
        "",
        "raylib",
        &[],
        r#"
@Link["raylib", { ^Texture }, { }];
@Data[Texture, {A[]}];
"#,
    );
    assert_error_kind!(error, ErrorKind::LinkNameConflict { .. });
}

#[test]
fn link_names_cannot_be_exported() {
    let error = load_error_with_plugin(
        "link_export",
        "",
        "raylib",
        &[],
        r#"
@Link["raylib", { }, { Foo : Fn[[] : nil] }];
@Export[Foo];
"#,
    );
    assert_error_kind!(error, ErrorKind::LinkExportConflict { .. });
}

#[test]
fn link_signatures_accept_any_value_type() {
    for (index, main) in [
        r#"@Link["demo", {}, { f : Fn[[nil] : i64] }];"#,
        r#"@Link["demo", {}, { f : Fn[[List[string]] : i64] }];"#,
        r#"@Link["demo", {}, { f : Fn[[Dict[string, i64]] : Pair[i64, string]] }];"#,
        r#"@Link["demo", {}, { f : Fn[[] : List[i64]] }];"#,
        r#"@Link["demo", {}, { A : List[i64] }];"#,
    ]
    .into_iter()
    .enumerate()
    {
        let tag = format!("link_type_{index}");
        let (_root, path) = write_files_with_plugin(&tag, "", "demo", &[], main);
        checked_text(&path, main);
    }
}

#[test]
fn link_fallible_result_requires_string_error() {
    let error = load_error_with_plugin(
        "link_result_error",
        "",
        "demo",
        &[],
        r#"@Link["demo", {}, { f : Fn[[] : Result[i64, i64]] }];"#,
    );
    assert_error_kind!(error, ErrorKind::PluginUnsupportedSignature { .. });
}
