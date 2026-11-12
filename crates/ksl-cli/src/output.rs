//! 标准输出与标准错误的统一写出.
//!
//! 下游关闭管道时静默退出; 其他写出错误报告到标准错误并以失败码退出.

use std::fmt;
use std::io::{self, Write};

/// 写出标准输出; `newline` 为真时补一个换行.
pub(crate) fn stdout(args: fmt::Arguments<'_>, newline: bool) {
    write(io::stdout(), args, newline);
}

/// 写出标准错误; `newline` 为真时补一个换行.
pub(crate) fn stderr(args: fmt::Arguments<'_>, newline: bool) {
    write(io::stderr(), args, newline);
}

fn write(mut stream: impl Write, args: fmt::Arguments<'_>, newline: bool) {
    let result = stream
        .write_fmt(args)
        .and_then(|()| {
            if newline {
                stream.write_all(b"\n")
            } else {
                Ok(())
            }
        })
        .and_then(|()| stream.flush());
    if let Err(error) = result {
        if error.kind() == io::ErrorKind::BrokenPipe {
            std::process::exit(0);
        }
        let _ = writeln!(io::stderr(), "写出输出失败: {error}");
        std::process::exit(1);
    }
}

macro_rules! out {
    () => {
        $crate::output::stdout(format_args!(""), false)
    };
    ($($arg:tt)*) => {
        $crate::output::stdout(format_args!($($arg)*), false)
    };
}

macro_rules! outln {
    () => {
        $crate::output::stdout(format_args!(""), true)
    };
    ($($arg:tt)*) => {
        $crate::output::stdout(format_args!($($arg)*), true)
    };
}

macro_rules! err {
    () => {
        $crate::output::stderr(format_args!(""), false)
    };
    ($($arg:tt)*) => {
        $crate::output::stderr(format_args!($($arg)*), false)
    };
}

macro_rules! errln {
    () => {
        $crate::output::stderr(format_args!(""), true)
    };
    ($($arg:tt)*) => {
        $crate::output::stderr(format_args!($($arg)*), true)
    };
}
