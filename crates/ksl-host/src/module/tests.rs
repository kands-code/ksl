//! 模块执行测试.
//!
//! 执行顺序, 导出环境与运行缓存; 加载与链接由 `ksl-compile` 测试覆盖.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ksl_base::name::Name;
use ksl_base::source::Sources;
use ksl_base::testutil::{TempDir, write, write_path};
use ksl_compile::*;
use ksl_value::value::Value;

use super::*;
use crate::plugin::LibraryKeeper;

/// 多个用例共用的模块文本.
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

/// 用已读取的源码检查模块图.
fn checked_text(path: &Path, text: &str) -> CheckedGraph {
    let mut sources = Sources::default();
    compile(
        RootSource::Text { path, text },
        &mut sources,
        &mut CompileCache::new(),
    )
    .expect("模块应通过链接与检查")
}

/// 从磁盘文件检查模块图.
fn checked_file(path: &Path) -> CheckedGraph {
    let mut sources = Sources::default();
    compile(RootSource::File(path), &mut sources, &mut CompileCache::new())
        .expect("模块应通过链接与检查")
}

fn load_ok(tag: &str, files: &[(&str, &str)], main: &str) -> CheckedGraph {
    let (_root, path) = write_files(tag, files, main);
    checked_text(&path, main)
}

#[test]
fn only_root_module_main_runs() {
    // 依赖模块的 `@Main` 只检查不执行; 其体中的 `@Exit` 不会生效.
    let graph = load_ok(
        "main_form",
        &[(
            "lib/a.ksl",
            r"@Let[dep, 1]; @Export[dep]; @Main[[self : Pid[nil, nil]] : nil, @Exit[7u8]];",
        )],
        r#"@Use[{dep}, "lib/a"]; @Let[_, dep]; @Main[[self : Pid[nil, nil]] : nil, nil];"#,
    );
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut libraries = LibraryKeeper::default();
    let execution = execute(&mut scheduler, &graph, &mut libraries).expect("模块应执行");
    let handler = execution.main_handler.expect("根模块应有 @Main");
    let result =
        scheduler.run_main(handler, execution.main_mailboxes.expect("@Main 应有信箱数"));
    assert!(matches!(result, Ok(Value::Nil)), "依赖的 @Main 不应执行");
    scheduler.shutdown();
}

#[test]
fn execution_returns_only_root_exports() {
    let graph = load_ok(
        "execute",
        &[("lib/a.ksl", LIB_A_MAKE)],
        r#"@Use[{make, ^A}, "lib/a"]; @Let[v, make[3]]; @Export[v, ^A];"#,
    );
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut libraries = LibraryKeeper::default();
    execute(&mut scheduler, &graph, &mut libraries).expect("模块应可执行");
    let value = scheduler
        .globals()
        .get(&Name::new("v"))
        .cloned()
        .expect("v 应进入导出环境");
    let ksl_value::value::Value::Data(data) = value else {
        panic!("v 应为 Data 值");
    };
    assert_eq!(data.ty.as_str(), "lib/a/A");
    assert_eq!(data.variant.as_str(), "C");
    assert!(!scheduler.globals().contains_key(&Name::new("make")));
    scheduler.shutdown();
}

#[test]
fn module_directory_follows_the_defining_module() {
    let root = temp_root("module_dir");
    write(
        &root,
        "lib/a.ksl",
        r"
@Def[where, [] : string, GetModuleDirectory[]];
@Export[where];
",
    );
    let main = r#"
@Use[{where}, "lib/a"];
@Let[rootDir, GetModuleDirectory[]];
@Let[libDir, where[]];
@Export[rootDir, libDir];
"#;
    let path = write_path(&root, "main.ksl", main);
    let graph = checked_text(&path, main);
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut libraries = LibraryKeeper::default();
    execute(&mut scheduler, &graph, &mut libraries).expect("模块应可执行");
    let directory = |name: &str| match scheduler.globals().get(&Name::new(name)) {
        Some(Value::Str(text)) => text.to_string(),
        other => panic!("{name} 应为字符串: {other:?}"),
    };
    let expected_root = fs::canonicalize(root.path()).expect("临时目录应可规范化");
    let expected_lib = expected_root.join("lib");
    assert_eq!(directory("rootDir"), expected_root.to_string_lossy());
    assert_eq!(directory("libDir"), expected_lib.to_string_lossy());
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
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut libraries = LibraryKeeper::default();
    execute(&mut scheduler, &graph, &mut libraries).expect("模块应可执行");
    let value = scheduler
        .globals()
        .get(&Name::new("v"))
        .cloned()
        .expect("v 应进入导出环境");
    let ksl_value::value::Value::Data(data) = value else {
        panic!("v 应为 Data 值");
    };
    assert_eq!(data.ty.as_str(), "lib/a/A");
    scheduler.shutdown();
}

