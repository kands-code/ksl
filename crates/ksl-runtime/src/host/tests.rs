//! 宿主会话测试.
//!
//! 覆盖交互输入与初始化文件的会话语义; 终端交互由命令行测试覆盖.

use ksl_base::symbol::resolve;
use ksl_frontend::check::{display_type, is_printable};
use ksl_frontend::error::{ErrorKind, KslError};

use super::*;
use crate::fmtstr::display_value;
use crate::testutil::{TempDir, write, write_path};
use crate::value::Value;

fn temp_root(tag: &str) -> TempDir {
    TempDir::new(&format!("ksl_host_{tag}_"))
}

fn eval(host: &mut Host, text: &str) -> InputOutcome {
    host.eval_input("<交互>", text).expect("交互输入应可解析并检查")
}

fn eval_value(host: &mut Host, text: &str) -> Value {
    eval(host, text).result.expect("交互输入应执行成功").0
}

fn error(host: &mut Host, text: &str) -> KslError {
    match host.eval_input("<交互>", text) {
        Ok(_) => panic!("交互输入应失败: {text}"),
        Err(error) => error,
    }
}

fn assert_i64(value: &Value, expected: i64) {
    match value {
        Value::I64(value) => assert_eq!(*value, expected),
        _ => panic!("结果应为 i64"),
    }
}

