//! 端到端运行示例脚本, 保证用户可直接执行的程序不被运行时改动破坏.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use common::TempDir;

mod common;

fn temp_root() -> TempDir {
    TempDir::new("ksl_cli_examples_")
}

fn example_ir(command: &str, markers: &[&str]) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("examples").join("03_actor_service.ksl");
    let output = Command::new(env!("CARGO_BIN_EXE_ksl"))
        .arg(command)
        .arg(&path)
        .output()
        .expect("命令应可启动");
    assert!(
        output.status.success(),
        "{command} 失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    for marker in markers {
        assert!(stdout.contains(marker), "缺少 {marker}: {stdout}");
    }
}

fn run_example(root: &Path, example: &str, args: &[&str], current_dir: &Path) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_ksl"))
        .current_dir(current_dir)
        .arg("run")
        .arg(root.join("examples").join(example))
        .args(args)
        .output()
        .expect("示例应可启动");
    assert!(
        output.status.success(),
        "示例 {example} 失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn run_example_with_input(
    root: &Path,
    example: &str,
    input: &str,
    current_dir: &Path,
) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ksl"))
        .current_dir(current_dir)
        .arg("run")
        .arg(root.join("examples").join(example))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("示例应可启动");
    child
        .stdin
        .take()
        .expect("标准输入应可写")
        .write_all(input.as_bytes())
        .expect("应可写入标准输入");
    let output = child.wait_with_output().expect("示例应结束");
    assert!(
        output.status.success(),
        "示例 {example} 失败: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn examples_print_expected_ir_shapes() {
    example_ir("cps", &["handler $handler0", "receive 0", "prim @Spawn"]);
    example_ir(
        "linear",
        &["handler $handler0", "b0", "receive 0", "prim @Spawn"],
    );
}

#[test]
fn examples_run_with_expected_output() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace = root.canonicalize().expect("仓库根目录应存在");

    let output = run_example(&root, "01_language_tour.ksl", &[], &workspace);
    assert!(output.contains("orders=3 total=1786.5 quantity=4"), "{output}");
    assert!(output.contains("keyboard=2 fact=120"), "{output}");

    let notes = root.join("examples/02_text_report/data/notes.txt");
    let output = run_example(
        &root,
        "02_text_report/main.ksl",
        &["--", notes.to_str().expect("路径应为 UTF-8")],
        &workspace,
    );
    assert!(output.contains("4 lines, 5 words, 32 chars"), "{output}");

    let output = run_example(&root, "03_actor_service.ksl", &[], &workspace);
    assert!(
        output.contains("registered=true counter=12 supervised=42"),
        "{output}"
    );

    let output = run_example(&root, "04_terminal.ksl", &[], &workspace);
    assert!(output.contains("ReadKey 需要终端"), "{output}");

    let temp = temp_root();
    let main = "05_task_manager/main.ksl";
    let output = run_example(&root, main, &["--", "add", "2", "write", "docs"], temp.path());
    assert!(output.contains("added 1"), "{output}");
    let output = run_example(&root, main, &["--", "add", "1", "review", "code"], temp.path());
    assert!(output.contains("added 2"), "{output}");
    let output = run_example(&root, main, &["--", "list"], temp.path());
    assert!(output.contains("1. [pending] write docs"), "{output}");
    assert!(output.contains("2. [pending] review code"), "{output}");
    let output = run_example(&root, main, &["--", "done", "1"], temp.path());
    assert!(output.contains("completed 1"), "{output}");
    let output = run_example(&root, main, &["--", "remove", "2"], temp.path());
    assert!(output.contains("removed 2"), "{output}");
    let output = run_example(&root, main, &["--", "list"], temp.path());
    assert!(output.contains("1. [done] write docs"), "{output}");
    assert!(!output.contains("review code"), "{output}");
    let output = run_example(&root, main, &["--", "stats"], temp.path());
    assert!(output.contains("total=1 pending=0 done=1"), "{output}");

    let temp = temp_root();
    let output = run_example_with_input(
        &root,
        "05_task_manager/service.ksl",
        "add 2 write docs\nlist\nstats\nquit\n",
        temp.path(),
    );
    assert!(output.contains("added 1"), "{output}");
    assert!(output.contains("1. [pending] write docs"), "{output}");
    assert!(output.contains("total=1 pending=1 done=0"), "{output}");
    assert!(output.contains("bye"), "{output}");
}
