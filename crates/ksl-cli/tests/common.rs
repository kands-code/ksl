//! 集成测试公共代码: 命令行启动, 输出收集与临时目录.
//!
//! 各集成测试目标只使用部分辅助, 模块整体放宽未使用项检查.

#![allow(dead_code)]

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

#[must_use]
pub(crate) fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ksl"))
}

/// 运行命令行并收集输出, 标准输入立即关闭.
#[must_use]
pub(crate) fn run(args: &[&str]) -> Output {
    command().args(args).output().expect("命令行应可启动")
}

/// 运行命令行, 写入标准输入并收集输出.
#[must_use]
pub(crate) fn run_with_input(args: &[&str], input: &str) -> Output {
    spawn(args, None, input)
}

/// 在指定目录运行命令行, 写入标准输入并收集输出.
#[must_use]
pub(crate) fn run_in(dir: &Path, args: &[&str], input: &str) -> Output {
    spawn(args, Some(dir), input)
}

/// 启动命令行, 写入标准输入并等待结束.
fn spawn(args: &[&str], dir: Option<&Path>, input: &str) -> Output {
    let mut cmd = command();
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().expect("命令行应可启动");
    child
        .stdin
        .take()
        .expect("标准输入应可写")
        .write_all(input.as_bytes())
        .expect("应可写入标准输入");
    child.wait_with_output().expect("命令行应结束")
}

/// 以 UTF-8 宽松解码标准输出.
#[must_use]
pub(crate) fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// 以 UTF-8 宽松解码标准错误.
#[must_use]
pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// 断言命令行成功退出, 且标准输出包含给定片段.
pub(crate) fn assert_stdout_contains(output: &Output, expected: &str) {
    assert!(output.status.success(), "命令行应成功退出: {}", stderr(output));
    let text = stdout(output);
    assert!(text.contains(expected), "标准输出应包含 {expected:?}: {text}");
}
