//! 运行时错误.

use ksl_base::source::Span;

use super::kind::ErrorKind;
use super::ksl::KslError;

/// 运行时错误.
#[derive(Debug, Clone)]
pub enum RuntimeError {
    /// 语言级错误或内部错误; 位置来自控制状态或续延帧携带的源码区间.
    Runtime(KslError),
    /// `@Exit[code]` 终止程序; 不是错误, 由宿主转为退出码.
    Exit(u8),
    /// 标准输出或标准错误已关闭; 由宿主静默结束, 不渲染诊断.
    BrokenPipe,
}

impl RuntimeError {
    /// 构造带位置的 ksl 级错误.
    #[must_use]
    pub fn runtime(kind: ErrorKind, span: Option<Span>) -> Self {
        Self::Runtime(KslError::with_span(kind, span))
    }

    /// 构造内部错误.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Runtime(KslError::internal(message, None))
    }
}
