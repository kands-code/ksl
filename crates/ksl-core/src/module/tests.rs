//! 模块加载器测试.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::testutil::TempDir;

fn temp_root(tag: &str) -> TempDir {
    TempDir::new(&format!("ksl_module_{tag}_"))
}

fn write(root: &TempDir, relative: &str, text: &str) -> PathBuf {
    let path = root.path().join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("模块目录应可创建");
    }
    fs::write(&path, text).expect("模块文件应可写入");
    path
}

fn load_text(root: &TempDir, text: &str) -> Result<ModuleGraph, KslError> {
    let path = write(root, "main.ksl", text);
    let mut sources = Sources::default();
    load(&path, text, &mut sources)
}

#[test]
fn loads_dependencies_and_rejects_invalid_imports() {
    // 依赖按后序排列, 根模块最后.
    let root = temp_root("order");
    write(
        &root,
        "lib/a.ksl",
        "@Use[{B}, \"b\"];\n@Data[A, {X[]}];\n@Export[^A];\n",
    );
    write(&root, "lib/b.ksl", "@Data[B, {Y[]}];\n@Export[^B];\n");
    let graph = load_text(&root, "@Use[{A}, \"lib/a\"];\n@Use[{B}, \"lib/b\"];\n")
        .expect("模块应可加载");
    let order: Vec<&str> = graph.order().iter().map(ModuleId::as_str).collect();
    assert_eq!(order, ["lib/b", "lib/a", "main"]);
    assert_eq!(graph.root().as_str(), "main");
    assert!(graph.get(&ModuleId("lib/a".to_string())).is_some());

    // 路径统一追加 .ksl 后缀, 写全后缀会再次追加.
    let root = temp_root("suffix");
    write(&root, "lib/a.ksl", "@Data[A, {X[]}];\n");
    let error = load_text(&root, "@Use[{A}, \"lib/a.ksl\"];\n").expect_err("后缀应被再次追加");
    assert!(matches!(
        error.kind,
        ErrorKind::ModuleReadFailed { ref path, .. } if path.ends_with("lib/a.ksl.ksl")
    ));

    // 不允许越出根目录.
    let root = temp_root("escape");
    let error = load_text(&root, "@Use[{A}, \"../a\"];\n").expect_err("不得越出根目录");
    assert!(matches!(error.kind, ErrorKind::ModulePathEscapesRoot { .. }));

    // 重复导入, 循环导入都报错.
    let root = temp_root("duplicate");
    write(&root, "lib/a.ksl", "@Data[A, {X[]}];\n");
    let error = load_text(&root, "@Use[{A}, \"lib/a\"];\n@Use[{A}, \"lib/a\"];\n")
        .expect_err("重复导入应报错");
    assert!(matches!(error.kind, ErrorKind::DuplicateUse { .. }));

    let root = temp_root("cycle");
    write(
        &root,
        "lib/a.ksl",
        "@Use[{B}, \"../main\"];\n@Data[A, {X[]}];\n",
    );
    let error = load_text(&root, "@Use[{A}, \"lib/a\"];\n").expect_err("循环导入应报错");
    assert!(matches!(error.kind, ErrorKind::CyclicImport { .. }));

    // 空 @Use 不加载依赖.
    let root = temp_root("empty_use");
    let graph = load_text(&root, "@Use[{}, \"missing\"];\nnil").expect("空 @Use 不应加载依赖");
    assert_eq!(graph.order().len(), 1);
}

