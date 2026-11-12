//! 宿主会话测试.
//!
//! 交互输入与初始化文件的会话语义; 终端交互由命令行测试覆盖.

use ksl_base::testutil::{TempDir, write, write_path};
use ksl_diagnostics::error::{ErrorKind, KslError};
use ksl_frontend::check::{display_type, is_printable};
use ksl_value::value::Value;

use super::*;

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
@Main[[self : Pid[nil, nil]] : nil, @Exit[1u8]];
"#,
    );
    let mut host = Host::new(root.path().to_path_buf());
    let outcome = host.load_file(&path).expect("初始化文件应可加载");
    assert!(outcome.result.is_ok(), "初始化文件应执行成功");

    let (value, ty) = eval(&mut host, "visible").result.expect("导出值应可读取");
    assert!(matches!(value, Value::Data(_)));
    let ty = ty.expect("导出值应有类型");
    assert!(!is_printable(&ty));
    assert_eq!(display_type(&ty, host.session().type_bindings()), "A");
    assert_i64(&eval_value(&mut host, "@Match[visible, {(A/C[n], n)}]"), 2);

    let failure = error(&mut host, "hidden");
    assert_eq!(failure.kind(), Some(&ErrorKind::UnknownName));
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
        .map(|(name, _)| name.as_str().to_string())
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
fn repl_type_alias_resolves_in_annotations_and_patterns() {
    let root = temp_root("type_alias");
    let mut host = Host::new(root.path().to_path_buf());
    write(&root, "lib/a.ksl", "@Data[A, {C[i64]}]; @Export[^A];");
    let value = eval_value(
        &mut host,
        r#"@Use[{^(A, NA)}, "lib/a"]; @Let[x : NA, NA/C[1]]; @Match[x, {(NA/C[n], n)}]"#,
    );
    assert_i64(&value, 1);
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
  @If[EqualI64[x, 1], @Exit[1u8], nil]];
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

#[test]
fn repl_session_keeps_loaded_modules_after_file_changes() {
    let root = temp_root("stale_dep");
    write(&root, "lib/a.ksl", r"@Let[value, 1]; @Export[value];");
    let init = write_path(&root, "main.ksl", r#"@Use[{value}, "lib/a"]; @Export[value];"#);
    let mut host = Host::new(root.path().to_path_buf());
    host.load_file(&init).expect("初始化文件应可加载");
    assert_i64(&eval_value(&mut host, "value"), 1);

    write(&root, "lib/a.ksl", r"@Let[value, 2]; @Export[value];");
    let Err(error) = host.eval_input("<交互>", r#"@Use[{value}, "lib/a"]; value"#) else {
        panic!("交互会话应保持模块快照");
    };
    assert!(matches!(
        error.kind(),
        Some(&ErrorKind::ModuleAlreadyLoaded { .. })
    ));
    assert_i64(&eval_value(&mut host, "value"), 1);
}
