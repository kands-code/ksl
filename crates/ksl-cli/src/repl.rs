//! 交互模式: 逐段读入并求值, 环境跨段持久.
//!
//! REPL 要求标准输入与标准输出都是终端.
//! 行编辑, 键位与提示符见 `crates/ksl-cli/src/repl/editor.rs` 的模块文档.
//! 命令以 `%` 开头, 只在交互模式生效.

use std::path::Path;
use std::process::ExitCode;

use ksl_base::source::Sources;
use ksl_diagnostics::diagnostic::{render_actor_error, render_error, render_runtime_error};
use ksl_diagnostics::error::{ErrorKind, RuntimeError};
use ksl_frontend::check::{Type, TypeBindings};
use ksl_frontend::lexer::{TokenKind, lex};
use ksl_host::Host;
use ksl_value::value::Value;

mod editor;
mod highlight;

use editor::{Editor, ReadOutcome, terminal_ready};

/// 进入交互模式.
///
/// 给定文件时先把它作为主模块加载, 执行依赖并绑定导出名字.
/// 加载出错则报告并进入空环境, 会话继续.
/// 给定代码时先执行该代码.
pub fn run(
    file: Option<&Path>,
    code: Option<&str>,
    io_workers: Option<usize>,
    cpu_workers: Option<usize>,
) -> ExitCode {
    if let Err(message) = terminal_ready() {
        errln!("{message}");
        return ExitCode::from(2);
    }
    let mut host = Host::in_current_dir_with_workers(io_workers, cpu_workers);
    host.set_repl_mode(true);

    if let Some(file) = file {
        // 初始化文件的输出按求值输出的区块规则排版.
        host.begin_stdout_tracking();
        host.begin_stderr_tracking();
        let loaded = host.load_file(file);
        finish_output_block(&host);
        match loaded {
            Ok(outcome) => {
                for err in &outcome.actor_errors {
                    errln!("{}", render_actor_error(host.sources(), err));
                }
                match outcome.result {
                    Ok(_) => {},
                    Err(RuntimeError::Exit(code)) => {
                        host.shutdown();
                        return ExitCode::from(code);
                    },
                    Err(RuntimeError::BrokenPipe) => {
                        host.shutdown();
                        return ExitCode::SUCCESS;
                    },
                    Err(err) => {
                        // 执行失败时全局环境已恢复, 会话与两份缓存保持空.
                        errln!("{}", render_runtime_error(host.sources(), &err));
                    },
                }
            },
            Err(err) => {
                errln!("{}", render_error(host.sources(), &err));
            },
        }
    }
    let mut ctx = EvalContext { host: &mut host };
    // 命令行代码算作一次输入, 占用第一个编号; 空白代码不算.
    // 非空轮次的输出与结果展示完毕后, 在下一个提示符前留一个空行.
    let mut separator = false;
    let mut round = 1_usize;
    if let Some(code) = code.filter(|code| !code.trim().is_empty()) {
        if let EvalOutcome::Exit(exit) = eval(&mut ctx, "<命令行>", code, round) {
            ctx.host.shutdown();
            return ExitCode::from(exit);
        }
        round += 1;
        separator = true;
    }

    let mut editor = Editor::new();
    loop {
        if separator {
            outln!();
        }
        separator = false;
        match editor.read(ctx.host, round) {
            ReadOutcome::Line(text) => {
                let empty = text.trim().is_empty();
                // 输入回显后固定留一个空行, 分隔输入, 输出与结果.
                // 空输入轮次只保留这一行.
                outln!();
                let outcome = handle_input(&mut ctx, &text, round);
                // 每次非空输入都占用一个编号, 空行不占.
                if !empty {
                    round += 1;
                    separator = true;
                }
                match outcome {
                    InputOutcome::Continue => {},
                    InputOutcome::Quit => break,
                    InputOutcome::Exit(code) => {
                        ctx.host.shutdown();
                        return ExitCode::from(code);
                    },
                }
            },
            // Ctrl-C 放弃本次输入, 会话继续.
            ReadOutcome::Interrupted => {},
            ReadOutcome::Eof => break,
            ReadOutcome::Failed => {
                ctx.host.shutdown();
                return ExitCode::FAILURE;
            },
        }
    }
    ctx.host.shutdown();
    ExitCode::SUCCESS
}

