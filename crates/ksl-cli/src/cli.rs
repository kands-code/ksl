//! 命令行参数解析.
//!
//! CLI 只有少量固定子命令, 解析自行实现.
//! 解析错误统一以退出码 2 结束, 与语言运行时的退出码区分.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use self::help::{USAGE_REPL, USAGE_RUN, USAGE_TOP};

mod help;

/// 解析后的命令行动作.
pub(crate) enum Action {
    Run {
        source: Source,
        args: Vec<String>,
        io_workers: Option<usize>,
        cpu_workers: Option<usize>,
    },
    Repl {
        file: Option<PathBuf>,
        code: Option<String>,
        io_workers: Option<usize>,
        cpu_workers: Option<usize>,
    },
    Lex(PathBuf),
    Ast(PathBuf),
    Check(PathBuf),
    Cps(PathBuf),
    Linear(PathBuf),
    Lsp,
    Help(Option<Topic>),
    Version,
}

/// `run` 的执行来源; 文件与代码互斥.
pub(crate) enum Source {
    File(PathBuf),
    Code(String),
}

#[derive(Clone, Copy)]
pub(crate) enum Topic {
    Run,
    Repl,
    Lex,
    Ast,
    Check,
    Cps,
    Linear,
    Lsp,
}

impl Topic {
    fn usage(self) -> &'static str {
        help::usage(self)
    }
}

/// 参数错误; 渲染为中文提示并以退出码 2 结束.
pub(crate) struct CliError {
    message: String,
    usage: &'static str,
}

impl CliError {
    fn new(message: impl Into<String>, usage: &'static str) -> Self {
        Self {
            message: message.into(),
            usage,
        }
    }

    /// 渲染完整错误文本, 供标准错误输出.
    pub(crate) fn render(&self) -> String {
        format!("错误: {}\n\n用法: {}\n", self.message, self.usage)
    }
}

/// 解析命令行参数; 不含程序名.
pub(crate) fn parse<I>(args: I) -> Result<Action, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let args: Vec<OsString> = args.into_iter().collect();
    let Some(first) = args.first() else {
        return Ok(Action::Help(None));
    };
    let Some(name) = first.to_str() else {
        return Err(unknown_subcommand(&first.to_string_lossy()));
    };
    match name {
        "-h" | "--help" => Ok(Action::Help(None)),
        "-V" | "--version" => Ok(Action::Version),
        "help" => parse_help(&args[1..]),
        "run" => parse_run(&args[1..]),
        "repl" => parse_repl(&args[1..]),
        "lex" => parse_file(&args[1..], Topic::Lex, Action::Lex),
        "ast" => parse_file(&args[1..], Topic::Ast, Action::Ast),
        "check" => parse_file(&args[1..], Topic::Check, Action::Check),
        "cps" => parse_file(&args[1..], Topic::Cps, Action::Cps),
        "linear" => parse_file(&args[1..], Topic::Linear, Action::Linear),
        "lsp" => parse_lsp(&args[1..]),
        _ if name.starts_with('-') => {
            Err(CliError::new(format!("未知选项: {name}"), USAGE_TOP))
        },
        _ => Err(unknown_subcommand(name)),
    }
}

/// 渲染帮助文本; `None` 表示顶层帮助.
pub(crate) fn help(topic: Option<Topic>) -> String {
    help::render(topic)
}

fn parse_help(args: &[OsString]) -> Result<Action, CliError> {
    if args.len() > 1 {
        return Err(CliError::new("帮助主题之后不应再有参数", USAGE_TOP));
    }
    let Some(name) = args.first() else {
        return Ok(Action::Help(None));
    };
    let Some(name) = name.to_str() else {
        return Err(CliError::new("帮助主题必须是 UTF-8 文本", USAGE_TOP));
    };
    match name {
        "help" | "-h" | "--help" => Ok(Action::Help(None)),
        _ => topic(name)
            .map(|topic| Action::Help(Some(topic)))
            .ok_or_else(|| CliError::new(format!("未知的帮助主题: {name}"), USAGE_TOP)),
    }
}

fn topic(name: &str) -> Option<Topic> {
    match name {
        "run" => Some(Topic::Run),
        "repl" => Some(Topic::Repl),
        "lex" => Some(Topic::Lex),
        "ast" => Some(Topic::Ast),
        "check" => Some(Topic::Check),
        "cps" => Some(Topic::Cps),
        "linear" => Some(Topic::Linear),
        "lsp" => Some(Topic::Lsp),
        _ => None,
    }
}

