//! 模块加载器测试.
//!
//! 覆盖模块图, 链接, 缓存与类型表.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use ksl_base::symbol::intern;

use super::*;
use crate::testutil::{TempDir, write, write_path};

/// 多个用例共用的模块文本.
const LIB_A_DATA: &str = r"@Data[A, {C[i64]}]; @Export[^A];";
const LIB_A_DATA_EMPTY: &str = r"@Data[A, {C[]}]; @Export[^A];";
const LIB_A_MAKE: &str = concat!(
    r"@Data[A, {C[i64]}]; ",
    r"@Let[make, @Fn[[n : i64] : A, A/C[n]]]; ",
    r"@Export[make, ^A];",
);
const MAKE_ONE: &str = r"@Let[make, @Fn[[] : i64, 1]]; @Export[make];";
const COUNT_DEF: &str = r"
@Def[count, [n : i64] : i64,
  @If[EqualI64[n, 0], 0, count[SubI64[n, 1]]]];
@Export[count];
";

fn temp_root(tag: &str) -> TempDir {
    TempDir::new(&format!("ksl_module_{tag}_"))
}

/// 写入一组模块文件并返回主文件路径.
fn write_files(tag: &str, files: &[(&str, &str)], main: &str) -> (TempDir, PathBuf) {
    let root = temp_root(tag);
    for (relative, text) in files {
        write(&root, relative, text);
    }
    let path = write_path(&root, "main.ksl", main);
    (root, path)
}

fn load_unlinked(tag: &str, files: &[(&str, &str)], main: &str) -> ModuleGraph {
    let (_root, path) = write_files(tag, files, main);
    let mut sources = Sources::default();
    load(&path, main, &mut sources).expect("模块应可加载")
}

fn load_unlinked_error(tag: &str, files: &[(&str, &str)], main: &str) -> KslError {
    let (_root, path) = write_files(tag, files, main);
    let mut sources = Sources::default();
    load(&path, main, &mut sources).expect_err("模块应报错")
}

fn load_ok(tag: &str, files: &[(&str, &str)], main: &str) -> LinkedGraph {
    let (_root, path) = write_files(tag, files, main);
    let mut sources = Sources::default();
    crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查")
}

fn load_error(tag: &str, files: &[(&str, &str)], main: &str) -> KslError {
    let (_root, path) = write_files(tag, files, main);
    let mut sources = Sources::default();
    crate::module::load_and_check(&path, main, &mut sources).expect_err("模块应报错")
}

/// 写入模块文件, 并在指定目录下放置空插件文件.
///
/// 链接阶段只要求插件路径存在, 不读取内容.
fn write_files_with_plugin(
    tag: &str,
    module_dir: &str,
    plugin: &str,
    files: &[(&str, &str)],
    main: &str,
) -> (TempDir, PathBuf) {
    let root = temp_root(tag);
    for (relative, text) in files {
        write(&root, relative, text);
    }
    let file = crate::ffi::plugin_file(plugin).expect("测试插件名应合法");
    let _ = write_path(&root, &format!("{module_dir}plugin/{file}"), "");
    let path = write_path(&root, "main.ksl", main);
    (root, path)
}

fn load_ok_with_plugin(
    tag: &str,
    module_dir: &str,
    plugin: &str,
    files: &[(&str, &str)],
    main: &str,
) -> LinkedGraph {
    let (_root, path) = write_files_with_plugin(tag, module_dir, plugin, files, main);
    let mut sources = Sources::default();
    crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查")
}

fn load_error_with_plugin(
    tag: &str,
    module_dir: &str,
    plugin: &str,
    files: &[(&str, &str)],
    main: &str,
) -> KslError {
    let (_root, path) = write_files_with_plugin(tag, module_dir, plugin, files, main);
    let mut sources = Sources::default();
    crate::module::load_and_check(&path, main, &mut sources).expect_err("模块应报错")
}

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
    assert!(graph.get(&ModuleId("lib/a".to_string())).is_some());
}