#[test]
fn interactive_input_rejects_module_forms() {
    let root = temp_root("forms");
    let mut host = Host::new(root.path().to_path_buf());

    let failure = error(&mut host, "@Main[[self : Pid[nil, nil]] : nil, nil]");
    assert_eq!(failure.kind, ErrorKind::MainOutsideModule);

    let failure = error(&mut host, "@Export[x];");
    assert_eq!(failure.kind, ErrorKind::ExportOutsideModule);

    let failure = error(&mut host, r#"@Link["x", { }, { f : Fn[[] : nil] }];"#);
    assert!(matches!(failure.kind, ErrorKind::LinkOutsideModule));
}

#[test]
fn module_exports_survive_gc_during_main() {
    let root = temp_root("gc_main");
    write(
        &root,
        "lib/a.ksl",
        r"
@Let[base, 40];
@Let[add, @Fn[[n : i64] : i64, AddI64[base, n]]];
@Let[other, 1];
@Export[add, other];
",
    );
    let path = write_path(
        &root,
        "main.ksl",
        r#"
@Use[{other}, "lib/a"];
@Main[[self : Pid[nil, nil]] : nil,
  @Block[@Let[f, @Fn[[x : i64] : i64, AddI64[x, 1]]], nil]]
"#,
    );

    let mut host = Host::new(root.path().to_path_buf());
    host.scheduler.set_gc_threshold(1);
    let outcome = host.run_file(&path).expect("脚本应可执行");
    assert!(outcome.result.is_ok());
    host.collect_cycles();
    assert_cached_closure_keeps_captures(&host);
}

#[test]
fn module_exports_survive_gc_during_code_input() {
    let root = temp_root("gc_code");
    write(
        &root,
        "lib/a.ksl",
        r"
@Let[base, 40];
@Let[add, @Fn[[n : i64] : i64, AddI64[base, n]]];
@Let[other, 1];
@Export[add, other];
",
    );
    let path = write_path(&root, "main.ksl", r#"@Use[{other}, "lib/a"];"#);

    let mut host = Host::new(root.path().to_path_buf());
    host.load_file(&path).expect("初始化文件应可加载");
    host.scheduler.set_gc_threshold(1);
    let outcome = host
        .run_code(
            "<命令行>",
            "@Block[@Let[f, @Fn[[x : i64] : i64, AddI64[x, 1]]], nil]",
        )
        .expect("代码段应可执行");
    assert!(outcome.result.is_ok());
    host.collect_cycles();
    assert_cached_closure_keeps_captures(&host);
}

fn assert_cached_closure_keeps_captures(host: &Host) {
    let add = ksl_base::symbol::intern("add");
    let closure = host
        .modules
        .values
        .values()
        .find_map(|module| module.get(&add))
        .and_then(|value| match value {
            Value::Closure(closure) => Some(closure),
            _ => None,
        })
        .expect("缓存应保留导出闭包");
    assert!(!closure.captures().is_empty(), "模块导出闭包的捕获不应被回收");
}

#[test]
fn initialization_file_exports_without_running_main() {
    let root = temp_root("init");
    write(
        &root,
        "lib/a.ksl",
        r"
@Data[A, {C[i64]}];
@Let[make, @Fn[[n : i64] : A, A/C[n]]];
@Export[make, ^A];
",
    );
    let path = write_path(
        &root,
        "main.ksl",
        r#"
@Use[{make, ^A}, "lib/a"];
@Let[hidden, 1];
@Let[visible, make[2]];
@Export[visible, ^A];
@Main[[self : Pid[nil, nil]] : nil, @Exit[1u]];
"#,
    );
    let mut host = Host::new(root.path().to_path_buf());
    let outcome = host.load_file(&path).expect("初始化文件应可加载");
    assert!(outcome.result.is_ok(), "初始化文件应执行成功");

    let (value, ty) = eval(&mut host, "visible").result.expect("导出值应可读取");
    assert!(matches!(value, Value::Data(_)));
    let ty = ty.expect("导出值应有类型");
    assert!(!is_printable(&ty));
    assert_eq!(display_type(&ty, host.session().type_names()), "A");
    assert_i64(&eval_value(&mut host, "@Match[visible, {(A/C[n], n)}]"), 2);

    let failure = error(&mut host, "hidden");
    assert_eq!(failure.kind, ErrorKind::UnknownName);
}

#[test]
fn session_keeps_displayable_values() {
    let root = temp_root("display");
    let mut host = Host::new(root.path().to_path_buf());

    assert_eq!(display_value(&eval_value(&mut host, "\"hello\"")), "\"hello\"");
    assert_eq!(display_value(&eval_value(&mut host, "'a'")), "'a'");
    let function = eval_value(&mut host, "@Let[f, @Fn[[x : i64] : i64, x]]; f");
    assert_eq!(display_value(&function), "Fn[[i64] : i64]");
}

#[test]
fn failed_input_rolls_back_bindings() {
    let root = temp_root("rollback");
    let mut host = Host::new(root.path().to_path_buf());

    let _ = eval_value(&mut host, "@Let[a, 1];");
    let outcome = eval(&mut host, "@Let[a, 5]; @Let[b, DivideI64[1, 0]];");
    assert!(outcome.result.is_err(), "运行时错误应使本段输入失败");
    assert_i64(&eval_value(&mut host, "a"), 1);

    let bindings: Vec<String> = host
        .session()
        .bindings()
        .into_iter()
        .map(|(name, _)| resolve(name).to_string())
        .collect();
    assert!(bindings.iter().any(|name| name == "a"));
    assert!(!bindings.iter().any(|name| name == "b"));
}

#[test]
fn module_import_overrides_local_binding() {
    let root = temp_root("import");
    let mut host = Host::new(root.path().to_path_buf());
    write(&root, "m.ksl", "@Let[f, 42]; @Export[f];");
    assert_i64(&eval_value(&mut host, "@Let[f, 1]; @Use[{f}, \"m\"]; f"), 42);
}

#[test]
fn empty_use_does_not_load_a_module() {
    let root = temp_root("empty_use");
    let mut host = Host::new(root.path().to_path_buf());
    assert_i64(&eval_value(&mut host, "@Use[{}, \"missing\"]; 7"), 7);
}

#[test]
fn reset_clears_bindings() {
    let root = temp_root("reset");
    let mut host = Host::new(root.path().to_path_buf());
    let _ = eval_value(&mut host, "@Let[a, 1];");
    host.reset();
    assert_eq!(host.session().bindings().len(), 0);
}

#[test]
fn globals_survive_gc_during_failed_file_run() {
    let root = temp_root("gc_file_rollback");
    let path = write_path(
        &root,
        "fail.ksl",
        r"
@Let[base, 1];
@Let[unused, @Fn[[n : i64] : i64, AddI64[base, n]]];
@Main[[self : Pid[nil, nil]] : nil, @Block[@Let[_, DivideI64[1, 0]], nil]];
",
    );
    let mut host = Host::new(root.path().to_path_buf());
    host.scheduler.set_gc_threshold(1);
    eval_value(
        &mut host,
        "@Let[base, 40]; @Let[f, @Fn[[n : i64] : i64, AddI64[base, n]]]; nil",
    );
    let outcome = host.run_file(&path).expect("文件应可加载");
    assert!(outcome.result.is_err(), "除零应使文件执行失败");
    assert_i64(&eval_value(&mut host, "f[2]"), 42);
}

#[test]
fn module_cache_is_cleared_when_root_changes() {
    let first = temp_root("root_first");
    write(&first, "lib/a.ksl", "@Let[x, 1]; @Export[x];");
    let first_main = write_path(
        &first,
        "main.ksl",
        r#"
@Use[{x}, "lib/a"];
@Let[_, x];
@Main[[self : Pid[nil, nil]] : nil, nil];
"#,
    );
    let second = temp_root("root_second");
    write(&second, "lib/a.ksl", "@Let[x, 2]; @Export[x];");
    let second_main = write_path(
        &second,
        "main.ksl",
        r#"
@Use[{x}, "lib/a"];
@Main[[self : Pid[nil, nil]] : nil,
  @If[EqualI64[x, 1], @Exit[1u], nil]];
"#,
    );

    let mut host = Host::new(first.path().to_path_buf());
    assert!(matches!(
        host.run_file(&first_main).expect("首次运行应成功").result,
        Ok(Value::Nil)
    ));
    assert!(matches!(
        host.run_file(&second_main).expect("更换根目录后应成功").result,
        Ok(Value::Nil)
    ));
}