#[test]
fn links_exports_and_namespaces() {
    // 导入名可重命名; 类型名在模块全名下链接.
    let root = temp_root("link");
    write(&root, "lib/a.ksl", "@Data[A, {C[i64]}];\n@Export[^A];\n");
    let main = "@Use[{^(A, NA)}, \"lib/a\"];\n@Let[x : NA, NA/C[1]];\n@Export[x, ^NA];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查");
    let main_module = graph.get(&ModuleId("main".to_string())).expect("主模块应存在");
    assert!(main_module.program.stmts.iter().all(|stmt| {
        !matches!(
            &stmt.node,
            crate::ast::ExprKind::Special(
                crate::ast::Special::Export { .. } | crate::ast::Special::Use { .. }
            )
        )
    }));
    let exported = main_module
        .exports
        .types
        .get(&crate::symbol::intern("NA"))
        .expect("NA 类型应导出");
    assert_eq!(&*crate::symbol::resolve(exported.full), "lib/a/A");
    let x_ty = main_module
        .value_types
        .get(&crate::symbol::intern("x"))
        .expect("x 类型应可确定");
    assert!(matches!(
        x_ty,
        crate::check::Type::Data(name) if &*crate::symbol::resolve(*name) == "lib/a/A"
    ));

    // 未导出的类型不能被导入.
    let root = temp_root("missing_export");
    write(&root, "lib/a.ksl", "@Data[A, {C[]}];\n");
    let main = "@Use[{^A}, \"lib/a\"];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let error = crate::module::load_and_check(&path, main, &mut sources)
        .expect_err("未导出的类型不能被导入");
    assert!(matches!(error.kind, ErrorKind::ImportNotExported { .. }));

    // 导出值签名中的私有类型报错.
    let root = temp_root("private_export");
    write(
        &root,
        "lib/a.ksl",
        "@Data[Hidden, {Secret[i64]}];\n@Let[get, @Fn[[] : Hidden, \
         Hidden/Secret[7]]];\n@Export[get];\n",
    );
    let main = "@Use[{get}, \"lib/a\"];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let error = crate::module::load_and_check(&path, main, &mut sources)
        .expect_err("导出值签名中的私有类型应报错");
    assert!(matches!(error.kind, ErrorKind::ExportPrivateType { .. }));

    // 同一类型的两个别名都指向原类型.
    let root = temp_root("alias");
    write(&root, "lib/a.ksl", "@Data[A, {C[i64]}];\n@Export[^A];\n");
    let main = "@Use[{^(A, NA), ^(A, NB)}, \"lib/a\"];\n@Let[x : NA, NA/C[1]];\n@Let[y : NB, \
                NB/C[2]];\n@Export[x, y, ^NA, ^NB];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph = crate::module::load_and_check(&path, main, &mut sources)
        .expect("两个别名应指向同一类型");
    let main_module = graph.get(&ModuleId("main".to_string())).expect("主模块应存在");
    let expected = "lib/a/A";
    for name in ["NA", "NB"] {
        let exported = main_module
            .exports
            .types
            .get(&crate::symbol::intern(name))
            .expect("别名应导出");
        assert_eq!(&*crate::symbol::resolve(exported.full), expected);
    }

    // 尾 @Data 按模块全名链接并导出.
    let root = temp_root("tail_data");
    write(&root, "lib/a.ksl", "@Export[^D];\n@Data[D, {C[i64]}]\n");
    let main = "@Use[{^D}, \"lib/a\"];\n@Let[x : D, D/C[1]];\n@Export[x, ^D];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph = crate::module::load_and_check(&path, main, &mut sources)
        .expect("尾 @Data 应按模块全名链接并导出");
    let main_module = graph.get(&ModuleId("main".to_string())).expect("主模块应存在");
    let x_ty = main_module
        .value_types
        .get(&crate::symbol::intern("x"))
        .expect("x 类型应可确定");
    assert!(matches!(
        x_ty,
        crate::check::Type::Data(name) if &*crate::symbol::resolve(*name) == "lib/a/D"
    ));

    // 尾 @Use 与尾 @Export 同样按声明处理.
    let root = temp_root("tail_decls");
    write(&root, "lib/a.ksl", "@Data[A, {C[]}];\n@Export[^A]\n");
    let main = "@Use[{^A}, \"lib/a\"]\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph = crate::module::load_and_check(&path, main, &mut sources)
        .expect("尾 @Use 与尾 @Export 应按声明处理");
    assert_eq!(graph.order[0].as_str(), "lib/a");
}

#[test]
fn builtin_data_types_are_visible_in_export_signatures() {
    let root = temp_root("builtin_export");
    write(
        &root,
        "lib/a.ksl",
        "@Def[parse, [s : string] : Result[DateTime, TimeError], \
         ParseDateTime[s]];\n@Export[parse];\n",
    );
    let main = "@Use[{parse}, \"lib/a\"];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    crate::module::load_and_check(&path, main, &mut sources)
        .expect("内置时间类型应可用于导出签名");
}

#[test]
fn rejects_export_and_import_conflicts() {
    // 同一模块只允许一个导出声明.
    let root = temp_root("export");
    let main = "@Let[x, 1];\n@Export[x];\n@Let[y, 2];\n@Export[y];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let error = crate::module::load_and_check(&path, main, &mut sources)
        .expect_err("同一模块只允许一个导出声明");
    assert!(matches!(error.kind, ErrorKind::MultipleExportStatements { .. }));

    // 两个导入值同名报错.
    let root = temp_root("import_conflict");
    write(
        &root,
        "lib/a.ksl",
        "@Let[make, @Fn[[] : i64, 1]];\n@Export[make];\n",
    );
    write(
        &root,
        "lib/b.ksl",
        "@Let[make, @Fn[[] : i64, 2]];\n@Export[make];\n",
    );
    let main = "@Use[{make}, \"lib/a\"];\n@Use[{make}, \"lib/b\"];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let error = crate::module::load_and_check(&path, main, &mut sources)
        .expect_err("两个导入值同名应报错");
    assert!(matches!(error.kind, ErrorKind::DuplicateImport { .. }));
}