#[test]
fn empty_use_loads_nothing() {
    let graph = load_unlinked("empty_use", &[], r#"@Use[{}, "missing"]; nil"#);
    assert_eq!(graph.order().len(), 1);
}

#[test]
fn rejects_duplicate_and_cyclic_imports() {
    let error = load_unlinked_error(
        "duplicate",
        &[("lib/a.ksl", "@Data[A, {X[]}];")],
        r#"@Use[{A}, "lib/a"]; @Use[{A}, "lib/a"];"#,
    );
    assert!(matches!(error.kind, ErrorKind::DuplicateUse { .. }));

    let error = load_unlinked_error(
        "cycle",
        &[("lib/a.ksl", r#"@Use[{B}, "../main"]; @Data[A, {X[]}];"#)],
        r#"@Use[{A}, "lib/a"];"#,
    );
    assert!(matches!(error.kind, ErrorKind::CyclicImport { .. }));
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
        error.kind,
        ErrorKind::ModuleReadFailed { ref path, .. } if path.ends_with("lib/a.ksl.ksl")
    ));

    let error = load_unlinked_error("escape", &[], r#"@Use[{A}, "../a"];"#);
    assert!(matches!(error.kind, ErrorKind::ModulePathEscapesRoot { .. }));

    let error = load_error("absolute", &[], r#"@Use[{A}, "/a"];"#);
    assert!(matches!(
        error.kind,
        ErrorKind::InvalidModulePath { ref path } if path == "/a"
    ));
}

#[test]
fn links_exports_and_renames_types() {
    let graph = load_ok(
        "link",
        &[("lib/a.ksl", LIB_A_DATA)],
        r#"@Use[{^(A, NA)}, "lib/a"]; @Let[x : NA, NA/C[1]]; @Export[x, ^NA];"#,
    );
    let main_module = graph.get(&ModuleId("main".to_string())).expect("主模块应存在");
    assert!(main_module.program.stmts.iter().all(|stmt| {
        !matches!(
            &stmt.node,
            ksl_frontend::ast::ExprKind::Special(
                ksl_frontend::ast::Special::Export { .. }
                    | ksl_frontend::ast::Special::Use { .. }
            )
        )
    }));
    let exported = main_module
        .exports
        .types
        .get(&intern("NA"))
        .expect("NA 类型应导出");
    assert_eq!(&*resolve(exported.full), "lib/a/A");
    let x_ty = main_module
        .value_types
        .get(&intern("x"))
        .expect("x 类型应可确定");
    assert!(x_ty.as_data().is_some_and(|name| &*resolve(name) == "lib/a/A"));
}

#[test]
fn rejects_missing_export() {
    let error = load_error(
        "missing_export",
        &[("lib/a.ksl", "@Data[A, {C[]}];")],
        r#"@Use[{^A}, "lib/a"];"#,
    );
    assert!(matches!(error.kind, ErrorKind::ImportNotExported { .. }));
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
    assert!(matches!(error.kind, ErrorKind::ExportPrivateType { .. }));
}

#[test]
fn multiple_aliases_resolve_to_source_type() {
    let graph = load_ok(
        "alias",
        &[("lib/a.ksl", LIB_A_DATA)],
        r#"
@Use[{^(A, NA), ^(A, NB)}, "lib/a"];
@Let[x : NA, NA/C[1]];
@Let[y : NB, NB/C[2]];
@Export[x, y, ^NA, ^NB];
"#,
    );
    let main_module = graph.get(&ModuleId("main".to_string())).expect("主模块应存在");
    for name in ["NA", "NB"] {
        let exported = main_module
            .exports
            .types
            .get(&intern(name))
            .expect("别名应导出");
        assert_eq!(&*resolve(exported.full), "lib/a/A");
    }
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
    let main_module = graph.get(&ModuleId("main".to_string())).expect("主模块应存在");
    let x_ty = main_module
        .value_types
        .get(&intern("x"))
        .expect("x 类型应可确定");
    assert!(x_ty.as_data().is_some_and(|name| &*resolve(name) == "lib/a/D"));
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
    assert!(matches!(error.kind, ErrorKind::MultipleExportStatements { .. }));
}

#[test]
fn rejects_duplicate_imports() {
    let error = load_error(
        "import_conflict",
        &[
            ("lib/a.ksl", MAKE_ONE),
            ("lib/b.ksl", r"@Let[make, @Fn[[] : i64, 2]]; @Export[make];"),
        ],
        r#"@Use[{make}, "lib/a"]; @Use[{make}, "lib/b"];"#,
    );
    assert!(matches!(error.kind, ErrorKind::DuplicateImport { .. }));
}

#[test]
fn only_root_main_runs() {
    // 依赖模块的 `@Main` 只检查不执行; 其体含 `@Exit`, 若执行会以 7 结束.
    let graph = load_ok(
        "main_form",
        &[(
            "lib/a.ksl",
            r"@Let[dep, 1]; @Export[dep]; @Main[[self : Pid[nil, nil]] : nil, @Exit[7u]];",
        )],
        r#"@Use[{dep}, "lib/a"]; @Let[_, dep]; @Main[[self : Pid[nil, nil]] : nil, nil];"#,
    );
    let mut scheduler = crate::runtime::scheduler::Scheduler::new();
    let execution = execute(&mut scheduler, &graph).expect("模块应执行");
    let handler = execution.main_handler.expect("根模块应有 @Main");
    let result =
        scheduler.run_main(handler, execution.main_mailboxes.expect("@Main 应有信箱数"));
    assert!(matches!(result, Ok(Value::Nil)), "依赖的 @Main 不应执行");
    scheduler.shutdown();
}

#[test]
fn rejects_multiple_main_statements() {
    let error = load_error(
        "multiple_main",
        &[],
        r"@Main[[self : Pid[nil, nil]] : nil, nil]; @Main[[self : Pid[nil, nil]] : nil, nil];",
    );
    assert!(matches!(error.kind, ErrorKind::MultipleMainStatements { .. }));
}

#[test]
fn main_is_extracted_as_entry_declaration() {
    let graph = load_ok("main_entry", &[], "@Main[[self : Pid[nil, nil]] : nil, nil];\n");
    let module = graph.get(&graph.root).expect("根模块应存在");
    assert!(module.main.is_some(), "入口声明应被抽取");
    assert!(module.program.tail.is_none(), "程序尾部不应保留 @Main");
    assert!(
        !module.program.exprs().any(|expr| matches!(
            &expr.node,
            ksl_frontend::ast::ExprKind::Special(Special::Main { .. })
        )),
        "程序语句中不应保留 @Main"
    );
    assert!(
        module
            .main
            .as_ref()
            .is_some_and(|main| main.handler.pid.as_pid().is_some()),
        "入口应记录 Pid 蓝图"
    );
}

#[test]
fn module_tail_expression_is_a_statement() {
    let error = load_error(
        "main_tail_non_nil",
        &[],
        "@Main[[self : Pid[nil, nil]] : nil, nil];\n40\n",
    );
    assert!(matches!(error.kind, ErrorKind::UnusedValue { .. }));

    let error = load_error("tail_non_nil", &[], "40\n");
    assert!(matches!(error.kind, ErrorKind::UnusedValue { .. }));

    load_ok(
        "main_tail_discarded",
        &[],
        "@Main[[self : Pid[nil, nil]] : nil, nil];\n@Let[_, 40];\n",
    );
    load_ok("tail_nil", &[], "nil\n");
}

#[test]
fn execution_returns_only_root_exports() {
    let graph = load_ok(
        "execute",
        &[("lib/a.ksl", LIB_A_MAKE)],
        r#"@Use[{make, ^A}, "lib/a"]; @Let[v, make[3]]; @Export[v, ^A];"#,
    );
    let mut scheduler = crate::runtime::scheduler::Scheduler::new();
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");
    let value = execution
        .globals
        .get(&intern("v"))
        .cloned()
        .expect("v 应进入导出环境");
    let crate::value::Value::Data(data) = value else {
        panic!("v 应为 Data 值");
    };
    assert_eq!(&*resolve(data.ty), "lib/a/A");
    assert_eq!(&*resolve(data.variant), "C");
    assert!(!execution.globals.contains_key(&intern("make")));
    scheduler.shutdown();
}

#[test]
fn intermediate_modules_can_reexport() {
    let graph = load_ok(
        "reexport",
        &[
            ("lib/a.ksl", LIB_A_MAKE),
            ("lib/b.ksl", r#"@Use[{make, ^A}, "a"]; @Export[make, ^A];"#),
        ],
        r#"@Use[{(make, build), ^(A, NA)}, "lib/b"]; @Let[v, build[5]]; @Export[v, ^NA];"#,
    );
    let mut scheduler = crate::runtime::scheduler::Scheduler::new();
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");
    let value = execution
        .globals
        .get(&intern("v"))
        .cloned()
        .expect("v 应进入导出环境");
    let crate::value::Value::Data(data) = value else {
        panic!("v 应为 Data 值");
    };
    assert_eq!(&*resolve(data.ty), "lib/a/A");
    scheduler.shutdown();
}

#[test]
fn local_bindings_shadow_imports() {
    let graph = load_ok(
        "shadow",
        &[("lib/a.ksl", MAKE_ONE)],
        r#"@Use[{make}, "lib/a"]; @Let[make, @Fn[[] : i64, 2]]; @Let[x, make[]]; @Export[x];"#,
    );
    let mut scheduler = crate::runtime::scheduler::Scheduler::new();
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");
    let value = execution
        .globals
        .get(&intern("x"))
        .cloned()
        .expect("x 应进入导出环境");
    assert!(matches!(value, crate::value::Value::I64(2)));
    scheduler.shutdown();
}

#[test]
fn cache_rejects_reloading_a_module() {
    let root = temp_root("cache_reload");
    write(&root, "lib/a.ksl", LIB_A_DATA_EMPTY);
    let mut sources = Sources::default();
    let source = sources.add("<交互>", r#"@Use[{^A}, "lib/a"];"#);
    let program =
        ksl_frontend::parser::parse_text(sources.text(source), source).expect("导入应可解析");
    let mut cache = ModuleCache::new();
    load_and_check_virtual_with_cache(root.path(), "<交互>", program, &mut sources, &mut cache)
        .expect("首次加载应成功");
    let source = sources.add("<交互>", r#"@Use[{^A}, "lib/a"];"#);
    let program =
        ksl_frontend::parser::parse_text(sources.text(source), source).expect("导入应可解析");
    let error = load_and_check_virtual_with_cache(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
    )
    .expect_err("直接再次加载应报错");
    assert!(matches!(error.kind, ErrorKind::ModuleAlreadyLoaded { .. }));
}

#[test]
fn cache_reuses_dependencies() {
    let root = temp_root("cache_reuse");
    write(
        &root,
        "lib/a.ksl",
        r"@Data[A, {C[]}]; @Let[make, @Fn[[] : A, A/C[]]]; @Export[make, ^A];",
    );
    write(
        &root,
        "lib/b.ksl",
        r#"@Use[{make, ^A}, "a"]; @Let[x, make[]]; @Export[x, ^A];"#,
    );
    let mut sources = Sources::default();
    let source = sources.add("<交互>", r#"@Use[{make}, "lib/a"];"#);
    let program =
        ksl_frontend::parser::parse_text(sources.text(source), source).expect("导入应可解析");
    let mut cache = ModuleCache::new();
    let first = load_and_check_virtual_with_cache(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
    )
    .expect("首次加载应成功");
    let order: Vec<&str> = first.order.iter().map(ModuleId::as_str).collect();
    assert_eq!(order, ["lib/a", "<交互>"]);

    let source = sources.add("<交互>", r#"@Use[{x}, "lib/b"];"#);
    let program =
        ksl_frontend::parser::parse_text(sources.text(source), source).expect("导入应可解析");
    let second = load_and_check_virtual_with_cache(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
    )
    .expect("新模块应复用已加载依赖");
    let order: Vec<&str> = second.order.iter().map(ModuleId::as_str).collect();
    assert_eq!(order, ["lib/b", "<交互>"]);
    assert!(second.get(&ModuleId("lib/a".to_string())).is_none());
}

#[test]
fn virtual_root_resolves_relative_paths() {
    let root = temp_root("virtual");
    write(&root, "lib/a.ksl", LIB_A_DATA_EMPTY);
    let mut sources = Sources::default();
    let source = sources.add("<交互>", r#"@Use[{^A}, "lib/a"];"#);
    let program = ksl_frontend::parser::parse_text(sources.text(source), source)
        .expect("行内导入应可解析");
    let graph =
        load_virtual(root.path(), "<交互>", program, &mut sources).expect("行内导入应可加载");
    let order: Vec<&str> = graph.order().iter().map(ModuleId::as_str).collect();
    assert_eq!(order, ["lib/a", "<交互>"]);
    assert!(graph.get(&ModuleId("lib/a".to_string())).is_some());
}

#[test]
fn cached_exports_survive_cycle_collection() {
    let root = temp_root("gc_cache");
    write(&root, "lib/a.ksl", COUNT_DEF);
    let main = r#"@Use[{count}, "lib/a"]; @Let[v, count[3]]; @Export[v];"#;
    let path = write_path(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查");
    let mut scheduler = crate::runtime::scheduler::Scheduler::new();
    let mut cache = ModuleCache::new();
    let execution = crate::module::execute_with_cache(
        &mut scheduler,
        &graph,
        RootEnv::OnlyExports,
        &mut cache,
    )
    .expect("模块应可执行");
    drop(execution);

    let roots = cache.root_values();
    let closure = roots
        .iter()
        .find_map(|value| match value {
            Value::Closure(closure) => Some(Arc::clone(closure)),
            _ => None,
        })
        .expect("缓存应持有导出的递归函数");
    let weak = Arc::downgrade(&closure);
    scheduler.collect_cycles(&roots);

    let closure = weak.upgrade().expect("模块缓存根应保留导出闭包");
    assert!(!closure.captures().is_empty(), "模块缓存根不应被清空捕获");
    scheduler.shutdown();
}

#[test]
fn cached_exports_survive_collection_during_module_execution() {
    let root = temp_root("gc_module_roots");
    write(&root, "lib/a.ksl", COUNT_DEF);
    write(
        &root,
        "lib/b.ksl",
        r"
@Def[loop, [n : i64] : i64,
  @If[EqualI64[n, 0], 0, loop[SubI64[n, 1]]]];
@Let[x, loop[2000]];
@Export[x];
",
    );
    write(
        &root,
        "lib/c.ksl",
        r#"@Use[{count}, "a"]; @Let[y, count[1]]; @Export[y];"#,
    );
    let main =
        r#"@Use[{y}, "lib/c"]; @Use[{x}, "lib/b"]; @Use[{count}, "lib/a"]; @Let[_, count[1]];"#;
    let path = write_path(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查");

    let mut scheduler = crate::runtime::scheduler::Scheduler::new();
    scheduler.set_gc_threshold(1);
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");

    assert!(matches!(execution.main_value, Value::Nil));
    assert!(
        scheduler.collection_count() > 0,
        "长时间模块执行期间应发生自动收集"
    );
    scheduler.shutdown();
}

#[test]
fn rejects_unknown_exports() {
    let error = load_error(
        "export_unknown",
        &[("lib/a.ksl", "@Export[nope];")],
        r#"@Use[{A}, "lib/a"];"#,
    );
    assert!(matches!(
        error.kind,
        ErrorKind::ExportUnknown { ref name } if name == "nope"
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
        error.kind,
        ErrorKind::DuplicateExport { ref name } if name == "x"
    ));
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
        .get(&ModuleId("lib/ray".to_string()))
        .expect("依赖模块应存在");
    assert!(lib.exports.link_values.contains_key(&intern("LoadTexture")));
    assert!(lib.exports.link_types.contains_key(&intern("Texture")));
    let main = graph.get(&ModuleId("main".to_string())).expect("主模块应存在");
    let ty = main
        .value_types
        .get(&intern("t"))
        .expect("Link 返回值类型应可确定");
    let ksl_frontend::check::Type::Opaque(name) = ty else {
        panic!("Link 类型应为不透明类型: {ty}");
    };
    assert_eq!(&*resolve(*name), "$plugin/lib/plugin/libraylib/Texture");
}

#[test]
fn link_types_follow_plugin_path() {
    let root = temp_root("link_identity");
    write(&root, "lib/a/mod.ksl", r#"@Link["demo", { ^T }, { }];"#);
    write(&root, "lib/b/mod.ksl", r#"@Link["demo", { ^T }, { }];"#);
    let file = crate::ffi::plugin_file("demo").expect("测试插件名应合法");
    write(&root, &format!("lib/a/plugin/{file}"), "");
    write(&root, &format!("lib/b/plugin/{file}"), "");
    let main = r#"
@Use[{^(T, TA)}, "lib/a/mod"];
@Use[{^(T, TB)}, "lib/b/mod"];
nil
"#;
    let path = write_path(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("同名插件应互不影响");

    let full = |id: &str| {
        let module = graph.get(&ModuleId(id.to_string())).expect("依赖模块应存在");
        let target = module
            .exports
            .link_types
            .get(&intern("T"))
            .expect("Link 类型应导出");
        resolve(target.full)
    };
    assert_eq!(&*full("lib/a/mod"), "$plugin/lib/a/plugin/libdemo/T");
    assert_eq!(&*full("lib/b/mod"), "$plugin/lib/b/plugin/libdemo/T");
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
    assert!(matches!(error.kind, ErrorKind::ExportPrivateType { .. }));
}

#[test]
fn link_names_conflict_with_bindings() {
    let error = load_error_with_plugin(
        "link_param",
        "",
        "raylib",
        &[],
        r#"
@Link["raylib", { }, { Foo : Fn[[] : nil] }];
@Def[g, [Foo : i64] : nil, nil];
"#,
    );
    assert!(matches!(error.kind, ErrorKind::LinkNameConflict { .. }));

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
    assert!(matches!(error.kind, ErrorKind::LinkNameConflict { .. }));

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
    assert!(matches!(error.kind, ErrorKind::LinkNameConflict { .. }));

    let error = load_error_with_plugin(
        "link_import",
        "",
        "raylib",
        &[("lib/a.ksl", r"@Let[x, 1]; @Export[x];")],
        r#"
@Use[{(x, Foo)}, "lib/a"];
@Link["raylib", { }, { Foo : Fn[[] : nil] }];
"#,
    );
    assert!(matches!(error.kind, ErrorKind::LinkNameConflict { .. }));
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
    assert!(matches!(error.kind, ErrorKind::LinkExportConflict { .. }));
}

#[test]
fn link_signatures_reject_unsupported_types() {
    let main = r#"@Link["demo", {}, { f : Fn[[nil] : i64] }];"#;
    let (_root, path) = write_files_with_plugin("link_nil_param", "", "demo", &[], main);
    let mut sources = Sources::default();
    crate::module::load_and_check(&path, main, &mut sources).expect("nil 参数应被接受");

    let error = load_error_with_plugin(
        "link_signature",
        "",
        "demo",
        &[],
        r#"@Link["demo", {}, { f : Fn[[List[i64]] : i64] }];"#,
    );
    assert!(matches!(
        error.kind,
        ErrorKind::PluginUnsupportedSignature { .. }
    ));

    let error = load_error_with_plugin(
        "link_result_error",
        "",
        "demo",
        &[],
        r#"@Link["demo", {}, { f : Fn[[] : Result[i64, i64]] }];"#,
    );
    assert!(matches!(
        error.kind,
        ErrorKind::PluginUnsupportedSignature { .. }
    ));
}

#[cfg(unix)]
const PLUGIN_SOURCE: &str = r#"
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use ksl_plugin::prelude::*;

pub struct Widget {
    value: i64,
}

impl KslOpaque for Widget {
    const NAME: &'static str = "Widget";
    const RELEASE_THREAD: u32 = ksl_plugin::THREAD_RUNTIME;
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static INIT_THREAD_TOKEN: AtomicU64 = AtomicU64::new(0);
static PUMPED: AtomicUsize = AtomicUsize::new(0);
static UNLOAD_MARKER: Mutex<Option<String>> = Mutex::new(None);

#[ksl_plugin(name = "demo")]
mod plugin {
    use super::*;

    #[ksl_init]
    fn setup() -> Result<(), String> {
        INITIALIZED.store(true, Ordering::SeqCst);
        INIT_THREAD_TOKEN.store(thread_token(), Ordering::SeqCst);
        Ok(())
    }

    #[ksl_unload]
    #[ksl_exec(main)]
    fn teardown() {
        INITIALIZED.store(false, Ordering::SeqCst);
        if let Some(path) = UNLOAD_MARKER.lock().unwrap().as_deref() {
            std::fs::write(path, "unloaded").expect("卸载标记应可写入");
        }
    }

    #[ksl_event_pump]
    fn pump() {
        PUMPED.fetch_add(1, Ordering::SeqCst);
    }

    #[ksl_export]
    fn pump_count() -> u64 {
        u64::try_from(PUMPED.load(Ordering::SeqCst)).expect("计数应可转换")
    }

    #[ksl_export]
    fn was_initialized() -> bool {
        INITIALIZED.load(Ordering::SeqCst)
    }

    #[ksl_export]
    fn init_thread_token() -> u64 {
        INIT_THREAD_TOKEN.load(Ordering::SeqCst)
    }

    #[ksl_export]
    fn set_unload_marker(path: &str) {
        *UNLOAD_MARKER.lock().unwrap() = Some(path.to_string());
    }

    #[ksl_export]
    fn thread_token() -> u64 {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::format!("{:?}", std::thread::current().id()).hash(&mut hasher);
        hasher.finish()
    }

    #[ksl_export]
    fn add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_export]
    fn half(x: f64) -> f64 {
        x / 2.0
    }

    #[ksl_export]
    fn is_positive(x: i64) -> bool {
        x > 0
    }

    #[ksl_export]
    fn greet(name: &str) -> String {
        format!("hello {name}")
    }

    #[ksl_export]
    fn join(left: &str, right: &str) -> String {
        format!("{left}+{right}")
    }

    #[ksl_export]
    fn shout(value: char) -> char {
        value.to_ascii_uppercase()
    }

    #[ksl_export]
    fn parse(value: &str) -> Result<i64, String> {
        value.parse().map_err(|_| "bad".to_string())
    }

    #[ksl_export]
    #[ksl_exec(io)]
    fn io_add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_export]
    #[ksl_exec(cpu)]
    fn cpu_add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_export]
    fn main_add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_export("MakeWidget")]
    fn make_widget() -> Widget {
        Widget { value: 7 }
    }

    #[ksl_export]
    fn widget_value(widget: &Widget) -> i64 {
        widget.value * 6
    }

    #[ksl_global]
    const VERSION: i64 = 1;

    #[ksl_global]
    const TITLE: &str = "demo";

    #[ksl_global]
    const MAIN_ONLY: i64 = 2;
}
"#;

#[cfg(unix)]
const PLUGIN_LINK: &str = r#"
@Link["demo", { ^Widget }, {
  add : Fn[[i64, i64] : i64],
  half : Fn[[f64] : f64],
  is_positive : Fn[[i64] : bool],
  greet : Fn[[string] : string],
  join : Fn[[string, string] : string],
  shout : Fn[[char] : char],
  parse : Fn[[string] : Result[i64, string]],
  io_add : Fn[[i64, i64] : i64],
  cpu_add : Fn[[i64, i64] : i64],
  main_add : Fn[[i64, i64] : i64],
  MakeWidget : Fn[[] : Widget],
  widget_value : Fn[[Widget] : i64],
  VERSION : i64,
  TITLE : string,
  MAIN_ONLY : i64,
  was_initialized : Fn[[] : bool],
  init_thread_token : Fn[[] : u64],
  pump_count : Fn[[] : u64],
  set_unload_marker : Fn[[string] : nil],
  thread_token : Fn[[] : u64]
}];
"#;

#[cfg(unix)]
const PLUGIN_CROSS_MODULE: &str = r#"
@Use[{MakeWidget, ^Widget}, "ffi"];
@Link["demo", {}, { widget_value : Fn[[Widget] : i64] }];
@Export[make, value, ^Widget];
@Let[make, @Fn[[] : Widget, MakeWidget[]]];
@Let[value, @Fn[[w : Widget] : i64, widget_value[w]]];
"#;

#[cfg(unix)]
fn plugin_manifest() -> String {
    format!(
        r#"
[package]
name = "ksl-test-plugin"
version = "0.1.0"
edition = "2024"

[workspace]

[lib]
name = "ksl_test_plugin"
crate-type = ["cdylib"]
path = "src/lib.rs"

[dependencies]
ksl-plugin = {{ path = "{}" }}
"#,
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ksl-plugin").replace('\\', "/")
    )
}

#[cfg(unix)]
fn install_test_plugin(root: &TempDir) {
    use std::process::Command;

    write(root, "plugin-src/Cargo.toml", &plugin_manifest());
    write(root, "plugin-src/src/lib.rs", PLUGIN_SOURCE);
    let target = root.path().join("plugin-target");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args(["build", "--offline", "--manifest-path"])
        .arg(root.path().join("plugin-src/Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .status()
        .expect("应能启动 cargo");
    assert!(status.success(), "测试插件编译失败");

    let extension = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    let built = target
        .join("debug")
        .join(format!("libksl_test_plugin.{extension}"));
    let plugin_target = root.path().join("lib/plugin");
    fs::create_dir_all(&plugin_target).expect("插件目标目录应可创建");
    fs::copy(built, plugin_target.join(format!("libdemo.{extension}")))
        .expect("插件应复制到模块目录");
}

#[cfg(unix)]
fn plugin_program(unload_marker: &Path) -> String {
    format!(
        r#"
@Use[{{
  add, half, is_positive, greet, join, shout, parse, io_add, cpu_add, main_add,
  MakeWidget, widget_value, VERSION, TITLE, MAIN_ONLY, was_initialized,
  init_thread_token, pump_count, set_unload_marker, thread_token, ^Widget
}}, "lib/ffi"];
@Use[{{make, value}}, "lib/cross"];
@Let[w, MakeWidget[]];
@Assert[EqualI64[add[2, 3], 5], "add"];
@Assert[EqualString[f"{{add}}", "add : Fn[[i64, i64] : i64]"], "外来函数显示签名"];
@Assert[EqualF64[half[9.0], 4.5], "half"];
@Assert[is_positive[1], "positive"];
@Assert[EqualString[greet["ksl"], "hello ksl"], "greet"];
@Assert[EqualString[join["a", "b"], "a+b"], "join"];
@Assert[EqualChar[shout['a'], 'A'], "char"];
@Assert[EqualI64[@ResultGet[parse["42"], -1], 42], "parse ok"];
@Assert[EqualBool[@IsErr[parse["x"]], true], "parse err"];
@Assert[
  EqualString[@Match[parse["x"], {{ (/Ok[_], "ok"), (/Err[e], e) }}], "bad"],
  "parse message"
];
@Assert[EqualI64[widget_value[w], 42], "widget"];
@Let[w2, make[]];
@Assert[EqualI64[value[w2], 42], "cross module widget"];
@Assert[EqualI64[VERSION, 1], "version"];
@Assert[EqualString[TITLE, "demo"], "title"];
@Assert[EqualI64[io_add[20, 22], 42], "io call"];
@Assert[EqualI64[cpu_add[20, 22], 42], "cpu call"];
@Assert[EqualI64[main_add[20, 22], 42], "main call"];
@Let[runtime_token, thread_token[]];
@Assert[EqualU64[init_thread_token[], runtime_token], "初始化与调用在同一线程"];
@Assert[GreaterU64[pump_count[], 0U], "事件泵应在调度循环中运行"];
@Assert[EqualI64[MAIN_ONLY, 2], "main global"];
@Assert[was_initialized[], "插件初始化入口应在加载后调用"];
set_unload_marker["{}"];

@Main[[self : Pid[nil, nil]] : nil,
  @Block[
    @Let[actor_token, thread_token[]],
    @Assert[EqualU64[actor_token, runtime_token], "Actor 与 Prelude 在同一调度线程"]
  ]]
"#,
        unload_marker.display()
    )
}

#[cfg(unix)]
#[test]
fn calls_plugin_functions_and_opaque_handles() {
    let root = temp_root("plugin_call");
    install_test_plugin(&root);
    write(&root, "lib/ffi.ksl", PLUGIN_LINK);
    write(&root, "lib/cross.ksl", PLUGIN_CROSS_MODULE);
    let unload_marker = root.path().join("unload.marker");
    let main = write_path(&root, "main.ksl", &plugin_program(&unload_marker));

    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check_file(&main, &mut sources).expect("插件模块应通过检查");
    let mut scheduler = crate::runtime::scheduler::Scheduler::new();
    let execution = crate::module::execute(&mut scheduler, &graph).expect("插件调用应成功");
    let main_handler = execution.main_handler.clone().expect("程序应声明 @Main");
    let mailboxes = execution.main_mailboxes.expect("根 Actor 应有信箱数");
    scheduler
        .run_main(main_handler, mailboxes)
        .expect("根 Actor 应成功");
    // 执行结果与 Prelude 全局环境都持有外来函数值, 全部释放后才触发卸载入口.
    drop(execution);
    scheduler.shutdown();
    drop(scheduler);
    assert_eq!(
        fs::read_to_string(&unload_marker).expect("卸载入口应执行"),
        "unloaded"
    );
}

#[test]
fn rechecking_virtual_root_reuses_cached_dependencies() {
    // 文档重复检查时, 根模块直接 @Use 已缓存依赖应复用缓存.
    let root = temp_root("recheck_dep");
    write(&root, "lib/a.ksl", LIB_A_DATA_EMPTY);
    let text = r#"@Use[{^A}, "lib/a"];"#;
    let mut sources = Sources::default();
    let mut cache = ModuleCache::new();
    for _ in 0..2 {
        let source = sources.add("<doc>", text);
        let program = ksl_frontend::parser::parse_text(sources.text(source), source)
            .expect("导入应可解析");
        recheck_virtual_with_cache(root.path(), "main", program, &mut sources, &mut cache)
            .expect("重复检查同一文档应成功");
    }
}

#[test]
fn rechecking_virtual_root_keeps_its_data_out_of_cache() {
    // 根模块的 Data 声明不进入缓存, 重复检查不会判为重复声明.
    let root = temp_root("recheck_data");
    let text = r"@Data[A, {C[]}]; @Export[^A];";
    let mut sources = Sources::default();
    let mut cache = ModuleCache::new();
    for _ in 0..2 {
        let source = sources.add("<doc>", text);
        let program = ksl_frontend::parser::parse_text(sources.text(source), source)
            .expect("数据声明应可解析");
        recheck_virtual_with_cache(root.path(), "main", program, &mut sources, &mut cache)
            .expect("重复检查同一文档应成功");
    }
    assert!(!cache.data.contains_key(&intern("main/A")));
}
