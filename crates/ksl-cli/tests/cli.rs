//! 命令行参数端到端测试.

use std::fmt::Write as _;

use common::{command, run, stderr, stdout};
use ksl_base::testutil::TempDir;

mod common;

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
    #[cfg(feature = "lsp")]
    assert!(text.contains("lsp"), "帮助应列出语言服务器: {text}");
    #[cfg(not(feature = "lsp"))]
    assert!(
        !text.contains("lsp"),
        "关闭 lsp 功能后不应列出语言服务器: {text}"
    );

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
    let output = run(&["help", "lsp"]);
    assert!(stdout(&output).contains("启动语言服务器"));
}

#[cfg(not(feature = "lsp"))]
#[test]
fn disabled_lsp_reports_build_capability() {
    let output = run(&["lsp"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "未启用的 lsp 应以退出码 2 结束: {}",
        stderr(&output)
    );
    let text = stderr(&output);
    assert!(text.contains("未启用 lsp"), "应说明当前构建未启用: {text}");
}

#[test]
fn usage_errors_report_chinese_messages() {
    assert_usage_error(&["frobnicate"], "未知子命令");
    assert_usage_error(&["--bogus"], "未知选项");
    assert_usage_error(&["run"], "缺少必需的参数");
    assert_usage_error(&["run", "a.ksl", "--code", "x"], "不能同时使用");
    assert_usage_error(&["run", "a.ksl", "b.ksl"], "多余的参数");
    assert_usage_error(&["run", "--code"], "缺少值");
}

#[test]
fn exit_sets_process_exit_code() {
    let output = run(&["run", "--code", "@Exit[3u8]"]);
    assert_eq!(
        output.status.code(),
        Some(3),
        "退出码应传达到进程: {}",
        stderr(&output)
    );

    let output = run(&["run", "--code", "@If[true, @Exit[4u8], nil]"]);
    assert_eq!(
        output.status.code(),
        Some(4),
        "程序根内的嵌套 @Exit 应传达到进程: {}",
        stderr(&output)
    );
}

#[test]
fn module_forms_are_rejected_for_code_input() {
    let output = run(&["run", "--code", "@Main[[self : Pid[nil, nil]] : nil, nil]"]);
    assert!(!output.status.success(), "命令行代码不应接受 @Main");
    assert!(
        stderr(&output).contains("@Main 只允许出现在模块中"),
        "错误信息应说明 @Main 只允许出现在模块中: {}",
        stderr(&output)
    );
}

#[test]
fn run_accepts_all_source_and_argument_forms() {
    let output = run(&[
        "run",
        "--code",
        r#"@If[EqualU64[@ListLength[Args[]], 2u64], Print["ok\n"], Print["bad\n"]]"#,
        "--",
        "a",
        "b",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");

    let cases: &[&[&str]] = &[
        &["run", "-c", r#"Print["ok\n"]"#],
        &["run", r#"-cPrint["ok\n"]"#],
        &["run", "--code", r#"Print["ok\n"]"#],
        &["run", r#"--code=Print["ok\n"]"#],
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

    let mut child = command()
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
        r#"@Block[Print["x"], loop[SubI64[n, 1]]]]]; loop[100000]"#
    );
    let (success, error) = run_with_closed_stdout(&["run", "--code", code]);
    assert!(success, "Print 应在管道关闭后正常结束: {error}");
    assert!(error.is_empty(), "Print 不应输出错误: {error}");
}
