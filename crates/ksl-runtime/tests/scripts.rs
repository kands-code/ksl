//! 脚本形式的功能测试.
//!
//! 每个 `.ksl` 文件是一个执行用例, 必须正常结束.
//! 期望值由用例内的 `@Assert` 断言给出; 断言失败即用例失败.

use std::fs;
use std::path::{Path, PathBuf};

use ksl_base::source::Sources;
use ksl_frontend::check::Session;
use ksl_frontend::diagnostic::{render_error, render_runtime_error};
use ksl_frontend::parser::parse_text;
use ksl_runtime::module::{normalize_main, take_main};
use ksl_runtime::runtime::scheduler::Scheduler;

#[test]
fn script_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scripts");
    let mut paths = Vec::new();
    collect_scripts(&root, &mut paths);
    paths.sort();
    assert!(!paths.is_empty(), "脚本测试目录为空: {}", root.display());

    let mut failures = Vec::new();
    for path in paths {
        if let Err(message) = run_case(&path) {
            failures.push(message);
        }
    }
    assert!(
        failures.is_empty(),
        "{} 个脚本用例失败:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

fn collect_scripts(root: &Path, paths: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_scripts(&path, paths);
        } else if path.extension().is_some_and(|ext| ext == "ksl") {
            paths.push(path);
        }
    }
}

fn run_case(path: &Path) -> Result<(), String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("{}: 读取失败: {err}", path.display()))?;
    let name = path.display().to_string();
    let mut sources = Sources::default();
    let source = sources.add(name, text);
    let mut program = parse_text(sources.text(source), source).map_err(|err| {
        format!(
            "{}: 解析失败:\n{}",
            path.display(),
            render_error(&sources, &err)
        )
    })?;

    let module = path.file_stem().map_or_else(
        || "root".to_string(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    normalize_main(&mut program, &module).map_err(|err| {
        format!(
            "{}: 模块处理失败:\n{}",
            path.display(),
            render_error(&sources, &err)
        )
    })?;
    let main = take_main(&mut program);
    let mut session = Session::new();
    let handler = session
        .check_with_entry(&program, main.as_ref())
        .map_err(|err| {
            format!(
                "{}: 类型检查失败:\n{}",
                path.display(),
                render_error(&sources, &err)
            )
        })?;
    let mailboxes = handler
        .as_ref()
        .and_then(|handler| handler.pid.as_pid())
        .map(|(mailboxes, _)| mailboxes.len());

    let mut scheduler = Scheduler::new();
    let result =
        scheduler.run_program_with_main(&program, session.types(), main.as_ref(), mailboxes);
    scheduler.shutdown();
    result.map(|_| ()).map_err(|err| {
        format!(
            "{}: 执行失败:\n{}",
            path.display(),
            render_runtime_error(&sources, &err)
        )
    })
}
