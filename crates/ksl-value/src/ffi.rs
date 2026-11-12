//! 外来函数调用.
//!
//! 插件加载与描述符校验在 `ksl-host`; 本模块只保存调用入口与执行类别.

use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;

use crate::native::{EXECUTION_CPU, EXECUTION_CURRENT, EXECUTION_IO};

mod foreign;

pub use self::foreign::ForeignFunction;

/// 插件调用的执行类别.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    /// 在调度线程内联执行.
    Current,
    /// 在 I/O 线程池执行.
    Io,
    /// 在 CPU 线程池执行.
    Cpu,
}

impl Execution {
    /// 由描述符中的类别编号解析.
    #[must_use]
    pub fn from_abi(value: u32) -> Option<Self> {
        match value {
            EXECUTION_CURRENT => Some(Self::Current),
            EXECUTION_IO => Some(Self::Io),
            EXECUTION_CPU => Some(Self::Cpu),
            _ => None,
        }
    }
}

pub fn plugin_call(message: impl Into<String>, span: Span) -> Error {
    Error::runtime(
        ksl_diagnostics::error::ErrorKind::PluginCallFailed {
            message: message.into(),
        },
        Some(span),
    )
}
