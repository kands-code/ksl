//! 词法记号定义.
//!
//! 记号只携带源文本区间与最小必要载荷. 字面量类型由类型检查阶段确定.

use ksl_base::name::Name;
use ksl_base::source::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FStrPart {
    /// 纯文本片段, 转义已解码.
    Text(String),
    /// 插值片段: 表达式原文, 可选格式参数与原文区间, 内容由语法阶段校验与再解析.
    Interp {
        expr: String,
        spec: Option<String>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    /// 标识符; 纯下划线序列在词法层直接识别为丢弃记号.
    Ident(Name),
    /// 丢弃记号 `_`, 只允许出现在绑定目标, 模式字段与通配模式中.
    Discard,
    /// `@` 特殊形式前缀.
    At,
    /// `^` 类型提升前缀.
    Caret,
    I64(i64),
    I32(i32),
    U8(u8),
    U32(u32),
    U64(u64),
    F32(f32),
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
    /// 类型限定名的组成部分, 如 `Shape/Circle` 中的斜杠.
    Slash,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}
