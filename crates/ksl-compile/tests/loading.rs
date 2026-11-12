//! 模块加载: 依赖图, 路径, 声明与入口归一化.

use ksl_base::source::Sources;
use ksl_base::testutil::{write, write_path};
use ksl_compile::{CachedImports, CompileCache, ModuleId, RootSource, load};
use ksl_diagnostics::error::ErrorKind;

mod common;
use common::*;

#[test]
fn loads_dependencies_in_postorder() {
    let graph = load_unlinked(
        "order",
        &[
            ("lib/a.ksl", r#"@Use[{B}, "b"]; @Data[A, {X[]}]; @Export[^A];"#),
            ("lib/b.ksl", r"@Data[B, {Y[]}]; @Export[^B];"),
        ],
        r#"@Use[{A}, "lib/a"]; @Use[{B}, "lib/b"];"#,
    );
    let order: Vec<&str> = graph.order().iter().map(ModuleId::as_str).collect();
    assert_eq!(order, ["lib/b", "lib/a", "main"]);
    assert_eq!(graph.root().as_str(), "main");
    assert!(graph.get(&ModuleId::new("lib/a".to_string())).is_some());
}

#[test]
fn deep_dependency_chain_loads() {
    const DEPTH: usize = 2000;
    let root = temp_root("deep_chain");
    let mut main = None;
    for i in 0..DEPTH {
        let text = if i + 1 < DEPTH {
            format!("@Export[v]; @Let[v, nil]; @Use[{{(v, next)}}, \"m{}\"];", i + 1)
        } else {
            "@Export[v]; @Let[v, nil];".to_string()
        };
        if i == 0 {
            main = Some(write_path(&root, "m0.ksl", &text));
        } else {
            write(&root, &format!("m{i}.ksl"), &text);
        }
    }
    let mut sources = Sources::default();
    let graph = load(
        RootSource::File(&main.expect("主模块路径")),
        &mut sources,
        &CompileCache::new(),
    )
    .expect("深依赖链应可加载");
    assert_eq!(graph.order().len(), DEPTH);
    assert_eq!(graph.order().last().map(ModuleId::as_str), Some("m0"));
}

#[test]
fn empty_use_loads_nothing() {
    let graph = load_unlinked("empty_use", &[], r#"@Use[{}, "missing"]; nil"#);
    assert_eq!(graph.order().len(), 1);
}

#[test]
fn rejects_duplicate_use() {
    let error = load_unlinked_error(
        "duplicate",
        &[("lib/a.ksl", "@Data[A, {X[]}];")],
        r#"@Use[{A}, "lib/a"]; @Use[{A}, "lib/a"];"#,
    );
    assert_error_kind!(error, ErrorKind::DuplicateUse { .. });
}

#[test]
fn rejects_cyclic_imports() {
    let error = load_unlinked_error(
        "cycle",
        &[("lib/a.ksl", r#"@Use[{B}, "../main"]; @Data[A, {X[]}];"#)],
        r#"@Use[{A}, "lib/a"];"#,
    );
    assert_error_kind!(error, ErrorKind::CyclicImport { .. });
}

#[test]
fn rejects_invalid_module_paths() {
    // 路径统一追加 .ksl 后缀, 写全后缀会再次追加.
    let error = load_unlinked_error(
        "suffix",
        &[("lib/a.ksl", "@Data[A, {X[]}];")],
        r#"@Use[{A}, "lib/a.ksl"];"#,
    );
    assert!(matches!(
        error.kind(),
        Some(ErrorKind::ModuleReadFailed { path, .. }) if path.ends_with("lib/a.ksl.ksl")
    ));

    let error = load_unlinked_error("escape", &[], r#"@Use[{A}, "../a"];"#);
    assert_error_kind!(error, ErrorKind::ModulePathEscapesRoot { .. });

    let error = load_error("absolute", &[], r#"@Use[{A}, "/a"];"#);
    assert!(matches!(
        error.kind(),
        Some(ErrorKind::InvalidModulePath { path }) if path == "/a"
    ));
}

#[test]
fn tail_use_and_export_are_declarations() {
    let graph = load_ok(
        "tail_decls",
        &[("lib/a.ksl", r"@Data[A, {C[]}]; @Export[^A]")],
        r#"@Use[{^A}, "lib/a"]"#,
    );
    assert_eq!(graph.order[0].as_str(), "lib/a");
}

#[test]
fn rejects_multiple_export_statements() {
    let error = load_error(
        "export",
        &[],
        r"@Let[x, 1]; @Export[x]; @Let[y, 2]; @Export[y];",
    );
    assert_error_kind!(error, ErrorKind::MultipleExportStatements { .. });
}

#[test]
fn rejects_multiple_main_statements() {
    let error = load_error(
        "multiple_main",
        &[],
        r"@Main[[self : Pid[nil, nil]] : nil, nil]; @Main[[self : Pid[nil, nil]] : nil, nil];",
    );
    assert_error_kind!(error, ErrorKind::MultipleMainStatements { .. });
}

#[test]
fn main_is_extracted_as_entry_declaration() {
    let graph = load_ok("main_entry", &[], "@Main[[self : Pid[nil, nil]] : nil, nil];\n");
    let module = graph.get(&graph.root).expect("根模块应存在");
    assert!(module.program.main.is_some(), "入口声明应被抽取");
    assert!(module.program.tail.is_none(), "程序尾部不应保留 @Main");
    assert!(
        module
            .program
            .main
            .as_ref()
            .is_some_and(|main| main.ty.as_handler().is_some()),
        "入口应记录 Handler 蓝图"
    );
}

#[test]
fn module_tail_expression_is_a_statement() {
    let error = load_error(
        "main_tail_non_nil",
        &[],
        r"@Main[[self : Pid[nil, nil]] : nil, nil]; 40",
    );
    assert_error_kind!(error, ErrorKind::UnusedValue { .. });

    let error = load_error("tail_non_nil", &[], "40\n");
    assert_error_kind!(error, ErrorKind::UnusedValue { .. });

    load_ok(
        "main_tail_discarded",
        &[],
        r"@Main[[self : Pid[nil, nil]] : nil, nil]; @Let[_, 40];",
    );
    load_ok("tail_nil", &[], "nil\n");
}

#[test]
fn virtual_root_resolves_relative_paths() {
    let root = temp_root("virtual");
    write(&root, "lib/a.ksl", LIB_A_DATA_EMPTY);
    let mut sources = Sources::default();
    let program = virtual_program(&mut sources, "<交互>", r#"@Use[{^A}, "lib/a"];"#);
    let graph = load(
        RootSource::Virtual {
            dir: root.path(),
            name: "<交互>",
            program,
            cached_imports: CachedImports::Reject,
        },
        &mut sources,
        &CompileCache::new(),
    )
    .expect("行内导入应可加载");
    assert_eq!(order_of(graph.order()), ["lib/a", "<交互>"]);
    assert!(graph.get(&ModuleId::new("lib/a".to_string())).is_some());
}
