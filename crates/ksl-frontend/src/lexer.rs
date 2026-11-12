//! 词法分析.
//!
//! 手写扫描器, 实现 `docs/topics/language/syntax.md` 的字面量与记号规则.
//! 词法阶段只回答形态是否合法, 字面量类型由类型检查阶段按后缀与形态确定.

mod token;

use ksl_base::source::{SourceId, Span};
use ksl_diagnostics::error::{ErrorKind, KslError};
pub use token::{FStrPart, Token, TokenKind};

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

/// 数值字面量后缀; 决定类型与取值范围.
#[derive(Clone, Copy)]
enum NumSuffix {
    I64,
    I32,
    U8,
    U32,
    U64,
    F32,
    F64,
}

impl NumSuffix {
    fn from_text(text: &str) -> Option<Self> {
        match text {
            "i64" => Some(Self::I64),
            "i32" => Some(Self::I32),
            "u8" => Some(Self::U8),
            "u32" => Some(Self::U32),
            "u64" => Some(Self::U64),
            "f32" => Some(Self::F32),
            "f64" => Some(Self::F64),
            _ => None,
        }
    }

    fn is_float(self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
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
        Ok(TokenKind::Ident(ksl_base::name::Name::new(
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

        // 十六进制前缀只接受小写 `0x`, 与格式化输出保持一致.
        let radix = if self.rest().starts_with("0x") {
            self.bump();
            self.bump();
            Some(16)
        } else if self.rest().starts_with("0b") {
            self.bump();
            self.bump();
            Some(2)
        } else {
            None
        };

        let (magnitude, is_float) = if let Some(radix) = radix {
            (self.radix_digits(radix, start)?, false)
        } else {
            let int_part = self.radix_digits(10, start)?;
            let mut is_float = false;
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
            (int_part, is_float)
        };

        let body_end = self.pos;
        let suffix = self.number_suffix(start)?;
        match suffix {
            Some(suffix) if suffix.is_float() => {
                if radix.is_some() {
                    return Err(self.err(ErrorKind::InvalidNumber, start));
                }
                self.float_token(suffix, &self.src[start..body_end], start)
            },
            Some(suffix) => {
                // 浮点主体不得带整数后缀.
                if is_float {
                    return Err(self.err(ErrorKind::InvalidNumber, start));
                }
                self.int_token(suffix, magnitude, negative, start)
            },
            None if is_float => {
                self.float_token(NumSuffix::F64, &self.src[start..body_end], start)
            },
            None => self.int_token(NumSuffix::I64, magnitude, negative, start),
        }
    }

    /// 消费数字后缀; 未知后缀是词法错误.
    fn number_suffix(&mut self, start: usize) -> Result<Option<NumSuffix>, KslError> {
        let suffix_start = self.pos;
        while self.peek().is_some_and(unicode_ident::is_xid_continue) {
            self.bump();
        }
        if suffix_start == self.pos {
            return Ok(None);
        }
        NumSuffix::from_text(&self.src[suffix_start..self.pos])
            .map(Some)
            .ok_or_else(|| self.err(ErrorKind::InvalidNumber, start))
    }

    /// 按浮点后缀解析主体; 指数与符号由 `parse` 一并处理.
    fn float_token(
        &self,
        suffix: NumSuffix,
        text: &str,
        start: usize,
    ) -> Result<TokenKind, KslError> {
        let invalid = || self.err(ErrorKind::InvalidNumber, start);
        match suffix {
            NumSuffix::F32 => text.parse::<f32>().map(TokenKind::F32).map_err(|_| invalid()),
            NumSuffix::F64 => text.parse::<f64>().map(TokenKind::F64).map_err(|_| invalid()),
            NumSuffix::I64
            | NumSuffix::I32
            | NumSuffix::U8
            | NumSuffix::U32
            | NumSuffix::U64 => {
                unreachable!("浮点主体只接受浮点后缀")
            },
        }
    }

    /// 按整数后缀做范围检查并构造记号.
    fn int_token(
        &self,
        suffix: NumSuffix,
        magnitude: u128,
        negative: bool,
        start: usize,
    ) -> Result<TokenKind, KslError> {
        match suffix {
            NumSuffix::I64 => {
                // 负号分支允许 magnitude 取到 2^63, 以书写 i64::MIN.
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
            },
            NumSuffix::I32 => {
                // 负号分支允许 magnitude 取到 2^31, 以书写 i32::MIN.
                let limit = if negative {
                    i32::MAX as u128 + 1
                } else {
                    i32::MAX as u128
                };
                if magnitude > limit {
                    return Err(self.err(ErrorKind::NumberOutOfRange, start));
                }
                let signed = i128::try_from(magnitude).expect("范围已在校验分支保证");
                let value = if negative { -signed } else { signed };
                let value = i32::try_from(value).expect("范围已在校验分支保证");
                Ok(TokenKind::I32(value))
            },
            NumSuffix::U8 => {
                if negative {
                    return Err(self.err(ErrorKind::InvalidNumber, start));
                }
                let value = u8::try_from(magnitude)
                    .map_err(|_| self.err(ErrorKind::NumberOutOfRange, start))?;
                Ok(TokenKind::U8(value))
            },
            NumSuffix::U32 => {
                if negative {
                    return Err(self.err(ErrorKind::InvalidNumber, start));
                }
                let value = u32::try_from(magnitude)
                    .map_err(|_| self.err(ErrorKind::NumberOutOfRange, start))?;
                Ok(TokenKind::U32(value))
            },
            NumSuffix::U64 => {
                if negative {
                    return Err(self.err(ErrorKind::InvalidNumber, start));
                }
                let value = u64::try_from(magnitude)
                    .map_err(|_| self.err(ErrorKind::NumberOutOfRange, start))?;
                Ok(TokenKind::U64(value))
            },
            NumSuffix::F32 | NumSuffix::F64 => {
                unreachable!("整数主体只接受整数后缀")
            },
        }
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

    /// 字符字面量, 恰好包含一个字符或一个转义.
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

    /// 消费一对花括号及其中的插值原文.
    /// 在字符串与字符字面量之外, 按第一个冒号拆分格式参数.
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
            span: Span::new(self.source, inner_start, expr_end),
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
    use ksl_base::name::Name;
    use ksl_base::source::{Sources, Span};
    use ksl_diagnostics::error::ErrorKind;

    use crate::lexer::{FStrPart, TokenKind, lex};

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
        lex(sources.text(id), id)
            .unwrap_err()
            .kind()
            .cloned()
            .expect("词法错误应来自错误目录")
    }

    #[test]
    fn number_literals() {
        assert_eq!(
            kinds("0 255u8 0xffu8 -12 +3.5 1e3 1E3 2.5e-2 0b101 0xff"),
            [
                TokenKind::I64(0),
                TokenKind::U8(255),
                TokenKind::U8(255),
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
        assert_eq!(
            kinds("-9223372036854775808"),
            [TokenKind::I64(i64::MIN), TokenKind::Eof]
        );
        assert_eq!(
            kinds("1f32 -2.5f32 0x1f 0b1"),
            [
                TokenKind::F32(1.0),
                TokenKind::F32(-2.5),
                TokenKind::I64(31),
                TokenKind::I64(1),
                TokenKind::Eof,
            ]
        );
        assert_eq!(
            kinds("42i64 255u32 4294967295u32 2147483647i32 -2147483648i32"),
            [
                TokenKind::I64(42),
                TokenKind::U32(255),
                TokenKind::U32(u32::MAX),
                TokenKind::I32(i32::MAX),
                TokenKind::I32(i32::MIN),
                TokenKind::Eof,
            ]
        );
        // 十六进制里的 `f`/`f32` 是数字, 不是浮点后缀.
        assert_eq!(
            kinds("0x1f64 0x1fu64 0x1f32"),
            [
                TokenKind::I64(8036),
                TokenKind::U64(31),
                TokenKind::I64(7986),
                TokenKind::Eof,
            ]
        );
        for source in ["256u8", "4294967296u32", "2147483648i32"] {
            assert_eq!(first_error(source), ErrorKind::NumberOutOfRange);
        }
        for source in ["-1u32", "42i", "1f", "1.5f16", "0b1f32", "1e3i32", "0XFF"] {
            assert_eq!(first_error(source), ErrorKind::InvalidNumber);
        }
        assert_eq!(first_error("- 12"), ErrorKind::UnexpectedChar);
    }

    #[test]
    fn string_char_and_raw_literals() {
        assert_eq!(
            kinds(r#""a\nb" 'x' '\''"\u{4e2d}""#),
            [
                TokenKind::Str("a\nb".to_string()),
                TokenKind::Char('x'),
                TokenKind::Char('\''),
                TokenKind::Str("中".to_string()),
                TokenKind::Eof,
            ]
        );
        assert_eq!(
            kinds(r###"r"a\nb" r##"c"d"##"###),
            [
                TokenKind::RawStr("a\\nb".to_string()),
                TokenKind::RawStr("c\"d".to_string()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn comments_and_discard() {
        assert_eq!(
            kinds("# 注释\n变量 __temp _ AddI64"),
            [
                TokenKind::Ident(Name::new("变量")),
                TokenKind::Ident(Name::new("__temp")),
                TokenKind::Discard,
                TokenKind::Ident(Name::new("AddI64")),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn identifiers_and_unicode() {
        assert_eq!(
            kinds("e\u{301} x\u{663}"),
            [
                TokenKind::Ident(Name::new("e\u{301}")),
                TokenKind::Ident(Name::new("x\u{663}")),
                TokenKind::Eof,
            ]
        );
        assert_eq!(first_error("\u{663}"), ErrorKind::UnexpectedChar);
        assert_eq!(first_error("_1"), ErrorKind::UnexpectedChar);
    }

    #[test]
    fn fstring_parts_and_spans() {
        assert_eq!(
            kinds(r#"f"a{x}b{x:.2}{{c}}""#),
            [
                TokenKind::FStr(vec![
                    FStrPart::Text("a".to_string()),
                    FStrPart::Interp {
                        expr: "x".to_string(),
                        spec: None,
                        span: Span {
                            start: 4,
                            end: 5,
                            ..Span::default()
                        },
                    },
                    FStrPart::Text("b".to_string()),
                    FStrPart::Interp {
                        expr: "x".to_string(),
                        spec: Some(".2".to_string()),
                        span: Span {
                            start: 8,
                            end: 9,
                            ..Span::default()
                        },
                    },
                    FStrPart::Text("{c}".to_string()),
                ]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn fstring_nested_punctuation() {
        let nested_kinds = kinds(r#"f"{':'} {"a:b"} {r"x}y"}""#);
        assert!(matches!(nested_kinds.first(), Some(TokenKind::FStr(_))));
        assert_eq!(nested_kinds.last(), Some(&TokenKind::Eof));
    }

    #[test]
    fn lex_error_span_points_at_literal() {
        let source = "0x";
        let mut sources = Sources::default();
        let id = sources.add("<test>", source);
        let err = lex(sources.text(id), id).unwrap_err();
        let span = err.span().expect("词法错误必有位置");
        assert_eq!(&source[span.start()..span.end()], "0x");
    }
}
