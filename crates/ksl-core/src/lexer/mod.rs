//! 词法分析.
//!
//! 手写扫描器, 实现 `docs/design.md` 第 2 节与第 4.1 节的字面量与记号规则;
//! 词法阶段只回答形态是否合法, 字面量类型由类型检查阶段按后缀与形态确定.

mod token;

pub use token::{FStrPart, Token, TokenKind};

use crate::error::{ErrorKind, KslError};
use crate::source::{SourceId, Span};

/// 把一份源代码切分为记号序列, 遇到首个错误即返回.
///
/// # Errors
///
/// 返回首个词法错误, 区间指向出错位置.
pub fn lex(source: &str, id: SourceId) -> Result<Vec<Token>, KslError> {
    let mut lexer = Lexer::new(source, id);
    let mut tokens = Vec::new();
    loop {
        lexer.skip_trivia();
        if lexer.is_eof() {
            break;
        }
        tokens.push(lexer.next_token()?);
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(id, lexer.pos, lexer.pos),
    });
    Ok(tokens)
}

struct Lexer<'a> {
    src: &'a str,
    source: SourceId,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str, source: SourceId) -> Self {
        Self {
            src,
            source,
            pos: 0,
        }
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn rest(&self) -> &'a str {
        &self.src[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        let mut chars = self.rest().chars();
        chars.next()?;
        chars.next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn eat(&mut self, expect: char) -> bool {
        if self.peek() == Some(expect) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn err(&self, kind: ErrorKind, start: usize) -> KslError {
        let end = self.pos.max(start + 1);
        KslError::new(kind, Span::new(self.source, start, end))
    }

    /// 跳过空白与 `#` 行注释.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(' ' | '\t' | '\r' | '\n') => {
                    self.bump();
                },
                Some('#') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.bump();
                    }
                },
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Token, KslError> {
        let start = self.pos;
        let kind = match self.peek() {
            Some('@') => {
                self.bump();
                TokenKind::At
            },
            Some('^') => {
                self.bump();
                TokenKind::Caret
            },
            Some('[') => {
                self.bump();
                TokenKind::LBracket
            },
            Some(']') => {
                self.bump();
                TokenKind::RBracket
            },
            Some('{') => {
                self.bump();
                TokenKind::LBrace
            },
            Some('}') => {
                self.bump();
                TokenKind::RBrace
            },
            Some('(') => {
                self.bump();
                TokenKind::LParen
            },
            Some(')') => {
                self.bump();
                TokenKind::RParen
            },
            Some(':') => {
                self.bump();
                TokenKind::Colon
            },
            Some(';') => {
                self.bump();
                TokenKind::Semi
            },
            Some(',') => {
                self.bump();
                TokenKind::Comma
            },
            Some('/') => {
                self.bump();
                TokenKind::Slash
            },
            Some('"') => self.string(start)?,
            Some('\'') => self.char_literal(start)?,
            // f" 开头的格式化字符串优先于普通标识符.
            Some('f') if self.rest().starts_with("f\"") => self.fstring(start)?,
            // r" / r#" 开头的原始字符串优先于普通标识符.
            Some('r') if self.raw_string_ahead() => self.raw_string(start)?,
            Some(c) if c.is_ascii_digit() => self.number(start)?,
            // 一元正负号必须紧贴数字, 落单的符号是词法错误.
            Some('+' | '-') if self.peek_second().is_some_and(|c| c.is_ascii_digit()) => {
                self.number(start)?
            },
            Some(c) if unicode_ident::is_xid_start(c) || c == '_' => {
                self.ident_or_discard(start)?
            },
            Some(c) => {
                let end = start + c.len_utf8();
                return Err(KslError::new(
                    ErrorKind::UnexpectedChar,
                    Span::new(self.source, start, end),
                ));
            },
            None => unreachable!("调用处已确认未到达文件末尾"),
        };
        Ok(Token {
            kind,
            span: Span::new(self.source, start, self.pos),
        })
    }

    /// 判断当前 `r` 是否引导一个原始字符串: `r` 后接任意个 `#`, 再接 `"`.
    fn raw_string_ahead(&self) -> bool {
        let after_r = &self.rest()[1..];
        let hashes = after_r.bytes().take_while(|&b| b == b'#').count();
        after_r[hashes..].starts_with('"')
    }

    /// 标识符或丢弃记号; 下划线后接数字不是合法标识符.
    fn ident_or_discard(&mut self, start: usize) -> Result<TokenKind, KslError> {
        if self.peek() == Some('_') {
            while self.peek() == Some('_') {
                self.bump();
            }
            match self.peek() {
                Some(c) if unicode_ident::is_xid_start(c) => {},
                Some(c) if c.is_ascii_digit() => {
                    let end = self.pos + c.len_utf8();
                    return Err(KslError::new(
                        ErrorKind::UnexpectedChar,
                        Span::new(self.source, start, end),
                    ));
                },
                // 纯下划线序列默认视为丢弃记号 `_`.
                _ => return Ok(TokenKind::Discard),
            }
        } else {
            self.bump();
        }
        while let Some(c) = self.peek() {
            if unicode_ident::is_xid_continue(c) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(TokenKind::Ident(crate::symbol::intern(
            &self.src[start..self.pos],
        )))
    }

    /// 数字字面量; 返回时已消费完整字面量, 类型后缀与范围一并校验.
    fn number(&mut self, start: usize) -> Result<TokenKind, KslError> {
        let negative = match self.peek() {
            Some('-') => {
                self.bump();
                true
            },
            Some('+') => {
                self.bump();
                false
            },
            _ => false,
        };

        let mut is_float = false;
        let magnitude: u128 = if self.rest().starts_with("0x") || self.rest().starts_with("0X")
        {
            self.bump();
            self.bump();
            self.radix_digits(16, start)?
        } else if self.rest().starts_with("0b") {
            self.bump();
            self.bump();
            self.radix_digits(2, start)?
        } else {
            let int_part = self.radix_digits(10, start)?;
            // 小数点后必须紧跟数字, `1.` 是非法形态.
            if self.peek() == Some('.') {
                if !self.peek_second().is_some_and(|c| c.is_ascii_digit()) {
                    return Err(self.err(ErrorKind::InvalidNumber, start));
                }
                is_float = true;
                self.bump();
                self.radix_digits(10, start)?;
            }
            // 指数大小写均可: `e` 与 `E` 同义.
            if matches!(self.peek(), Some('e' | 'E')) {
                is_float = true;
                self.bump();
                if matches!(self.peek(), Some('+' | '-')) {
                    self.bump();
                }
                if !self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    return Err(self.err(ErrorKind::InvalidNumber, start));
                }
                self.radix_digits(10, start)?;
            }
            int_part
        };

        // 数字之后不允许粘连其他标识符字符, 后缀只有 `u` 与 `U`.
        if self.peek().is_some_and(unicode_ident::is_xid_continue)
            && !matches!(self.peek(), Some('u' | 'U'))
        {
            return Err(self.err(ErrorKind::InvalidNumber, start));
        }

        if is_float {
            // 无符号后缀只属于整数, 浮点字面量不得带后缀.
            if matches!(self.peek(), Some('u' | 'U')) {
                return Err(self.err(ErrorKind::InvalidNumber, start));
            }
            let text = &self.src[start..self.pos];
            let value = text
                .parse::<f64>()
                .map_err(|_| self.err(ErrorKind::InvalidNumber, start))?;
            return Ok(TokenKind::F64(value));
        }

        if matches!(self.peek(), Some('u' | 'U')) {
            let wide = self.peek() == Some('U');
            self.bump();
            // 后缀之后同样不允许继续粘连标识符字符.
            if self.peek().is_some_and(unicode_ident::is_xid_continue) {
                return Err(self.err(ErrorKind::InvalidNumber, start));
            }
            if negative {
                // 无符号整数没有负值, 带无符号后缀的负号是词法错误.
                return Err(self.err(ErrorKind::InvalidNumber, start));
            }
            if wide {
                let value = u64::try_from(magnitude)
                    .map_err(|_| self.err(ErrorKind::NumberOutOfRange, start))?;
                return Ok(TokenKind::U64(value));
            }
            if magnitude > u128::from(u8::MAX) {
                return Err(self.err(ErrorKind::NumberOutOfRange, start));
            }
            // 上一个分支已校验 magnitude 不超过 u8::MAX, 转换不会失败.
            let value = u8::try_from(magnitude).expect("范围已在校验分支保证");
            return Ok(TokenKind::U8(value));
        }

        // 负号分支允许 magnitude 取到 2^63, 否则 i64::MIN 无法书写.
        let limit = if negative {
            i64::MAX as u128 + 1
        } else {
            i64::MAX as u128
        };
        if magnitude > limit {
            return Err(self.err(ErrorKind::NumberOutOfRange, start));
        }
        let signed = i128::try_from(magnitude).expect("范围已在校验分支保证");
        let value = if negative { -signed } else { signed };
        let value = i64::try_from(value).expect("范围已在校验分支保证");
        Ok(TokenKind::I64(value))
    }

    /// 消费给定进制的数字序列并累加值; 十六进制同时接受大小写.
    fn radix_digits(&mut self, radix: u32, start: usize) -> Result<u128, KslError> {
        let mut value: u128 = 0;
        let mut count = 0;
        while let Some(c) = self.peek()
            && let Some(digit) = c.to_digit(radix)
        {
            count += 1;
            value = value
                .checked_mul(u128::from(radix))
                .and_then(|v| v.checked_add(u128::from(digit)))
                .ok_or_else(|| self.err(ErrorKind::NumberOutOfRange, start))?;
            self.bump();
        }
        if count == 0 {
            return Err(self.err(ErrorKind::InvalidNumber, start));
        }
        Ok(value)
    }

    /// 普通字符串, 处理转义.
    fn string(&mut self, start: usize) -> Result<TokenKind, KslError> {
        self.bump();
        let mut text = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err(ErrorKind::UnterminatedString, start)),
                Some('"') => break,
                Some('\\') => text.push(self.escape(start)?),
                Some(c) => text.push(c),
            }
        }
        Ok(TokenKind::Str(text))
    }

    /// 字符字面量, 恰好承载一个字符或一个转义.
    fn char_literal(&mut self, start: usize) -> Result<TokenKind, KslError> {
        self.bump();
        let c = match self.bump() {
            None => return Err(self.err(ErrorKind::UnterminatedString, start)),
            Some('\\') => self.escape(start)?,
            Some(c) => c,
        };
        if !self.eat('\'') {
            return Err(self.err(ErrorKind::UnterminatedString, start));
        }
        Ok(TokenKind::Char(c))
    }

    /// 转义序列; `\u{...}` 要求合法 Unicode 标量值.
    fn escape(&mut self, start: usize) -> Result<char, KslError> {
        match self.bump() {
            None => Err(self.err(ErrorKind::UnterminatedString, start)),
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('r') => Ok('\r'),
            Some('0') => Ok('\0'),
            Some('\\') => Ok('\\'),
            Some('"') => Ok('"'),
            Some('\'') => Ok('\''),
            Some('u') => {
                if !self.eat('{') {
                    return Err(self.err(ErrorKind::InvalidEscape, start));
                }
                let digits_start = self.pos;
                while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    self.bump();
                }
                let digits = &self.src[digits_start..self.pos];
                if digits.is_empty() || !self.eat('}') {
                    return Err(self.err(ErrorKind::InvalidEscape, start));
                }
                let code = u32::from_str_radix(digits, 16)
                    .map_err(|_| self.err(ErrorKind::InvalidEscape, start))?;
                char::from_u32(code)
                    .ok_or_else(|| self.err(ErrorKind::InvalidUnicodeScalar, start))
            },
            Some(_) => Err(self.err(ErrorKind::InvalidEscape, start)),
        }
    }

    /// 格式化字符串; 拆分为文本与插值片段, 插值内容交语法阶段校验.
    fn fstring(&mut self, start: usize) -> Result<TokenKind, KslError> {
        self.bump();
        self.bump();
        let mut parts = Vec::new();
        let mut text = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err(ErrorKind::UnterminatedString, start)),
                Some('"') => {
                    self.bump();
                    break;
                },
                Some('{') if self.peek_second() == Some('{') => {
                    self.bump();
                    self.bump();
                    text.push('{');
                },
                Some('}') if self.peek_second() == Some('}') => {
                    self.bump();
                    self.bump();
                    text.push('}');
                },
                Some('{') => {
                    if !text.is_empty() {
                        parts.push(FStrPart::Text(std::mem::take(&mut text)));
                    }
                    parts.push(self.fstr_interp(start)?);
                },
                // 落单的 `}` 必须写作 `}}`.
                Some('}') => return Err(self.err(ErrorKind::InvalidEscape, start)),
                Some('\\') => {
                    self.bump();
                    text.push(self.escape(start)?);
                },
                Some(_) => text.push(self.bump().expect("peek 已确认为 Some")),
            }
        }
        if !text.is_empty() {
            parts.push(FStrPart::Text(text));
        }
        Ok(TokenKind::FStr(parts))
    }

    /// 消费一对花括号及其中的插值原文, 在字符串与字符字面量之外按第一个冒号拆分格式参数.
    fn fstr_interp(&mut self, start: usize) -> Result<FStrPart, KslError> {
        self.bump();
        let inner_start = self.pos;
        let mut spec_at = None;
        loop {
            match self.peek() {
                None => return Err(self.err(ErrorKind::UnterminatedString, start)),
                Some('}') => break,
                // 插值内部不允许嵌套花括号.
                Some('{') => return Err(self.err(ErrorKind::InvalidEscape, start)),
                Some(':') if spec_at.is_none() => {
                    spec_at = Some(self.pos);
                    self.bump();
                },
                Some('"' | '\'') => self.skip_quoted(start)?,
                Some('r') if self.raw_string_ahead() => {
                    self.raw_string(start)?;
                },
                Some(_) => {
                    self.bump();
                },
            }
        }
        let expr_end = spec_at.unwrap_or(self.pos);
        let expr = &self.src[inner_start..expr_end];
        let spec = spec_at.map(|at| self.src[at + 1..self.pos].to_string());
        self.bump();
        if expr.is_empty() {
            return Err(self.err(ErrorKind::EmptyFStrInterp, start));
        }
        Ok(FStrPart::Interp {
            expr: expr.to_string(),
            spec,
        })
    }

    /// 跳过字符串或字符字面量; 转义与 `\u{...}` 一并跳过.
    fn skip_quoted(&mut self, start: usize) -> Result<(), KslError> {
        let Some(quote) = self.bump() else {
            return Err(self.err(ErrorKind::UnterminatedString, start));
        };
        loop {
            match self.bump() {
                None => return Err(self.err(ErrorKind::UnterminatedString, start)),
                Some(c) if c == quote => return Ok(()),
                Some('\\') => match self.peek() {
                    Some('u') => {
                        self.bump();
                        if self.peek() == Some('{') {
                            self.bump();
                            while let Some(c) = self.bump() {
                                if c == '}' {
                                    break;
                                }
                            }
                        }
                    },
                    Some(_) => {
                        self.bump();
                    },
                    None => return Err(self.err(ErrorKind::UnterminatedString, start)),
                },
                Some(_) => {},
            }
        }
    }

    /// 原始字符串; 尾部 `"` 后的 `#` 数量必须与开头一致.
    fn raw_string(&mut self, start: usize) -> Result<TokenKind, KslError> {
        self.bump();
        let mut hashes = 0;
        while self.eat('#') {
            hashes += 1;
        }
        self.bump();
        let content_start = self.pos;
        loop {
            if self.is_eof() {
                return Err(self.err(ErrorKind::UnterminatedString, start));
            }
            if self.peek() == Some('"') {
                let bytes = self.src.as_bytes();
                let mut end = self.pos + 1;
                let mut count = 0;
                while end < bytes.len() && bytes[end] == b'#' {
                    end += 1;
                    count += 1;
                }
                if count == hashes {
                    let content = self.src[content_start..self.pos].to_string();
                    self.pos = end;
                    return Ok(TokenKind::RawStr(content));
                }
            }
            self.bump();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Sources;
    use crate::symbol::intern;

    fn kinds(source: &str) -> Vec<TokenKind> {
        let mut sources = Sources::default();
        let id = sources.add("<test>", source);
        lex(sources.text(id), id)
            .unwrap_or_else(|err| panic!("词法失败: {err}"))
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    fn first_error(source: &str) -> ErrorKind {
        let mut sources = Sources::default();
        let id = sources.add("<test>", source);
        lex(sources.text(id), id).unwrap_err().kind
    }

    #[test]
    fn number_literals_and_errors() {
        // 整数, u8 后缀, 十六进制, 二进制, 浮点, 指数与大小写形态.
        let tokens = kinds("0 255u 0xffu 0Xff -12 +3.5 1e3 1E3 2.5e-2 0b101 0xff");
        assert_eq!(
            tokens,
            [
                TokenKind::I64(0),
                TokenKind::U8(255),
                TokenKind::U8(255),
                TokenKind::I64(255),
                TokenKind::I64(-12),
                TokenKind::F64(3.5),
                TokenKind::F64(1.0e3),
                TokenKind::F64(1.0e3),
                TokenKind::F64(2.5e-2),
                TokenKind::I64(5),
                TokenKind::I64(255),
                TokenKind::Eof,
            ]
        );
        // 负号分支允许取到 2^63, i64::MIN 可书写.
        assert_eq!(
            kinds("-9223372036854775808"),
            [TokenKind::I64(i64::MIN), TokenKind::Eof]
        );
        // u8 值域, 负号配 u 后缀, 松散负号.
        assert_eq!(first_error("256u"), ErrorKind::NumberOutOfRange);
        assert_eq!(first_error("-1u"), ErrorKind::InvalidNumber);
        assert_eq!(first_error("- 12"), ErrorKind::UnexpectedChar);
    }

    #[test]
    fn strings_comments_and_raw_literals() {
        let string_kinds = kinds(r#""a\nb" 'x' '\''"\u{4e2d}""#);
        assert_eq!(
            string_kinds,
            [
                TokenKind::Str("a\nb".to_string()),
                TokenKind::Char('x'),
                TokenKind::Char('\''),
                TokenKind::Str("中".to_string()),
                TokenKind::Eof,
            ]
        );
        let raw_kinds = kinds(r###"r"a\nb" r##"c"d"##"###);
        assert_eq!(
            raw_kinds,
            [
                TokenKind::RawStr("a\\nb".to_string()),
                TokenKind::RawStr("c\"d".to_string()),
                TokenKind::Eof,
            ]
        );
        let comment_kinds = kinds("# 注释\n变量 __temp _ AddI64");
        assert_eq!(
            comment_kinds,
            [
                TokenKind::Ident(intern("变量")),
                TokenKind::Ident(intern("__temp")),
                TokenKind::Discard,
                TokenKind::Ident(intern("AddI64")),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn identifiers_and_unicode() {
        // 组合附加符与 Unicode 数字属于 XID_Continue.
        assert_eq!(
            kinds("e\u{301} x\u{663}"),
            [
                TokenKind::Ident(intern("e\u{301}")),
                TokenKind::Ident(intern("x\u{663}")),
                TokenKind::Eof,
            ]
        );
        // XID_Start 之外的字符不能作为首字符.
        assert_eq!(first_error("\u{663}"), ErrorKind::UnexpectedChar);
        assert_eq!(first_error("_1"), ErrorKind::UnexpectedChar);
    }

    #[test]
    fn fstrings_and_spans() {
        let fstr_kinds = kinds(r#"f"a{x}b{x:.2}{{c}}""#);
        let expected = vec![
            TokenKind::FStr(vec![
                FStrPart::Text("a".to_string()),
                FStrPart::Interp {
                    expr: "x".to_string(),
                    spec: None,
                },
                FStrPart::Text("b".to_string()),
                FStrPart::Interp {
                    expr: "x".to_string(),
                    spec: Some(".2".to_string()),
                },
                FStrPart::Text("{c}".to_string()),
            ]),
            TokenKind::Eof,
        ];
        assert_eq!(fstr_kinds, expected);
        // 字符串与原始字符串内部的冒号和右花括号不结束插值.
        let nested_kinds = kinds(r#"f"{':'} {"a:b"} {r"x}y"}""#);
        assert!(matches!(nested_kinds.first(), Some(TokenKind::FStr(_))));
        assert_eq!(nested_kinds.last(), Some(&TokenKind::Eof));
        // 词法错误区间指向出错位置.
        let source = "0x";
        let mut sources = Sources::default();
        let id = sources.add("<test>", source);
        let err = lex(sources.text(id), id).unwrap_err();
        let span = err.span.expect("词法错误必有位置");
        assert_eq!(&source[span.start..span.end], "0x");
    }
}