#[test]
fn executes_modules_and_reexports() {
    // 只把主模块的导出放入结果环境.
    let root = temp_root("execute");
    write(
        &root,
        "lib/a.ksl",
        "@Data[A, {C[i64]}];\n@Let[make, @Fn[[n : i64] : A, A/C[n]]];\n@Export[make, ^A];\n",
    );
    let main = "@Use[{make, ^A}, \"lib/a\"];\n@Let[v, make[3]];\n@Export[v, ^A];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查");
    let mut scheduler = crate::cps::scheduler::Scheduler::new();
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");
    let value = execution
        .globals
        .get(&crate::symbol::intern("v"))
        .cloned()
        .expect("v 应进入导出环境");
    let crate::value::Value::Data(data) = value else {
        panic!("v 应为 Data 值");
    };
    assert_eq!(&*crate::symbol::resolve(data.ty), "lib/a/A");
    assert_eq!(&*crate::symbol::resolve(data.variant), "C");
    assert!(!execution.globals.contains_key(&crate::symbol::intern("make")));
    scheduler.shutdown();

    // 中间模块可再次导出上一模块的类型与值.
    let root = temp_root("reexport");
    write(
        &root,
        "lib/a.ksl",
        "@Data[A, {C[i64]}];\n@Let[make, @Fn[[n : i64] : A, A/C[n]]];\n@Export[make, ^A];\n",
    );
    write(
        &root,
        "lib/b.ksl",
        "@Use[{make, ^A}, \"a\"];\n@Export[make, ^A];\n",
    );
    let main =
        "@Use[{(make, build), ^(A, NA)}, \"lib/b\"];\n@Let[v, build[5]];\n@Export[v, ^NA];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph = crate::module::load_and_check(&path, main, &mut sources)
        .expect("re-export 应通过链接与检查");
    let mut scheduler = crate::cps::scheduler::Scheduler::new();
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");
    let value = execution
        .globals
        .get(&crate::symbol::intern("v"))
        .cloned()
        .expect("v 应进入导出环境");
    let crate::value::Value::Data(data) = value else {
        panic!("v 应为 Data 值");
    };
    assert_eq!(&*crate::symbol::resolve(data.ty), "lib/a/A");
    scheduler.shutdown();

    // 本地值遮蔽同名导入值.
    let root = temp_root("shadow");
    write(
        &root,
        "lib/a.ksl",
        "@Let[make, @Fn[[] : i64, 1]];\n@Export[make];\n",
    );
    let main = "@Use[{make}, \"lib/a\"];\n@Let[make, @Fn[[] : i64, 2]];\n@Let[x, \
                make[]];\n@Export[x];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph = crate::module::load_and_check(&path, main, &mut sources)
        .expect("本地值应遮蔽同名导入值");
    let mut scheduler = crate::cps::scheduler::Scheduler::new();
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");
    let value = execution
        .globals
        .get(&crate::symbol::intern("x"))
        .cloned()
        .expect("x 应进入导出环境");
    assert!(matches!(value, crate::value::Value::I64(2)));
    scheduler.shutdown();
}

