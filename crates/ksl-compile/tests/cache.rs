//! 编译缓存: 复用, 重载与失效.

use ksl_base::name::Name;
use ksl_base::source::Sources;
use ksl_base::testutil::write;
use ksl_compile::{CachedImports, CompileCache, ModuleId};
use ksl_diagnostics::error::ErrorKind;

mod common;
use common::*;

#[test]
fn cache_rejects_reloading_a_module() {
    let root = temp_root("cache_reload");
    write(&root, "lib/a.ksl", LIB_A_DATA_EMPTY);
    let mut sources = Sources::default();
    let mut cache = CompileCache::watching();
    let program = virtual_program(&mut sources, "<交互>", r#"@Use[{^A}, "lib/a"];"#);
    compile_virtual(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
        CachedImports::Reject,
    )
    .expect("首次加载应成功");
    let program = virtual_program(&mut sources, "<交互>", r#"@Use[{^A}, "lib/a"];"#);
    let error = compile_virtual(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
        CachedImports::Reject,
    )
    .expect_err("直接再次加载应报错");
    assert_error_kind!(error, ErrorKind::ModuleAlreadyLoaded { .. });
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
    let mut cache = CompileCache::new();
    let program = virtual_program(&mut sources, "<交互>", r#"@Use[{make}, "lib/a"];"#);
    let first = compile_virtual(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
        CachedImports::Reject,
    )
    .expect("首次加载应成功");
    assert_eq!(order_of(&first.order), ["lib/a", "<交互>"]);

    let program = virtual_program(&mut sources, "<交互>", r#"@Use[{x}, "lib/b"];"#);
    let second = compile_virtual(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
        CachedImports::Reject,
    )
    .expect("新模块应复用已加载依赖");
    assert_eq!(order_of(&second.order), ["lib/b", "<交互>"]);
    assert!(second.get(&ModuleId::new("lib/a".to_string())).is_none());
}

#[test]
fn rechecking_virtual_root_reuses_cached_dependencies() {
    // 文档重复检查时, 根模块直接 @Use 已缓存依赖应复用缓存.
    let root = temp_root("recheck_dep");
    write(&root, "lib/a.ksl", LIB_A_DATA_EMPTY);
    let text = r#"@Use[{^A}, "lib/a"];"#;
    let mut sources = Sources::default();
    let mut cache = CompileCache::new();
    for _ in 0..2 {
        let program = virtual_program(&mut sources, "<doc>", text);
        compile_virtual(
            root.path(),
            "main",
            program,
            &mut sources,
            &mut cache,
            CachedImports::Allow,
        )
        .expect("重复检查同一文档应成功");
    }
}

#[test]
fn rechecking_virtual_root_reuses_cached_type_declaration() {
    let root = temp_root("recheck_type_decl");
    write(
        &root,
        "lib/a.ksl",
        r"@Data[A, {C[i64]}]; @Let[make, @Fn[[] : A, A/C[1]]]; @Export[make, ^A];",
    );
    let text = r#"@Use[{make, ^A}, "lib/a"]; @Let[n, @Match[make[], { (/C[v], v) }]];"#;
    let mut sources = Sources::default();
    let mut cache = CompileCache::new();
    for _ in 0..2 {
        let program = virtual_program(&mut sources, "<doc>", text);
        compile_virtual(
            root.path(),
            "main",
            program,
            &mut sources,
            &mut cache,
            CachedImports::Allow,
        )
        .expect("重复检查同一文档应复用缓存中的类型声明");
    }
}

#[test]
fn rechecking_virtual_root_keeps_its_data_out_of_cache() {
    // 根模块的 Data 声明不进入缓存, 重复检查不会判为重复声明.
    let root = temp_root("recheck_data");
    let text = r"@Data[A, {C[]}]; @Export[^A];";
    let mut sources = Sources::default();
    let mut cache = CompileCache::new();
    for _ in 0..2 {
        let program = virtual_program(&mut sources, "<doc>", text);
        compile_virtual(
            root.path(),
            "main",
            program,
            &mut sources,
            &mut cache,
            CachedImports::Allow,
        )
        .expect("重复检查同一文档应成功");
    }
    assert!(!cache.has_data(&Name::new("main/A")));
}

#[test]
fn cache_invalidates_when_dependency_file_changes() {
    let root = temp_root("cache_stale");
    write(&root, "lib/a.ksl", LIB_A_DATA_EMPTY);
    let mut sources = Sources::default();
    let mut cache = CompileCache::watching();
    let program = virtual_program(&mut sources, "<交互>", r#"@Use[{^A}, "lib/a"];"#);
    compile_virtual(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
        CachedImports::Reject,
    )
    .expect("首次加载应成功");

    // 新增变体; 缓存若未失效, 第二次检查会找不到 A/D.
    write(&root, "lib/a.ksl", r"@Data[A, {C[], D[i64]}]; @Export[^A];");
    let program = virtual_program(
        &mut sources,
        "<交互>",
        r#"@Use[{^A}, "lib/a"]; @Let[_, A/D[1]];"#,
    );
    compile_virtual(
        root.path(),
        "<交互>",
        program,
        &mut sources,
        &mut cache,
        CachedImports::Reject,
    )
    .expect("依赖变化后应重新加载");
}
