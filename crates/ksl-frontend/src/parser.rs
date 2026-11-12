//! 语法分析.
//!
//! 词法记号交给 `grammar` 中的 winnow parser, 产出 `ast::Program`.
//! 特殊形式与位置约束分离: 位置由 `validate` 检查.

use ksl_base::source::SourceId;
use ksl_diagnostics::error::KslError;

use crate::ast::Program;
use crate::lexer::{Token, lex};

mod grammar;

/// 词法并解析一份源代码.
///
/// # Errors
///
/// 词法或语法任一阶段失败时返回对应错误.
pub fn parse_text(text: &str, source: SourceId) -> Result<Program, KslError> {
    let tokens = lex(text, source)?;
    parse(&tokens)
}

/// 把记号序列解析为顶层程序.
///
/// # Errors
///
/// 记号序列不符合语法时返回首个错误.
pub fn parse(tokens: &[Token]) -> Result<Program, KslError> {
    let source = tokens
        .first()
        .map_or(SourceId::default(), |token| token.span.source);
    grammar::parse(tokens, source)
}
