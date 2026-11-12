//! 终端文本配色.
//!
//! 直接写 ANSI 序列, 不为着色单独引入依赖.

/// 记号配色.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    /// 不着色.
    Plain,
    /// 数值字面量.
    Yellow,
    /// 字符串与字符字面量.
    Green,
    /// 内置函数名.
    Cyan,
    /// 内置类型名与类型限定部分.
    Type,
    /// 字面常量: true, false, nil.
    Constant,
    /// 特殊形式与操作名.
    Purple,
    /// 构造器与变体名: 类型限定名的斜杠之后, 含模式中省略类型限定的形态.
    Blue,
    /// 注释.
    Gray,
    /// 前缀符号: `@` 与 `^`.
    Symbol,
    /// 限定名分隔符 `/`.
    Delimiter,
    /// 光标处的括号及其配对.
    Bracket,
    /// 光标处的括号没有配对.
    BracketError,
}

impl Ink {
    #[must_use]
    pub fn paint(self, text: &str) -> String {
        match self {
            Self::Plain | Self::Delimiter => text.to_string(),
            Self::Yellow => format!("\x1b[33m{text}\x1b[0m"),
            Self::Green => format!("\x1b[32m{text}\x1b[0m"),
            Self::Cyan => format!("\x1b[36m{text}\x1b[0m"),
            Self::Type => format!("\x1b[1;35m{text}\x1b[0m"),
            Self::Constant => format!("\x1b[31m{text}\x1b[0m"),
            Self::Purple => format!("\x1b[35m{text}\x1b[0m"),
            Self::Blue => format!("\x1b[34m{text}\x1b[0m"),
            Self::Gray | Self::Symbol => format!("\x1b[90m{text}\x1b[0m"),
            Self::Bracket => format!("\x1b[1;36m{text}\x1b[0m"),
            Self::BracketError => format!("\x1b[1;31m{text}\x1b[0m"),
        }
    }
}
