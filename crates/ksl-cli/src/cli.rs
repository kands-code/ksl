//! 命令行参数解析.
//!
//! CLI 只有少量固定子命令, 自实现解析以保持帮助与错误信息的中文形态.
//! 解析错误统一以退出码 2 结束, 与语言运行时的退出码区分.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

const USAGE_TOP: &str = "ksl <命令> [参数]";
const USAGE_RUN: &str =
    "ksl run <文件> [-- <参数>...]\n       ksl run --code <代码> [-- <参数>...]";
const USAGE_REPL: &str = "ksl repl [文件] [--code <代码>]";

const HELP_TOP: &str = "\
ksl 编程语言

用法: ksl <命令> [参数]

命令:
  run     执行脚本或代码
  repl    进入交互模式
  lex     输出词法记号序列
  ast     输出语法树
  check   只做类型检查, 不执行
  cps     输出树形 CPS IR
  linear  输出线性 IR
  lsp     启动语言服务器
  help    显示帮助或指定命令的帮助

选项:
  -h, --help     显示帮助
  -V, --version  显示版本
";

const HELP_RUN: &str = "\
执行脚本或代码

用法:
  ksl run <文件> [-- <参数>...]
  ksl run --code <代码> [-- <参数>...]

参数:
  <文件>            源文件路径
  [-- <参数>...]    传给脚本的命令行参数

选项:
  -c, --code <代码>  直接执行的代码
  -h, --help         显示帮助
";

const HELP_REPL: &str = "\
进入交互模式

用法:
  ksl repl [文件] [--code <代码>]

参数:
  [文件]  启动前加载的模块文件

选项:
  -c, --code <代码>  启动前执行的代码
  -h, --help         显示帮助
";

const HELP_LSP: &str = "\
启动语言服务器

用法: ksl lsp

选项:
  -h, --help  显示帮助
";

/// 解析后的命令行动作.
pub(crate) enum Action {
    Run {
        source: Source,
        args: Vec<String>,
    },
    Repl {
        file: Option<PathBuf>,
        code: Option<String>,
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

/// 帮助主题.
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
        match self {
            Self::Run => USAGE_RUN,
            Self::Repl => USAGE_REPL,
            Self::Lex => "ksl lex <文件>",
            Self::Ast => "ksl ast <文件>",
            Self::Check => "ksl check <文件>",
            Self::Cps => "ksl cps <文件>",
            Self::Linear => "ksl linear <文件>",
            Self::Lsp => "ksl lsp",
        }
    }

    fn help(self) -> String {
        match self {
            Self::Run => HELP_RUN.to_owned(),
            Self::Repl => HELP_REPL.to_owned(),
            Self::Lex => file_help("lex", "输出词法记号序列"),
            Self::Ast => file_help("ast", "输出语法树"),
            Self::Check => file_help("check", "只做类型检查, 不执行"),
            Self::Cps => file_help("cps", "输出树形 CPS IR"),
            Self::Linear => file_help("linear", "输出线性 IR"),
            Self::Lsp => HELP_LSP.to_owned(),
        }
    }
}

fn file_help(name: &str, about: &str) -> String {
    format!(
        "{about}\n\n用法: ksl {name} <文件>\n\n参数:\n  <文件>  源文件路径\n\n选项:\n  -h, \
         --help  显示帮助\n"
    )
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
    match topic {
        None => HELP_TOP.to_owned(),
        Some(topic) => topic.help(),
    }
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
        _ => None,
    }
}

fn parse_run(args: &[OsString]) -> Result<Action, CliError> {
    let mut file = None;
    let mut code = None;
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
    })
}

fn parse_repl(args: &[OsString]) -> Result<Action, CliError> {
    let mut file = None;
    let mut code = None;
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
    Ok(Action::Repl { file, code })
}

/// 解析 `-c` / `--code` 形态; 返回 Some 表示已消费该选项, 值为需要额外跳过的参数数.
///
/// 长选项与短选项都接受紧贴值, 与常见命令行习惯一致.
fn code_value(
    text: &str,
    next: Option<&OsString>,
    code: &mut Option<String>,
    usage: &'static str,
) -> Result<Option<usize>, CliError> {
    let value = if text == "-c" || text == "--code" {
        let Some(next) = next else {
            return Err(CliError::new("选项 --code 缺少值", usage));
        };
        // 与常见命令行习惯一致: 选项值不能以 `-` 开头, 需要时改用紧贴形态.
        if next == "--" || is_option(next) {
            return Err(CliError::new("选项 --code 缺少值", usage));
        }
        utf8_value(next, "--code", usage)?
    } else if let Some(value) = text.strip_prefix("--code=") {
        value.to_owned()
    } else if let Some(value) = text.strip_prefix("-c").filter(|value| !value.is_empty()) {
        value.to_owned()
    } else {
        return Ok(None);
    };
    if code.replace(value).is_some() {
        return Err(CliError::new("选项 --code 不能重复使用", usage));
    }
    Ok(Some(usize::from(text == "-c" || text == "--code")))
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
