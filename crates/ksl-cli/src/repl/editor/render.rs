//! 固定行的终端输出.
//!
//! 当前行重绘由 `ksl_core::terminal` 负责; 这里只打印已固定的输入行.

use std::io::{self, Write};

use super::super::highlight::Span;

/// 打印一行固定输入; 终端自行处理折行, 不需要光标定位.
pub(super) fn print_fixed(
    out: &mut impl Write,
    prompt: &str,
    line: &str,
    spans: &[Span],
) -> io::Result<()> {
    write!(out, "{prompt}")?;
    if spans.is_empty() {
        write!(out, "{line}")?;
    } else {
        for span in spans {
            write!(out, "{}", span.ink.paint(&line[span.start..span.end]))?;
        }
    }
    write!(out, "\r\n")?;
    out.flush()
}
