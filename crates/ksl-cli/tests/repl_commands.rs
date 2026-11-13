//! REPL 命令端到端测试.

use std::io::Write as _;
use std::process::{Command, Stdio};

fn run_repl(input: &str) -> (String, String) {
    run_repl_with_args(&["repl"], input)
}

fn run_repl_with_args(args: &[&str], input: &str) -> (String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ksl"))
        .args(args)
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
fn repl_commands_control_session_and_bindings() {
    let (stdout, stderr) = run_repl("@Let[x, 1];\n%type AddI64[x, 2]\n%bindings\n");
    assert!(
        stdout.contains("AddI64[x, 2] : i64"),
        "type 命令应显示表达式类型: {stdout}{stderr}"
    );
    assert!(
        stdout.contains("x : i64"),
        "bindings 命令应列出全局绑定: {stdout}{stderr}"
    );
    assert!(
        !stdout.contains("2= 3"),
        "type 命令不应执行表达式: {stdout}{stderr}"
    );

    let (stdout, stderr) = run_repl("@Let[a, 1];\n%reset\n%bindings\n");
    assert!(
        stdout.contains("会话已重置"),
        "reset 应明确反馈: {stdout}{stderr}"
    );
    assert!(
        stdout.contains("(无全局绑定)"),
        "reset 后不应保留绑定: {stdout}{stderr}"
    );

    let (stdout, stderr) = run_repl("%help\n%quit\n");
    assert!(
        stdout.contains("%type") && stdout.contains("%bindings"),
        "help 应列出命令: {stdout}{stderr}"
    );
    assert!(
        !stderr.contains("未知命令"),
        "help 与 quit 不应报未知命令: {stderr}"
    );
}

#[test]
fn repl_rejects_main() {
    let (_, stderr) = run_repl("@Main[[self : Pid[nil, nil]] : nil, nil]\n%quit\n");
    assert!(
        stderr.contains("@Main 只允许出现在模块中"),
        "REPL 应拒绝 @Main: {stderr}"
    );
}

#[test]
fn repl_pipe_output_has_no_prompt_or_separator() {
    let (stdout, stderr) = run_repl("@Let[x, 1];\nAddI64[x, 2]\n");
    assert!(
        !stdout.contains("1> ") && !stdout.contains("1| "),
        "管道模式不应输出提示符: {stdout}{stderr}"
    );
    assert!(
        !stdout.contains("\n\n"),
        "管道模式不应输出轮次分隔空行: {stdout}{stderr}"
    );
}