#[test]
fn local_bindings_shadow_imports() {
    let graph = load_ok(
        "shadow",
        &[("lib/a.ksl", MAKE_ONE)],
        r#"@Use[{make}, "lib/a"]; @Let[make, @Fn[[] : i64, 2]]; @Let[x, make[]]; @Export[x];"#,
    );
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut libraries = LibraryKeeper::default();
    execute(&mut scheduler, &graph, &mut libraries).expect("模块应可执行");
    let value = scheduler
        .globals()
        .get(&Name::new("x"))
        .cloned()
        .expect("x 应进入导出环境");
    assert!(matches!(value, ksl_value::value::Value::I64(2)));
    scheduler.shutdown();
}

#[test]
fn failed_execution_keeps_cache_unchanged() {
    let root = temp_root("cache_failure");
    write(&root, "lib/a.ksl", r"@Let[x, 1]; @Export[x];");
    let main = r#"@Use[{x}, "lib/a"]; @Let[_, DivideI64[1, 0]];"#;
    let path = write_path(&root, "main.ksl", main);
    let graph = checked_text(&path, main);
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut cache = InstanceCache::new();
    let mut libraries = LibraryKeeper::default();
    let result = execute_with_cache(
        &mut scheduler,
        &graph,
        RootEnv::OnlyExports,
        &mut cache,
        &mut libraries,
    );
    assert!(result.is_err(), "根模块除零应使执行失败");
    assert!(cache.instances.is_empty(), "执行失败时不应缓存导入模块的导出值");
    scheduler.shutdown();
}

#[test]
fn cached_exports_survive_cycle_collection() {
    let root = temp_root("gc_cache");
    write(&root, "lib/a.ksl", COUNT_DEF);
    let main = r#"@Use[{count}, "lib/a"]; @Let[v, count[3]]; @Export[v];"#;
    let path = write_path(&root, "main.ksl", main);
    let graph = checked_text(&path, main);
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut cache = InstanceCache::new();
    let mut libraries = LibraryKeeper::default();
    let execution = execute_with_cache(
        &mut scheduler,
        &graph,
        RootEnv::OnlyExports,
        &mut cache,
        &mut libraries,
    )
    .expect("模块应可执行");
    drop(execution);
    assert!(
        cache
            .instances
            .contains_key(&ModuleId::new("lib/a".to_string())),
        "成功执行后依赖模块应进入缓存"
    );
    assert!(!cache.instances.contains_key(&graph.root), "根模块不应进入缓存");

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
    assert!(closure.has_captures(), "模块缓存根不应被清空捕获");
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
    let graph = checked_text(&path, main);

    let mut scheduler = ksl_runtime::Scheduler::new();
    scheduler.set_gc_threshold(1);
    let mut libraries = LibraryKeeper::default();
    let execution = execute(&mut scheduler, &graph, &mut libraries).expect("模块应可执行");

    assert!(matches!(execution.main_value, Value::Nil));
    assert!(
        scheduler.collection_count() > 0,
        "长时间模块执行期间应发生自动收集"
    );
    scheduler.shutdown();
}

#[cfg(unix)]
fn demo_plugin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/demo")
}

/// 构建 demo 插件, 并把模块与动态库复制到临时根目录.
///
/// 构建目录固定在工作区 `target/tmp` 下, 让 cargo 在多次测试运行间复用缓存.
#[cfg(unix)]
fn install_demo_plugin(root: &TempDir) {
    use std::process::Command;

    let source = demo_plugin_dir();
    let target =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/ksl-demo-plugin-target");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args(["build", "--offline", "--manifest-path"])
        .arg(source.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target)
        .status()
        .expect("应能启动 cargo");
    assert!(status.success(), "demo 插件编译失败");

    let extension = if cfg!(target_os = "macos") {
        "dylib"
    } else {
        "so"
    };
    let built = target
        .join("debug")
        .join(format!("libksl_demo_plugin.{extension}"));
    let plugin_target = root.path().join("plugin");
    fs::create_dir_all(&plugin_target).expect("插件目标目录应可创建");
    fs::copy(built, plugin_target.join(format!("libdemo.{extension}")))
        .expect("插件应复制到模块目录");
    for file in ["demo.ksl", "cross.ksl"] {
        fs::copy(source.join(file), root.path().join(file)).expect("测试模块应复制到临时目录");
    }
    let runtime = fs::read_to_string(source.join("runtime.ksl")).expect("运行时脚本应可读取");
    let marker = format!("{:?}", root.path().join("unload.marker").display().to_string());
    fs::write(
        root.path().join("runtime.ksl"),
        runtime.replace("\"__UNLOAD_MARKER__\"", &marker),
    )
    .expect("运行时脚本应可写入");
}

#[cfg(unix)]
#[test]
fn runs_demo_plugin() {
    let root = temp_root("demo_plugin");
    install_demo_plugin(&root);
    let unload_marker = root.path().join("unload.marker");
    let main = root.path().join("runtime.ksl");

    let graph = checked_file(&main);
    let mut scheduler = ksl_runtime::Scheduler::new();
    let mut libraries = LibraryKeeper::default();
    let execution = execute(&mut scheduler, &graph, &mut libraries).expect("插件调用应成功");
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
