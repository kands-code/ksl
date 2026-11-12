//! 用户主动中断的失败类别.

use std::fmt;

/// 用户主动中断的类别; 每个类别对应一个独立的错误条目.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// 断言失败.
    Assertion,
    /// 主动中止.
    Panic,
    /// 尚未实现.
    Unimplemented,
    /// 不该到达的分支.
    Unreachable,
}

impl fmt::Display for FailKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Assertion => "assert",
            Self::Panic => "panic",
            Self::Unimplemented => "todo",
            Self::Unreachable => "unreachable",
        };
        f.write_str(text)
    }
}
