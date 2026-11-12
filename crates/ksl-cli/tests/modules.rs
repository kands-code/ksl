//! 模块加载与执行的端到端测试.

use std::fs;
use std::io::Write as _;
use std::process::{Command, Stdio};

use common::TempDir;

mod common;

fn temp_root() -> TempDir {
    let root = TempDir::new("ksl_cli_module_");
    fs::create_dir_all(root.path().join("lib")).expect("模块目录应可创建");
    root
}

/// 在给定目录中运行命令行, 返回标准输出与标准错误.
fn run_in(root: &TempDir, args: &[&str], input: &str) -> (String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ksl"))
        .args(args)
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("命令行应可启动");
    child
        .stdin
        .take()
        .expect("标准输入应可写")
        .write_all(input.as_bytes())
        .expect("应可写入标准输入");
    let output = child.wait_with_output().expect("命令行应结束");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn run_executes_modules_and_imports() {
    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        "@Data[A, {C[i64]}];\n@Let[make, @Fn[[n : i64] : A, A/C[n]]];\n@Let[show, @Fn[[v : A] \
         : string, @Match[v, { (/C[n], f\"{n}\") }]]];\n@Export[make, show, ^A];\n",
    )
    .expect("依赖模块应可写入");
    fs::write(
        root.path().join("main.ksl"),
        "@Use[{make, show}, \"lib/a\"];\n@Let[v, make[42]];\n@Let[s, Print[show[v]]];\nnil\n",
    )
    .expect("主模块应可写入");
    let output = Command::new(env!("CARGO_BIN_EXE_ksl"))
        .arg("run")
        .arg(root.path().join("main.ksl"))
        .output()
        .expect("命令行应可启动");
    assert!(
        output.status.success(),
        "模块执行失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");

    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        "@Def[f, [n : i64] : i64, AddI64[n, 1]];\n@Export[f];\n",
    )
    .expect("依赖模块应可写入");
    fs::write(
        root.path().join("lib/b.ksl"),
        "@Use[{f}, \"a\"];\n@Let[g, @Fn[[n : i64] : i64, MulI64[f[n], f[n]]]];\n@Export[g];\n",
    )
    .expect("模块应可写入");
    fs::write(
        root.path().join("main.ksl"),
        "@Use[{g}, \"lib/b\"];\n@Let[x, g[20]];\nPrint[f\"{x}\"]\n",
    )
    .expect("主模块应可写入");
    let output = Command::new(env!("CARGO_BIN_EXE_ksl"))
        .arg("run")
        .arg(root.path().join("main.ksl"))
        .output()
        .expect("命令行应可启动");
    assert!(
        output.status.success(),
        "模块执行失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "441\n");

    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        "@Data[A, {C[i64]}];\n@Let[make, @Fn[[n : i64] : A, A/C[n]]];\n@Export[make, ^A];\n",
    )
    .expect("依赖模块应可写入");
    let (stdout, stderr) = run_in(
        &root,
        &[
            "run",
            "--code",
            "@Use[{make, ^A}, \"lib/a\"]; @Let[v, make[9]]; @Let[n, @Match[v, {(A/C[x], \
             x)}]]; @Let[s, Print[f\"{n}\"]];",
        ],
        "",
    );
    assert!(stdout.contains('9'), "命令行代码应可导入模块: {stdout}{stderr}");
}

#[test]
fn repl_exposes_module_visibility_and_types() {
    let root = temp_root();
    let main = root.path().join("main.ksl");
    fs::write(
        &main,
        "@Let[hidden, 1];\n@Let[visible, 2];\n@Export[visible];\n",
    )
    .expect("主文件应可写入");
    let (stdout, stderr) = run_in(
        &root,
        &["repl", main.to_str().expect("路径应为 UTF-8")],
        "visible\nhidden\n",
    );
    assert!(stdout.contains("1= 2"), "未导出的名字未正确隔离: {stdout}");
    assert!(stderr.contains("未定义的名字"), "未导出名字应不可见: {stderr}");

    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        "@Data[A, {C[i64]}];\n@Export[^A];\n",
    )
    .expect("依赖模块应可写入");
    fs::write(
        root.path().join("main.ksl"),
        "@Use[{^A}, \"lib/a\"];\n@Let[v, A/C[7]];\n@Export[v, ^A];\n",
    )
    .expect("主模块应可写入");
    let (stdout, stderr) = run_in(
        &root,
        &[
            "repl",
            root.path().join("main.ksl").to_str().expect("路径应为 UTF-8"),
        ],
        "A/C[3]\n@Match[v, {(A/C[n], n)}]\n",
    );
    assert!(
        stdout.contains("1= <A>"),
        "导出的类型不可打印时应显示名义类型: {stdout}{stderr}"
    );
    assert!(stdout.contains("2= 7"), "导出的类型应可匹配: {stdout}{stderr}");

    let root = temp_root();
    fs::write(
        root.path().join("lib/a.ksl"),
        "@Data[A, {C[i64]}];\n@Let[make, @Fn[[n : i64] : A, A/C[n]]];\n@Export[make, ^A];\n",
    )
    .expect("依赖模块应可写入");
    let (stdout, stderr) = run_in(
        &root,
        &["repl"],
        "@Use[{make, ^A}, \"lib/a\"];\n@Let[v, make[5]];\n@Match[v, {(A/C[n], n)}]\n",
    );
    assert!(
        stdout.contains("3= 5"),
        "行内导入的值应可使用: {stdout}{stderr}"
    );

    let root = temp_root();
    let (stdout, stderr) = run_in(&root, &["repl"], "@Export[x];\n");
    assert!(
        stderr.contains("导出声明只允许出现在模块中"),
        "交互输入不应允许导出: {stdout}{stderr}"
    );
}

#[test]
fn repl_session_semantics_are_preserved() {
    let root = temp_root();
    let (stdout, stderr) = run_in(
        &root,
        &["repl"],
        "\"hello\"\n'a'\n@Let[f, @Fn[[x : i64] : i64, x]]; f\n",
    );
    assert!(
        stdout.contains("1= \"hello\""),
        "字符串应带引号: {stdout}{stderr}"
    );
    assert!(stdout.contains("2= 'a'"), "字符应带引号: {stdout}{stderr}");
    assert!(
        stdout.contains("3= Fn[[i64] : i64]"),
        "匿名函数应显示签名: {stdout}{stderr}"
    );

    let (stdout, stderr) = run_in(
        &root,
        &["repl"],
        "@Let[a, 1];\n@Let[a, 5]; @Let[b, DivideI64[1, 0]];\na\n",
    );
    assert!(
        stdout.contains("3= 1"),
        "失败输入应回滚全局绑定: {stdout}{stderr}"
    );

    fs::write(root.path().join("m.ksl"), "@Let[f, 42];\n@Export[f];\n").expect("模块应可写入");
    let (stdout, stderr) = run_in(&root, &["repl"], "@Let[f, 1]; @Use[{f}, \"m\"]; f\n");
    assert!(
        stdout.contains("1= 42"),
        "导入应在所在位置覆盖本地值: {stdout}{stderr}"
    );

    let (stdout, stderr) = run_in(&root, &["repl"], "@Use[{}, \"missing\"]; 7\n");
    assert!(
        stdout.contains("1= 7"),
        "空 @Use 不应加载依赖: {stdout}{stderr}"
    );
}
