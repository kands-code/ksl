//! REPL 入口测试.

use std::process::Stdio;

use common::command;

mod common;

#[test]
fn repl_requires_terminal() {
    let output = command()
        .arg("repl")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("命令行应可启动");
    assert_eq!(output.status.code(), Some(2), "非终端 REPL 应以退出码 2 结束");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("REPL 需要"), "应报告终端要求: {stderr}");

    let output = command()
        .args(["repl", "-c", "Print[\"不应执行\"]"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("命令行应可启动");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty(), "终端检查应先于命令行代码");
}
