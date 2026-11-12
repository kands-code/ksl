//! 命令行参数端到端测试.

use std::fmt::Write as _;
use std::process::{Command, Output};

use common::TempDir;

mod common;

fn ksl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ksl"))
}

fn run(args: &[&str]) -> Output {
    ksl().args(args).output().expect("命令行应可启动")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_usage_error(args: &[&str], message: &str) {
    let output = run(args);
    assert_eq!(
        output.status.code(),
        Some(2),
        "用法错误应返回退出码 2: {}",
        stderr(&output)
    );
    let text = stderr(&output);
    assert!(text.contains(message), "错误信息应包含 {message}: {text}");
    assert!(text.contains("用法:"), "错误信息应给出用法: {text}");
}

#[test]
fn help_and_version_cover_all_commands() {
    let output = run(&["--help"]);
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("ksl 编程语言"), "帮助应使用中文: {text}");
    assert!(text.contains("linear"), "帮助应列出子命令: {text}");
    assert!(text.contains("lsp"), "帮助应列出语言服务器: {text}");

    let output = run(&["--version"]);
    assert!(output.status.success());
    assert!(stdout(&output).contains(env!("CARGO_PKG_VERSION")));

    for args in [["run", "--help"], ["lex", "--help"], ["lsp", "--help"]] {
        let output = run(&args);
        assert!(output.status.success());
    }
    let output = run(&["run", "--help"]);
    assert!(stdout(&output).contains("执行脚本或代码"));
    let output = run(&["help", "lex"]);
    assert!(stdout(&output).contains("输出词法记号序列"));
}

#[test]
fn usage_errors_report_chinese_messages() {
    assert_usage_error(&["frobnicate"], "未知子命令");
    assert_usage_error(&["--bogus"], "未知选项");
    assert_usage_error(&["run"], "缺少必需的参数");
    assert_usage_error(&["run", "a.ksl", "--code", "x"], "不能同时使用");
    assert_usage_error(&["run", "a.ksl", "b.ksl"], "多余的参数");
    assert_usage_error(&["run", "--code"], "缺少值");
    assert_usage_error(&["lex"], "缺少必需的参数");
    assert_usage_error(&["check", "a.ksl", "b.ksl"], "多余的参数");
}

#[test]
fn run_accepts_all_source_and_argument_forms() {
    let output = run(&[
        "run",
        "--code",
        r#"@If[EqualU64[@ListLength[Args[]], 2U], Print["ok"], Print["bad"]]"#,
        "--",
        "a",
        "b",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");

    let cases: &[&[&str]] = &[
        &["run", "-c", r#"Print["ok"]"#],
        &["run", r#"-cPrint["ok"]"#],
        &["run", "--code", r#"Print["ok"]"#],
        &["run", r#"--code=Print["ok"]"#],
    ];
    for args in cases {
        let output = run(args);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(stdout(&output), "ok\n");
    }
}

#[cfg(unix)]
fn run_with_closed_stdout(args: &[&str]) -> (bool, String) {
    use std::io::{BufRead, BufReader, Read};
    use std::process::Stdio;

    let mut child = ksl()
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("命令行应可启动");
    let stdout = child.stdout.take().expect("标准输出应可读");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader.read_line(&mut line).expect("应可读取首行");
    drop(reader);

    let mut error = String::new();
    child
        .stderr
        .take()
        .expect("标准错误应可读")
        .read_to_string(&mut error)
        .expect("应可读取标准错误");
    let status = child.wait().expect("命令行应结束");
    (status.success(), error)
}

#[cfg(unix)]
#[test]
fn output_survives_closed_pipe() {
    let root = TempDir::new("ksl_cli_closed_pipe_");
    let path = root.path().join("big.ksl");
    let mut source = String::new();
    for i in 0..1000 {
        writeln!(source, "@Def[fn{i}, [n : i64] : i64, AddI64[n, {i}]];")
            .expect("写入字符串应成功");
    }
    std::fs::write(&path, source).expect("源文件应可写入");
    let path = path.to_str().expect("路径应为 UTF-8");

    for command in ["cps", "linear"] {
        let (success, error) = run_with_closed_stdout(&[command, path]);
        assert!(success, "{command} 应在管道关闭后正常结束: {error}");
        assert!(error.is_empty(), "{command} 不应输出错误: {error}");
    }

    let code = concat!(
        r#"@Def[loop, [n : i64] : nil, "#,
        r#"@If[LessOrEqualI64[n, 0], nil, "#,
        r#"@Block[@Let[_p, Print["x"]], loop[SubI64[n, 1]]]]]; loop[100000]"#
    );
    let (success, error) = run_with_closed_stdout(&["run", "--code", code]);
    assert!(success, "Print 应在管道关闭后正常结束: {error}");
    assert!(error.is_empty(), "Print 不应输出错误: {error}");
}