fn parse_run(args: &[OsString]) -> Result<Action, CliError> {
    let mut file = None;
    let mut code = None;
    let mut io_workers = None;
    let mut cpu_workers = None;
    let mut script_args = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            script_args = args[i + 1..]
                .iter()
                .map(|arg| {
                    arg.to_str()
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| CliError::new("脚本参数必须是 UTF-8 文本", USAGE_RUN))
                })
                .collect::<Result<_, _>>()?;
            break;
        }
        if is_help(arg) {
            return Ok(Action::Help(Some(Topic::Run)));
        }
        if let Some(text) = arg.to_str()
            && let Some(value) = code_value(text, args.get(i + 1), &mut code, USAGE_RUN)?
        {
            i += value;
        } else if let Some(text) = arg.to_str()
            && let Some(value) =
                workers_value(text, args.get(i + 1), &mut io_workers, USAGE_RUN)?
        {
            i += value;
        } else if let Some(text) = arg.to_str()
            && let Some(value) =
                cpu_workers_value(text, args.get(i + 1), &mut cpu_workers, USAGE_RUN)?
        {
            i += value;
        } else if is_option(arg) {
            return Err(unexpected_option(arg, Topic::Run));
        } else if file.is_some() {
            return Err(unexpected_argument(arg, Topic::Run));
        } else {
            file = Some(PathBuf::from(arg));
        }
        i += 1;
    }
    if file.is_some() && code.is_some() {
        return Err(CliError::new("文件与 --code 不能同时使用", USAGE_RUN));
    }
    let source = match (file, code) {
        (Some(file), None) => Source::File(file),
        (None, Some(code)) => Source::Code(code),
        _ => return Err(CliError::new("缺少必需的参数: 文件或 --code", USAGE_RUN)),
    };
    Ok(Action::Run {
        source,
        args: script_args,
        io_workers,
        cpu_workers,
    })
}

fn parse_repl(args: &[OsString]) -> Result<Action, CliError> {
    let mut file = None;
    let mut code = None;
    let mut io_workers = None;
    let mut cpu_workers = None;
    let mut options_ended = false;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if !options_ended && arg == "--" {
            options_ended = true;
            i += 1;
            continue;
        }
        if !options_ended {
            if is_help(arg) {
                return Ok(Action::Help(Some(Topic::Repl)));
            }
            if let Some(text) = arg.to_str()
                && let Some(value) = code_value(text, args.get(i + 1), &mut code, USAGE_REPL)?
            {
                i += value;
                i += 1;
                continue;
            }
            if let Some(text) = arg.to_str()
                && let Some(value) =
                    workers_value(text, args.get(i + 1), &mut io_workers, USAGE_REPL)?
            {
                i += value;
                i += 1;
                continue;
            }
            if let Some(text) = arg.to_str()
                && let Some(value) =
                    cpu_workers_value(text, args.get(i + 1), &mut cpu_workers, USAGE_REPL)?
            {
                i += value;
                i += 1;
                continue;
            }
            if is_option(arg) {
                return Err(unexpected_option(arg, Topic::Repl));
            }
        }
        if file.is_some() {
            return Err(unexpected_argument(arg, Topic::Repl));
        }
        file = Some(PathBuf::from(arg));
        i += 1;
    }
    Ok(Action::Repl {
        file,
        code,
        io_workers,
        cpu_workers,
    })
}

/// 解析 `-w` / `--io-workers` 形态.
/// 返回 `Ok(Some(n))` 表示已消费该选项, `n` 为额外跳过的参数数.
fn workers_value(
    text: &str,
    next: Option<&OsString>,
    workers: &mut Option<usize>,
    usage: &'static str,
) -> Result<Option<usize>, CliError> {
    count_value(text, next, 'w', "io-workers", workers, usage)
}

/// 解析 `-j` / `--cpu-workers` 形态.
fn cpu_workers_value(
    text: &str,
    next: Option<&OsString>,
    workers: &mut Option<usize>,
    usage: &'static str,
) -> Result<Option<usize>, CliError> {
    count_value(text, next, 'j', "cpu-workers", workers, usage)
}

