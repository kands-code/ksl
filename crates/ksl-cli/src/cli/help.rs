//! 命令行帮助文本.
//!
//! 用法摘要与完整帮助集中在本模块; 参数解析只引用用法摘要.

use super::Topic;

pub(super) const USAGE_TOP: &str = "ksl <命令> [参数]";
pub(super) const USAGE_RUN: &str = "\
ksl run <文件> [选项] [-- <参数>...]
      ksl run --code <代码> [选项] [-- <参数>...]";
pub(super) const USAGE_REPL: &str = "ksl repl [选项] [文件]";

const HELP_RUN: &str = "\
执行脚本或代码

用法:
  ksl run <文件> [选项] [-- <参数>...]
  ksl run --code <代码> [选项] [-- <参数>...]

参数:
  <文件>            源文件路径
  [-- <参数>...]    传给脚本的命令行参数

选项:
  -c, --code <代码>          直接执行的代码
  -w, --io-workers <数量>    阻塞 I/O 工作线程数, 默认按 CPU 核数取 4-16
  -j, --cpu-workers <数量>   CPU 工作线程数, 默认等于 CPU 核数
  -h, --help                 显示帮助
";

const HELP_REPL: &str = "\
进入交互模式

用法:
  ksl repl [选项] [文件]

参数:
  [文件]  启动前加载的模块文件

选项:
  -c, --code <代码>          启动前执行的代码
  -w, --io-workers <数量>    阻塞 I/O 工作线程数, 默认按 CPU 核数取 4-16
  -j, --cpu-workers <数量>   CPU 工作线程数, 默认等于 CPU 核数
  -h, --help                 显示帮助
";

const HELP_LSP: &str = "\
启动语言服务器

用法: ksl lsp

选项:
  -h, --help  显示帮助
";

/// 指定主题的用法摘要.
pub(super) fn usage(topic: Topic) -> &'static str {
    match topic {
        Topic::Run => USAGE_RUN,
        Topic::Repl => USAGE_REPL,
        Topic::Lex => "ksl lex <文件>",
        Topic::Ast => "ksl ast <文件>",
        Topic::Check => "ksl check <文件>",
        Topic::Cps => "ksl cps <文件>",
        Topic::Linear => "ksl linear <文件>",
        Topic::Lsp => "ksl lsp",
    }
}

/// 渲染帮助文本; `None` 表示顶层帮助.
pub(super) fn render(topic: Option<Topic>) -> String {
    match topic {
        None => top_help(),
        Some(topic) => topic_help(topic),
    }
}

fn topic_help(topic: Topic) -> String {
    match topic {
        Topic::Run => HELP_RUN.to_owned(),
        Topic::Repl => HELP_REPL.to_owned(),
        Topic::Lex => file_help("lex", "输出词法记号序列"),
        Topic::Ast => file_help("ast", "输出语法树"),
        Topic::Check => file_help("check", "只做类型检查, 不执行"),
        Topic::Cps => file_help("cps", "输出树形 CPS IR"),
        Topic::Linear => file_help("linear", "输出线性 IR"),
        Topic::Lsp => HELP_LSP.to_owned(),
    }
}

fn file_help(name: &str, about: &str) -> String {
    format!(
        "{about}

用法: ksl {name} <文件>

参数:
  <文件>  源文件路径

选项:
  -h, --help  显示帮助
"
    )
}

/// 顶层帮助; 关闭 `lsp` 功能时不列出语言服务器.
fn top_help() -> String {
    #[cfg(feature = "lsp")]
    const LSP: &str = "  lsp     启动语言服务器\n";
    #[cfg(not(feature = "lsp"))]
    const LSP: &str = "";
    format!(
        "\
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
{LSP}  help    显示帮助或指定命令的帮助

选项:
  -h, --help     显示帮助
  -V, --version  显示版本
"
    )
}
