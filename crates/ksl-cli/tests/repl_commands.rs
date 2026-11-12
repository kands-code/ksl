//! REPL 命令端到端测试.

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

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

fn run_repl_with_timeout(args: &[&str], input: &str, timeout: Duration) -> (String, String) {
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
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().expect("应可查询进程状态") {
            Some(_) => break,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let output = child.wait_with_output().expect("命令行应结束");
                panic!(
                    "REPL 未在超时内结束: {}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            },
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }
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

#[test]
fn repl_waits_for_actor_input_before_prompt() {
    let code = r#"
        @Let[reader, @Handler[
            [self : Pid[nil, nil]] : nil,
            @Block[
                @Match[ReadLine["actor> "], {
                    (/Ok[line], @Match[line, {
                        (/Some[text], Print[f"actor: {text}"]),
                        (/None[], Print["actor eof"])
                    }]),
                    (/Err[_], Print["actor error"])
                }]
            ]]];
        @Let[_p, @Spawn[1u, reader]];
        @Match[ReadLine["main> "], {
            (/Ok[line], @Match[line, {
                (/Some[text], Print[f"main: {text}"]),
                (/None[], Print["main eof"])
            }]),
            (/Err[_], Print["main error"])
        }]
    "#;
    let (stdout, stderr) = run_repl_with_args(&["repl", "--code", code], "first\nsecond\n7\n");
    assert!(
        stdout.contains("main: first"),
        "主任务应先读取: {stdout}{stderr}"
    );
    assert!(
        stdout.contains("actor: second"),
        "Actor 输入应在提示符前完成: {stdout}{stderr}"
    );
    assert!(
        stdout.contains("2= 7"),
        "Actor 输入后 REPL 应继续: {stdout}{stderr}"
    );
}

#[test]
fn repl_input_not_blocked_by_cpu_actor() {
    let code = r#"
        @Let[spin, @Handler[
            [self : Pid[nil, nil]] : nil,
            @Continue[]
        ]];
        @Let[reader, @Handler[
            [self : Pid[nil, nil]] : nil,
            @Block[
                @Match[ReadLine["actor> "], {
                    (/Ok[line], @Match[line, {
                        (/Some[text], Print[f"actor: {text}"]),
                        (/None[], Print["actor eof"])
                    }]),
                    (/Err[_], Print["actor error"])
                }]
            ]]];
        @Let[_s, @Spawn[1u, spin]];
        @Let[_p, @Spawn[1u, reader]];
        @Match[ReadLine["main> "], {
            (/Ok[line], @Match[line, {
                (/Some[text], Print[f"main: {text}"]),
                (/None[], Print["main eof"])
            }]),
            (/Err[_], Print["main error"])
        }]
    "#;
    let (stdout, stderr) = run_repl_with_timeout(
        &["repl", "--code", code],
        "first\nsecond\n7\n",
        Duration::from_secs(5),
    );
    assert!(
        stdout.contains("actor: second"),
        "CPU 密集 Actor 不应阻止输入完成: {stdout}{stderr}"
    );
    assert!(
        stdout.contains("2= 7"),
        "CPU 密集 Actor 不应阻止 REPL 恢复: {stdout}{stderr}"
    );
}