/// 一段完整输入的处置结果.
enum InputOutcome {
    /// 会话继续.
    Continue,
    /// 结束会话.
    Quit,
    /// 程序以 `@Exit` 结束.
    Exit(u8),
}

/// 处置一段完整输入: 先看是否为百分号命令, 否则交给语言求值.
fn handle_input(ctx: &mut EvalContext<'_>, text: &str, round: usize) -> InputOutcome {
    if text.trim().is_empty() {
        return InputOutcome::Continue;
    }
    match handle_command(ctx, text) {
        CommandOutcome::Handled => InputOutcome::Continue,
        CommandOutcome::Quit => InputOutcome::Quit,
        CommandOutcome::NotCommand => match eval(ctx, "<交互>", text, round) {
            EvalOutcome::Ok | EvalOutcome::Failed => InputOutcome::Continue,
            EvalOutcome::Exit(code) => InputOutcome::Exit(code),
        },
    }
}

/// REPL 求值上下文; 宿主持有跨输入的状态.
struct EvalContext<'a> {
    host: &'a mut Host,
}

/// REPL 命令处理结果.
enum CommandOutcome {
    /// 不是百分号命令, 交给语言求值.
    NotCommand,
    /// 已处理, 会话继续.
    Handled,
    /// 退出 REPL.
    Quit,
}

/// 处理 `%命令`.
fn handle_command(ctx: &mut EvalContext<'_>, text: &str) -> CommandOutcome {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('%') else {
        return CommandOutcome::NotCommand;
    };
    let rest = rest.trim();
    if rest.is_empty() {
        errln!("命令为空; 输入 %help 查看命令");
        return CommandOutcome::Handled;
    }
    let (name, args) = match rest.find(char::is_whitespace) {
        Some(index) => (&rest[..index], rest[index..].trim()),
        None => (rest, ""),
    };
    match name {
        "help" => {
            print_command_help();
            CommandOutcome::Handled
        },
        "type" => {
            command_type(ctx, args);
            CommandOutcome::Handled
        },
        "bindings" => {
            command_bindings(ctx);
            CommandOutcome::Handled
        },
        "reset" => {
            command_reset(ctx);
            CommandOutcome::Handled
        },
        "quit" => CommandOutcome::Quit,
        _ => {
            errln!("未知命令: %{name}; 输入 %help 查看命令");
            CommandOutcome::Handled
        },
    }
}

fn print_command_help() {
    outln!("REPL 命令:");
    outln!("  %help              显示本帮助");
    outln!("  %type <表达式>     检查表达式类型");
    outln!("  %bindings          列出当前全局绑定");
    outln!("  %reset             清空会话状态");
    outln!("  %quit              退出 REPL");
}

/// 只对表达式做类型检查, 不执行, 不修改会话.
fn command_type(ctx: &mut EvalContext<'_>, source: &str) {
    let source = source.trim();
    if source.is_empty() {
        errln!("用法: %type <表达式>");
        return;
    }
    let program = match ctx.host.parse("<交互>", source) {
        Ok(program) => program,
        Err(err) => {
            errln!("{}", render_error(ctx.host.sources(), &err));
            return;
        },
    };
    if !program.stmts.is_empty() || program.tail.is_none() {
        errln!("%type 只接受单个表达式");
        return;
    }
    match ctx.host.check_program(&program) {
        Ok(Some(ty)) => outln!(
            "{source} : {}",
            ksl_frontend::check::display_type(&ty, ctx.host.session().type_bindings())
        ),
        Ok(None) => errln!("%type 需要表达式"),
        Err(err) => errln!("{}", render_error(ctx.host.sources(), &err)),
    }
}

/// 列出当前会话的全局绑定.
fn command_bindings(ctx: &EvalContext<'_>) {
    let mut bindings = ctx.host.session().bindings();
    bindings.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));
    if bindings.is_empty() {
        outln!("(无全局绑定)");
        return;
    }
    for (name, ty) in bindings {
        outln!(
            "{} : {}",
            name.as_str(),
            ksl_frontend::check::display_type(&ty, ctx.host.session().type_bindings())
        );
    }
}

