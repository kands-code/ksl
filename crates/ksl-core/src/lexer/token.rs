//! 词法记号定义.
//!
//! 记号只携带源文本区间与最小必要载荷; 字面量的类型由类型检查阶段按后缀
//! 与形态确定, 词法阶段只保证形态合法, 不做类型层面的取舍.

use crate::source::Span;
use crate::symbol::Symbol;

/// 格式化字符串的片段.
#[derive(Debug, Clone, PartialEq)]
pub enum FStrPart {
    /// 纯文本片段, 转义已解码.
    Text(String),
    /// 插值片段: 表达式原文与可选格式参数, 内容由语法阶段校验与再解析.
    Interp { expr: String, spec: Option<String> },
}

/// 记号种类.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// 标识符; 纯下划线序列在词法层直接识别为抛弃记号.
    Ident(Symbol),
    /// 抛弃记号 `_`, 只允许出现在绑定位, 字段忽略位与字段复用位.
    Discard,
    /// `@` 特殊形式前缀.
    At,
    /// `^` 类型提升前缀.
    Caret,
    I64(i64),
    U8(u8),
    U64(u64),
    F64(f64),
    Char(char),
    /// 普通字符串, 转义已解码.
    Str(String),
    /// 原始字符串, 内容原样保留, 不处理任何转义.
    RawStr(String),
    /// 格式化字符串, 已切分为文本与插值片段.
    FStr(Vec<FStrPart>),
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    LParen,
    RParen,
    Colon,
    Semi,
    Comma,
    /// 受限名字的组成部分, 如 `Shape/Circle` 中的斜杠.
    Slash,
    Eof,
}

/// 带区间的记号.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