#[test]
fn module_cache_and_virtual_roots() {
    // 同一模块不能在同一缓存中直接再次加载.
    let root = temp_root("cache_reload");
    write(&root, "lib/a.ksl", "@Data[A, {C[]}];\n@Export[^A];\n");
    let mut sources = Sources::default();
    let source = sources.add("<交互>", "@Use[{^A}, \"lib/a\"];\n");
    let program =
        crate::parser::parse_text(sources.text(source), source).expect("导入应可解析");
    let mut cache = ModuleCache::new();
    load_and_check_virtual_with_cache(root.path(), "<交互>", program, &mut sources, &mut cache)
        .expect("首次加载应成功");
    let source = sources.add("<交互>", "@Use[{^A}, \"lib/a\"];\n");
    let program =
        crate::parser::parse_text(sources.text(source), source).expect("导入应可解析");
    let error = load_and_check_virtual_with_cache(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
    )
    .expect_err("直接再次加载应报错");
    assert!(matches!(error.kind, ErrorKind::ModuleAlreadyLoaded { .. }));

    // 新模块复用缓存中的依赖, 不重复加载.
    let root = temp_root("cache_reuse");
    write(
        &root,
        "lib/a.ksl",
        "@Data[A, {C[]}];\n@Let[make, @Fn[[] : A, A/C[]]];\n@Export[make, ^A];\n",
    );
    write(
        &root,
        "lib/b.ksl",
        "@Use[{make, ^A}, \"a\"];\n@Let[x, make[]];\n@Export[x, ^A];\n",
    );
    let mut sources = Sources::default();
    let source = sources.add("<交互>", "@Use[{make}, \"lib/a\"];\n");
    let program =
        crate::parser::parse_text(sources.text(source), source).expect("导入应可解析");
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

    let source = sources.add("<交互>", "@Use[{x}, \"lib/b\"];\n");
    let program =
        crate::parser::parse_text(sources.text(source), source).expect("导入应可解析");
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

    // 虚拟根按根目录解析相对路径.
    let root = temp_root("virtual");
    write(&root, "lib/a.ksl", "@Data[A, {C[]}];\n@Export[^A];\n");
    let mut sources = Sources::default();
    let source = sources.add("<交互>", "@Use[{^A}, \"lib/a\"];\n");
    let program =
        crate::parser::parse_text(sources.text(source), source).expect("行内导入应可解析");
    let graph =
        load_virtual(root.path(), "<交互>", program, &mut sources).expect("行内导入应可加载");
    let order: Vec<&str> = graph.order().iter().map(ModuleId::as_str).collect();
    assert_eq!(order, ["lib/a", "<交互>"]);
    assert!(graph.get(&ModuleId("lib/a".to_string())).is_some());
}

#[test]
fn module_roots_survive_gc() {
    // 模块缓存持有导出闭包, 环回收后捕获仍在.
    let root = temp_root("gc_cache");
    write(
        &root,
        "lib/a.ksl",
        "@Def[count, [n : i64] : i64,\n    @If[EqualI64[n, 0], 0, count[SubI64[n, \
         1]]]];\n@Export[count];\n",
    );
    let main = "@Use[{count}, \"lib/a\"];\n@Let[v, count[3]];\n@Export[v];\n";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查");
    let mut scheduler = crate::cps::scheduler::Scheduler::new();
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

    // 模块执行期间触发自动收集时, 已缓存导出仍是根.
    let root = temp_root("gc_module_roots");
    write(
        &root,
        "lib/a.ksl",
        "@Def[count, [n : i64] : i64,\n    @If[EqualI64[n, 0], 0, count[SubI64[n, \
         1]]]];\n@Export[count];\n",
    );
    write(
        &root,
        "lib/b.ksl",
        "@Def[loop, [n : i64] : i64,\n    @If[EqualI64[n, 0], 0, loop[SubI64[n, \
         1]]]];\n@Let[x, loop[2000]];\n@Export[x];\n",
    );
    write(
        &root,
        "lib/c.ksl",
        "@Use[{count}, \"a\"];\n@Let[y, count[1]];\n@Export[y];\n",
    );
    let main =
        "@Use[{y}, \"lib/c\"];\n@Use[{x}, \"lib/b\"];\n@Use[{count}, \"lib/a\"];\ncount[1]";
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查");

    let mut scheduler = crate::cps::scheduler::Scheduler::new();
    scheduler.set_gc_threshold(1);
    let execution = crate::module::execute(&mut scheduler, &graph).expect("模块应可执行");

    assert!(matches!(execution.main_value, Value::I64(0)));
    assert!(
        scheduler.collection_count() > 0,
        "长时间模块执行期间应发生自动收集"
    );
    scheduler.shutdown();
}

#[test]
fn module_type_tables_are_recorded() {
    let root = temp_root("type_table");
    write(&root, "lib/a.ksl", "@Data[A, {C[i64]}];\n@Export[^A];\n");
    let main = r#"@Use[{^A}, "lib/a"]; @Let[x : A, A/C[1]];"#;
    let path = write(&root, "main.ksl", main);
    let mut sources = Sources::default();
    let graph =
        crate::module::load_and_check(&path, main, &mut sources).expect("模块应通过链接与检查");
    let dependency = graph
        .get(&ModuleId("lib/a".to_string()))
        .expect("依赖模块应存在");
    assert!(
        dependency
            .types
            .values()
            .any(|ty| matches!(ty, crate::check::Type::Data(_))),
        "依赖模块应记录 Data 类型"
    );
    let main_module = graph.get(&graph.root).expect("主模块应存在");
    assert!(!main_module.types.is_empty(), "主模块应记录类型表");
}
