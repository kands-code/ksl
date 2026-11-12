//! 脚本形式的功能测试.
//!
//! 每个 `.ksl` 文件以行注释给出期望; 注释同时会被词法器忽略, 所以用例
//! 可以直接用 `ksl run` 运行. 支持的单行指令:
//!
//! - `# @value <显示文本>`: 执行并比较尾值的默认显示;
//! - `# @error <错误码>`: 解析, 类型检查或执行应报指定错误;
//! - `# @exit <退出码>`: 执行应以 `@Exit` 终止;
//! - `# @check`: 只做类型检查并要求通过;
//! - `# @run`: 执行并要求成功, 不比较结果.
//!
//! 未写指令时默认按 `@run`. 每个用例只允许一个测试指令.

use std::fs;
use std::path::{Path, PathBuf};

use ksl_core::check::Session;
use ksl_core::cps::scheduler::Scheduler;
use ksl_core::diagnostic::{render_error, render_runtime_error};
use ksl_core::error::{KslError, RuntimeError};
use ksl_core::fmtstr::display_value;
use ksl_core::parser::parse_text;
use ksl_core::source::Sources;

#[derive(Debug, Clone)]
enum Mode {
    Check,
    Run,
}

#[derive(Debug, Clone)]
enum Expected {
    Success,
    Value(String),
    Error(String),
    Exit(u8),
}

#[derive(Debug, Clone)]
struct Case {
    mode: Mode,
    expected: Expected,
}

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
    let case = parse_case(&text).map_err(|message| format!("{}: {message}", path.display()))?;

    let name = path.display().to_string();
    let mut sources = Sources::default();
    let source = sources.add(name, text);
    let program = match parse_text(sources.text(source), source) {
        Ok(program) => program,
        Err(err) => return expect_check_error(&sources, &case, &err),
    };
    let mut session = Session::new();
    if let Err(err) = session.check(&program) {
        return expect_check_error(&sources, &case, &err);
    }
    if matches!(case.mode, Mode::Check) {
        return match case.expected {
            Expected::Success => Ok(()),
            Expected::Error(code) => {
                Err(format!("{}: 期望错误 {code}, 但类型检查通过", path.display()))
            },
            Expected::Value(_) | Expected::Exit(_) => {
                Err(format!("{}: @check 不能与 @value / @exit 组合", path.display()))
            },
        };
    }

    let mut scheduler = Scheduler::new();
    let result = scheduler.run_program(&program, session.types());
    scheduler.shutdown();
    match case.expected {
        Expected::Success => match result {
            Ok(_) => Ok(()),
            Err(err) => Err(format!(
                "{}: 期望执行成功, 实际失败:\n{}",
                path.display(),
                render_runtime_error(&sources, &err)
            )),
        },
        Expected::Value(expected) => match result {
            Ok(value) => {
                let actual = display_value(&value);
                if actual == expected {
                    Ok(())
                } else {
                    Err(format!(
                        "{}: 尾值不符\n期望: {expected}\n实际: {actual}",
                        path.display()
                    ))
                }
            },
            Err(err) => Err(format!(
                "{}: 期望尾值 {expected}, 实际失败:\n{}",
                path.display(),
                render_runtime_error(&sources, &err)
            )),
        },
        Expected::Exit(code) => match result {
            Err(RuntimeError::Exit(actual)) if actual == code => Ok(()),
            Err(RuntimeError::Exit(actual)) => Err(format!(
                "{}: 退出码不符, 期望 {code}, 实际 {actual}",
                path.display()
            )),
            Ok(_) => Err(format!("{}: 期望 @Exit[{code}u], 实际正常结束", path.display())),
            Err(err) => Err(format!(
                "{}: 期望 @Exit[{code}u], 实际失败:\n{}",
                path.display(),
                render_runtime_error(&sources, &err)
            )),
        },
        Expected::Error(code) => match result {
            Err(RuntimeError::Runtime(err)) if err.code() == code => Ok(()),
            Err(RuntimeError::Runtime(err)) => Err(format!(
                "{}: 运行时错误码不符, 期望 {code}\n{}",
                path.display(),
                render_error(&sources, &err)
            )),
            Err(RuntimeError::Internal(message)) => Err(format!(
                "{}: 期望错误 {code}, 实际内部错误: {message}",
                path.display()
            )),
            Err(RuntimeError::Exit(actual)) => Err(format!(
                "{}: 期望错误 {code}, 实际以退出码 {actual} 结束",
                path.display()
            )),
            Err(RuntimeError::BrokenPipe) => {
                Err(format!("{}: 期望错误 {code}, 实际输出流已关闭", path.display()))
            },
            Ok(_) => Err(format!("{}: 期望错误 {code}, 实际执行成功", path.display())),
        },
    }
}

fn expect_check_error(sources: &Sources, case: &Case, err: &KslError) -> Result<(), String> {
    match &case.expected {
        Expected::Error(code) if err.code() == *code => Ok(()),
        Expected::Error(code) => Err(format!(
            "错误码不符, 期望 {code}\n{}",
            render_error(sources, err)
        )),
        Expected::Success => Err(format!(
            "期望类型检查通过, 实际失败:\n{}",
            render_error(sources, err)
        )),
        Expected::Value(_) | Expected::Exit(_) => Err(format!(
            "期望执行结果, 实际类型检查失败:\n{}",
            render_error(sources, err)
        )),
    }
}

fn parse_case(text: &str) -> Result<Case, String> {
    let mut case = Case {
        mode: Mode::Run,
        expected: Expected::Success,
    };
    let mut seen = false;
    for line in text.lines() {
        let line = line.trim();
        let Some(directive) = line.strip_prefix('#') else {
            continue;
        };
        let directive = directive.trim();
        let Some(rest) = directive.strip_prefix('@') else {
            continue;
        };
        if seen {
            return Err("只允许一个测试指令".to_string());
        }
        seen = true;
        let (name, argument) = rest
            .split_once(char::is_whitespace)
            .map_or((rest, ""), |(name, argument)| (name, argument.trim()));
        case = match name {
            "run" => Case {
                mode: Mode::Run,
                expected: Expected::Success,
            },
            "check" => Case {
                mode: Mode::Check,
                expected: Expected::Success,
            },
            "value" => {
                if argument.is_empty() {
                    return Err("@value 缺少期望文本".to_string());
                }
                Case {
                    mode: Mode::Run,
                    expected: Expected::Value(argument.to_string()),
                }
            },
            "error" => {
                if argument.is_empty() {
                    return Err("@error 缺少错误码".to_string());
                }
                Case {
                    mode: Mode::Run,
                    expected: Expected::Error(argument.to_string()),
                }
            },
            "exit" => {
                let code = argument
                    .parse::<u8>()
                    .map_err(|_| format!("@exit 退出码非法: {argument}"))?;
                Case {
                    mode: Mode::Run,
                    expected: Expected::Exit(code),
                }
            },
            _ => return Err(format!("未知测试指令: @{name}")),
        };
    }
    Ok(case)
}