/// 清空会话状态; 保留当前根目录.
fn command_reset(ctx: &mut EvalContext<'_>) {
    ctx.host.reset();
    outln!("会话已重置");
}

/// 一段输入的求值结果.
enum EvalOutcome {
    /// 求值成功, 会话状态已提交.
    Ok,
    /// 求值失败, 错误已报告, 会话状态保持原样.
    Failed,
    /// 程序以 `Exit` 终止, 其值为退出码.
    Exit(u8),
}

/// 求值一段输入.
///
/// 宿主负责两阶段提交与回滚; 输出跟踪, Actor 错误与结果展示在本函数完成.
fn eval(ctx: &mut EvalContext<'_>, name: &str, text: &str, round: usize) -> EvalOutcome {
    // 上一条输入的结果已离开栈; 在输入边界回收不可达环.
    ctx.host.collect_cycles();
    // 跟踪程序输出的行首状态与是否写过输出, 本轮据此排版.
    ctx.host.begin_stdout_tracking();
    ctx.host.begin_stderr_tracking();
    let outcome = ctx.host.eval_input(name, text);
    finish_output_block(ctx.host);
    match outcome {
        Err(err) => {
            errln!("{}", render_error(ctx.host.sources(), &err));
            EvalOutcome::Failed
        },
        Ok(outcome) => {
            for err in &outcome.actor_errors {
                errln!("{}", render_actor_error(ctx.host.sources(), err));
            }
            match outcome.result {
                Ok((value, ty)) => {
                    show(&value, ty.as_ref(), ctx.host.session().type_bindings(), round);
                    EvalOutcome::Ok
                },
                Err(RuntimeError::Exit(code)) => EvalOutcome::Exit(code),
                // 输出流已关闭; 继续打印提示与结果没有意义.
                Err(RuntimeError::BrokenPipe) => EvalOutcome::Exit(0),
                Err(err) => {
                    errln!("{}", render_runtime_error(ctx.host.sources(), &err));
                    EvalOutcome::Failed
                },
            }
        },
    }
}

/// 按输出跟踪收尾一段程序输出: 末尾不在行首时补换行, 写过输出时留空行.
fn finish_output_block(host: &Host) {
    let line_start = host.take_stdout_line_start();
    let stdout_wrote = host.take_stdout_wrote();
    let stderr_wrote = host.take_stderr_wrote();
    if !line_start {
        outln!();
    }
    if stdout_wrote || stderr_wrote {
        outln!();
    }
}

/// 显示求值结果, 统一以 `<编号>= ` 前缀, 与 `Print` 的直接输出区分.
///
/// 不可打印类型显示尖括号类型名; 可打印值使用带引号的展示形态.
fn show(value: &Value, ty: Option<&Type>, names: &TypeBindings, round: usize) {
    if let Some(ty) = ty
        && !ksl_frontend::check::is_printable(ty)
    {
        outln!("{round}= <{}>", ksl_frontend::check::display_type(ty, names));
        return;
    }
    outln!("{round}= {}", ksl_value::fmtstr::display_value(value));
}

/// 判定输入是否完整: 括号都已闭合, 且字符串已闭合.
///
/// 借词法器判定字符串的闭合: 词法阶段报"字面量未闭合"即输入未完.
/// 其余词法错误不是未完, 交由语法阶段报告.
/// 括号只按开闭计数, 不校验类型是否配对; 类型不匹配属于语法错误, 应立即报告.
pub(super) fn complete(text: &str) -> bool {
    let mut sources = Sources::default();
    let id = sources.add("<交互>", text);
    let tokens = match lex(sources.text(id), id) {
        Ok(tokens) => tokens,
        Err(err) if matches!(err.kind(), Some(ErrorKind::UnterminatedString)) => return false,
        Err(_) => return true,
    };
    let mut stack: Vec<TokenKind> = Vec::new();
    for token in tokens {
        match token.kind {
            TokenKind::LBracket => stack.push(TokenKind::RBracket),
            TokenKind::LBrace => stack.push(TokenKind::RBrace),
            TokenKind::LParen => stack.push(TokenKind::RParen),
            TokenKind::RBracket | TokenKind::RBrace | TokenKind::RParen => {
                stack.pop();
            },
            _ => {},
        }
    }
    stack.is_empty()
}
