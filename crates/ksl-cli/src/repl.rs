//! 交互模式: 逐段读入并求值, 环境跨段持久.
//!
//! 输入采用单行编辑加固定行续行: 当前行由内置编辑器就地编辑, 括号或字符串
//! 未闭合时把当前行固定到终端, 以 `n| ` 继续输入; 完整后整段提交. 命令以
//! `%` 开头, 不属于语言语法. 结果统一以 `n= ` 前缀显示.

use std::collections::HashSet;
use std::path::Path;
use std::process::ExitCode;

use ksl_core::builtin;
use ksl_core::check::Type;
use ksl_core::diagnostic::{render_actor_error, render_error, render_runtime_error};
use ksl_core::error::{ErrorKind, RuntimeError};
use ksl_core::host::Host;
use ksl_core::lexer::{TokenKind, lex};
use ksl_core::rewrite::TypeNames;
use ksl_core::source::Sources;
use ksl_core::symbol::resolve;
use ksl_core::value::Value;

mod editor;
mod highlight;

use editor::{Editor, ReadOutcome};

/// 进入交互模式.
///
/// 给定文件时先把它作为主模块加载, 执行依赖并绑定导出名字;
/// 加载出错则报告并进入空环境, 会话继续. 给定代码时先执行该代码.
#[allow(clippy::too_many_lines)]
pub fn run(file: Option<&Path>, code: Option<&str>) -> ExitCode {
    let mut host = Host::in_current_dir();

    if let Some(file) = file {
        match host.run_file(file) {
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
                        // 执行失败时全局环境已恢复, 会话与模块缓存保持空.
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
    // 命令行代码算作一次输入, 占用第一个编号.
    let mut round = 1_usize;
    if let Some(code) = code {
        if let EvalOutcome::Exit(exit) = eval(&mut ctx, "<命令行>", code, round) {
            ctx.host.shutdown();
            return ExitCode::from(exit);
        }
        round += 1;
    }

    let builtins: HashSet<_> = builtin::names().into_iter().collect();
    let mut editor = Editor::new(builtins);
    // 非空轮次结束后, 在下一个提示符前留一个空行; 管道模式不加.
    let mut separator = false;
    loop {
        while ctx.host.pending_input() {
            ksl_core::io::begin_stdout_tracking();
            let errors = ctx.host.pump_program();
            let line_start = ksl_core::io::take_stdout_line_start();
            if !line_start {
                outln!();
            }
            for err in &errors {
                errln!("{}", render_actor_error(ctx.host.sources(), err));
            }
        }
        if separator && editor.is_interactive() {
            outln!();
        }
        separator = false;
        match editor.read(round) {
            ReadOutcome::Line(text) => {
                let empty = text.trim().is_empty();
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
            ReadOutcome::Eof | ReadOutcome::Failed => break,
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

/// 处理 `%命令`; 命令不属于语言语法, 只在交互模式生效.
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

/// 显示 REPL 命令帮助.
fn print_command_help() {
    outln!("REPL 命令:");
    outln!("  %help              显示本帮助");
    outln!("  %type <表达式>     只做类型检查, 不执行也不提交绑定");
    outln!("  %bindings          列出当前全局绑定");
    outln!("  %reset             清空全局绑定, 类型环境, 模块缓存与 Actor");
    outln!("  %quit              退出 REPL");
}

/// 只对表达式做类型检查, 不执行, 不修改会话.
fn command_type(ctx: &mut EvalContext<'_>, source: &str) {
    let source = source.trim();
    if source.is_empty() {
        errln!("用法: %type <表达式>");
        return;
    }
    let mut program = match ctx.host.parse("<交互>", source) {
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
    match ctx.host.check_program(&mut program) {
        Ok(Some(ty)) => outln!(
            "{source} : {}",
            ksl_core::check::display_type(&ty, ctx.host.session().type_names())
        ),
        Ok(None) => errln!("%type 需要表达式"),
        Err(err) => errln!("{}", render_error(ctx.host.sources(), &err)),
    }
}

/// 列出当前会话的全局绑定.
fn command_bindings(ctx: &mut EvalContext<'_>) {
    let mut bindings = ctx.host.session().bindings();
    bindings
        .sort_by(|(left, _), (right, _)| resolve(*left).as_ref().cmp(resolve(*right).as_ref()));
    if bindings.is_empty() {
        outln!("(无全局绑定)");
        return;
    }
    for (name, ty) in bindings {
        outln!(
            "{} : {}",
            resolve(name),
            ksl_core::check::display_type(&ty, ctx.host.session().type_names())
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
/// 宿主负责两阶段提交与回滚; 此处只处理输出跟踪, Actor 错误与结果展示.
fn eval(ctx: &mut EvalContext<'_>, name: &str, text: &str, round: usize) -> EvalOutcome {
    // 上一条输入的结果已离开栈; 在输入边界回收不可达环.
    ctx.host.collect_cycles();
    // 跟踪程序输出的行首状态与是否写过输出, 供宿主排版.
    ksl_core::io::begin_stdout_tracking();
    ksl_core::io::begin_stderr_tracking();
    let outcome = ctx.host.eval_input(name, text);
    let line_start = ksl_core::io::take_stdout_line_start();
    let stdout_wrote = ksl_core::io::take_stdout_wrote();
    let stderr_wrote = ksl_core::io::take_stderr_wrote();
    // 程序输出末尾没有换行时补一个, 使随后的内容独占一行.
    if !line_start {
        outln!();
    }
    // 程序写过输出时, 在结果或诊断前留一个空行, 与程序输出分开.
    if stdout_wrote || stderr_wrote {
        outln!();
    }
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
                    show(&value, ty.as_ref(), ctx.host.session().type_names(), round);
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

/// 显示求值结果: 统一以 `<编号>= ` 前缀, 与 `Print` 的直接输出区分.
///
/// 不可打印类型显示尖括号类型名; 可打印值使用带引号的展示形态.
fn show(value: &Value, ty: Option<&Type>, names: &TypeNames, round: usize) {
    if let Some(ty) = ty
        && !ksl_core::check::is_printable(ty)
    {
        outln!("{round}= <{}>", ksl_core::check::display_type(ty, names));
        return;
    }
    outln!("{round}= {}", ksl_core::fmtstr::display_value(value));
}

/// 判定输入是否完整: 括号配平, 且字符串已闭合.
///
/// 借词法器判定字符串的闭合: 词法阶段报"字面量未闭合"即输入未完;
/// 其余词法错误不是未完, 交由语法阶段报告.
pub(super) fn complete(text: &str) -> bool {
    let mut sources = Sources::default();
    let id = sources.add("<交互>", text);
    let tokens = match lex(sources.text(id), id) {
        Ok(tokens) => tokens,
        Err(err) if err.kind == ErrorKind::UnterminatedString => return false,
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