fn count_value(
    text: &str,
    next: Option<&OsString>,
    short: char,
    long: &str,
    slot: &mut Option<usize>,
    usage: &'static str,
) -> Result<Option<usize>, CliError> {
    let Some((value, skip)) = option_value(text, next, short, long, usage)? else {
        return Ok(None);
    };
    let name = format!("--{long}");
    let count = value
        .parse::<usize>()
        .map_err(|_| CliError::new(format!("选项 {name} 的值必须是正整数"), usage))?;
    if count == 0 {
        return Err(CliError::new(format!("选项 {name} 的值必须大于 0"), usage));
    }
    if slot.replace(count).is_some() {
        return Err(CliError::new(format!("选项 {name} 不能重复使用"), usage));
    }
    Ok(Some(skip))
}

/// 解析 `-c` / `--code` 形态.
/// 返回 `Ok(Some(n))` 表示已消费该选项, `n` 为额外跳过的参数数.
fn code_value(
    text: &str,
    next: Option<&OsString>,
    code: &mut Option<String>,
    usage: &'static str,
) -> Result<Option<usize>, CliError> {
    let Some((value, skip)) = option_value(text, next, 'c', "code", usage)? else {
        return Ok(None);
    };
    if code.replace(value).is_some() {
        return Err(CliError::new("选项 --code 不能重复使用", usage));
    }
    Ok(Some(skip))
}

/// 解析带值的选项; 长选项与短选项都接受紧贴值.
///
/// 返回选项值与需要额外跳过的参数数.
/// 下一参数为 `--` 或以 `-` 开头时视为缺值, 单独的 `-` 不算. 此时须改用紧贴形态.
fn option_value(
    text: &str,
    next: Option<&OsString>,
    short: char,
    long: &str,
    usage: &'static str,
) -> Result<Option<(String, usize)>, CliError> {
    let short = format!("-{short}");
    let long = format!("--{long}");
    let value = if text == short.as_str() || text == long.as_str() {
        let Some(next) = next else {
            return Err(CliError::new(format!("选项 {long} 缺少值"), usage));
        };
        if next == "--" || is_option(next) {
            return Err(CliError::new(format!("选项 {long} 缺少值"), usage));
        }
        (utf8_value(next, &long, usage)?, 1)
    } else if let Some(value) = text.strip_prefix(&format!("{long}=")) {
        (value.to_owned(), 0)
    } else if let Some(value) = text
        .strip_prefix(short.as_str())
        .filter(|value| !value.is_empty())
    {
        (value.to_owned(), 0)
    } else {
        return Ok(None);
    };
    Ok(Some(value))
}

fn utf8_value(value: &OsStr, option: &str, usage: &'static str) -> Result<String, CliError> {
    value
        .to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| CliError::new(format!("{option} 的值必须是 UTF-8 文本"), usage))
}

fn parse_file(
    args: &[OsString],
    topic: Topic,
    make: fn(PathBuf) -> Action,
) -> Result<Action, CliError> {
    let mut file = None;
    let mut options_ended = false;
    for arg in args {
        if !options_ended && arg == "--" {
            options_ended = true;
            continue;
        }
        if !options_ended && is_help(arg) {
            return Ok(Action::Help(Some(topic)));
        }
        if !options_ended && is_option(arg) {
            return Err(unexpected_option(arg, topic));
        }
        if file.is_some() {
            return Err(unexpected_argument(arg, topic));
        }
        file = Some(PathBuf::from(arg));
    }
    file.map(make)
        .ok_or_else(|| CliError::new("缺少必需的参数: 文件", topic.usage()))
}

fn parse_lsp(args: &[OsString]) -> Result<Action, CliError> {
    let Some(arg) = args.first() else {
        return Ok(Action::Lsp);
    };
    if is_help(arg) {
        return Ok(Action::Help(Some(Topic::Lsp)));
    }
    if is_option(arg) {
        return Err(unexpected_option(arg, Topic::Lsp));
    }
    Err(unexpected_argument(arg, Topic::Lsp))
}

fn is_help(arg: &OsStr) -> bool {
    matches!(arg.to_str(), Some("-h" | "--help"))
}

fn is_option(arg: &OsStr) -> bool {
    arg.to_str()
        .is_some_and(|text| text.starts_with('-') && text != "-")
}

fn unknown_subcommand(name: &str) -> CliError {
    CliError::new(format!("未知子命令: {name}"), USAGE_TOP)
}

fn unexpected_option(arg: &OsStr, topic: Topic) -> CliError {
    CliError::new(format!("未知选项: {}", arg.to_string_lossy()), topic.usage())
}

fn unexpected_argument(arg: &OsStr, topic: Topic) -> CliError {
    CliError::new(format!("多余的参数: {}", arg.to_string_lossy()), topic.usage())
}
