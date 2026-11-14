//! ksl 命令行入口.
//!
//! 子命令组织见 `docs/design.md` 第 11 节: lex, ast, check, cps, linear, run, repl, lsp.
//! 命令必须显式给出; `ksl` 无参数显示帮助, `ksl repl [初始化文件]` 进入交互.

#[macro_use]
mod output;

mod cli;
mod lsp;
mod repl;

use std::path::Path;
use std::process::ExitCode;

use ksl_core::diagnostic::{render_actor_error, render_error, render_runtime_error};
use ksl_core::error::{KslError, RuntimeError};
use ksl_core::host::{self, Host};
use ksl_core::source::Sources;
use ksl_core::{lexer, module, parser};

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> ExitCode {
    let action = match cli::parse(std::env::args_os().skip(1)) {
        Ok(action) => action,
        Err(err) => {
            err!("{}", err.render());
            return ExitCode::from(2);
        },
    };
    match action {
        cli::Action::Lex(file) => cmd_lex(&file),
        cli::Action::Ast(file) => cmd_ast(&file),
        cli::Action::Check(file) => cmd_check(&file),
        cli::Action::Cps(file) => cmd_cps(&file),
        cli::Action::Linear(file) => cmd_linear(&file),
        cli::Action::Lsp => match lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                errln!("LSP 错误: {err}");
                ExitCode::FAILURE
            },
        },
        cli::Action::Run {
            source,
            args,
            io_workers,
        } => {
            apply_io_workers(io_workers);
            cmd_run(&source, args)
        },
        cli::Action::Repl {
            file,
            code,
            io_workers,
        } => {
            apply_io_workers(io_workers);
            repl::run(file.as_deref(), code.as_deref())
        },
        cli::Action::Help(topic) => {
            out!("{}", cli::help(topic));
            ExitCode::SUCCESS
        },
        cli::Action::Version => {
            outln!("ksl {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        },
    }
}

/// 读取源文件; 成功返回源码文本.
fn read_source(file: &Path) -> Option<String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Some(text),
        Err(err) => {
            errln!("无法读取文件 {}: {err}", file.display());
            None
        },
    }
}

/// 渲染源码诊断并返回失败退出码.
fn report_error(sources: &Sources, err: &KslError) -> ExitCode {
    errln!("{}", render_error(sources, err));
    ExitCode::FAILURE
}

/// 加载并检查主文件; 失败时已渲染诊断.
fn load_graph(file: &Path, sources: &mut Sources) -> Option<module::LinkedGraph> {
    match module::load_and_check_file(file, sources) {
        Ok(graph) => Some(graph),
        Err(err) => {
            errln!("{}", render_error(sources, &err));
            None
        },
    }
}

/// 取主模块并降级为 CPS IR; 主模块缺失时报告内部错误.
fn lower_root(graph: &module::LinkedGraph) -> Option<ksl_core::cps::ir::Module> {
    let Some(root) = graph.get(&graph.root) else {
        errln!("内部错误: 模块图缺少主模块");
        return None;
    };
    Some(ksl_core::cps::ir::lower_program(&root.program, &root.types))
}

/// 把词法错误渲染为纯文本诊断, 供终端输出.
fn cmd_lex(file: &Path) -> ExitCode {
    let Some(text) = read_source(file) else {
        return ExitCode::FAILURE;
    };
    let mut sources = Sources::default();
    let id = sources.add(file.display().to_string(), &text);
    match lexer::lex(sources.text(id), id) {
        Ok(tokens) => {
            for token in tokens {
                outln!("{token:?}");
            }
            ExitCode::SUCCESS
        },
        Err(err) => report_error(&sources, &err),
    }
}

/// 把语法错误渲染为纯文本诊断, 供终端输出.
fn cmd_ast(file: &Path) -> ExitCode {
    let Some(text) = read_source(file) else {
        return ExitCode::FAILURE;
    };
    let mut sources = Sources::default();
    let id = sources.add(file.display().to_string(), &text);
    match parser::parse_text(sources.text(id), id) {
        Ok(program) => {
            outln!("{program:#?}");
            ExitCode::SUCCESS
        },
        Err(err) => report_error(&sources, &err),
    }
}

/// 把类型检查错误渲染为纯文本诊断, 供终端输出.
fn cmd_check(file: &Path) -> ExitCode {
    let mut sources = Sources::default();
    match module::load_and_check_file(file, &mut sources) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => report_error(&sources, &err),
    }
}

/// 把检查后的程序降级为 CPS IR 并输出, 供调试.
fn cmd_cps(file: &Path) -> ExitCode {
    let mut sources = Sources::default();
    let Some(graph) = load_graph(file, &mut sources) else {
        return ExitCode::FAILURE;
    };
    let Some(ir) = lower_root(&graph) else {
        return ExitCode::FAILURE;
    };
    out!("{ir}");
    ExitCode::SUCCESS
}

/// 把检查后的程序降级为线性 CPS IR 并输出, 供调试.
fn cmd_linear(file: &Path) -> ExitCode {
    let mut sources = Sources::default();
    let Some(graph) = load_graph(file, &mut sources) else {
        return ExitCode::FAILURE;
    };
    let Some(ir) = lower_root(&graph) else {
        return ExitCode::FAILURE;
    };
    let linear = ksl_core::cps::linear::linearize(&ir);
    out!("{linear}");
    ExitCode::SUCCESS
}

/// 执行脚本: 词法, 语法, 类型检查, 降级为线性 IR 后执行.
///
/// 运行时错误渲染诊断到标准错误, 退出码 1; `@Exit[code]` 以指定码退出.
fn cmd_run(source: &cli::Source, args: Vec<String>) -> ExitCode {
    match source {
        cli::Source::File(file) => run_file(file, args),
        cli::Source::Code(code) => run_code(code, args),
    }
}

/// 执行文件形式的脚本.
fn run_file(file: &Path, args: Vec<String>) -> ExitCode {
    let mut host = Host::in_current_dir();
    host.set_args(args);
    match host.run_file(file) {
        Ok(outcome) => finish_host(&mut host, outcome),
        Err(err) => report_error(host.sources(), &err),
    }
}

/// 执行命令行代码段.
///
/// 代码段不是模块, 不能使用 Actor 原语.
fn run_code(code: &str, args: Vec<String>) -> ExitCode {
    let mut host = Host::in_current_dir();
    host.set_args(args);
    match host.run_code("<命令行>", code) {
        Ok(outcome) => finish_host(&mut host, outcome),
        Err(err) => report_error(host.sources(), &err),
    }
}

/// 应用命令行给定的 I/O 工作线程数; 必须在首次 I/O 提交之前调用.
fn apply_io_workers(io_workers: Option<usize>) {
    if let Some(count) = io_workers {
        ksl_core::io::set_io_workers(count);
    }
}

/// 统一处理运行时错误, Actor 错误与宿主关闭.
fn finish_host(host: &mut Host, outcome: host::RunOutcome) -> ExitCode {
    host.shutdown();
    for err in &outcome.actor_errors {
        errln!("{}", render_actor_error(host.sources(), err));
    }
    match outcome.result {
        Ok(_) | Err(RuntimeError::BrokenPipe) => ExitCode::SUCCESS,
        Err(RuntimeError::Exit(code)) => ExitCode::from(code),
        Err(err) => {
            errln!("{}", render_runtime_error(host.sources(), &err));
            ExitCode::FAILURE
        },
    }
}
