//! 交互输入的记号配色与括号配对.
//!
//! 语法着色与括号匹配都以语言词法器为准: 字符串与注释里的括号不会被误算,
//! 多行字符串的颜色也能跨行延续. 词法失败时退回普通文本, 由调用方决定是否
//! 继续显示.

use std::collections::HashSet;

use ksl_core::lexer::{Token, TokenKind, lex};
use ksl_core::source::Sources;
use ksl_core::symbol::Symbol;
use ksl_core::terminal::Ink;

/// 一行内的样式区间; 偏移相对该行起点.
#[derive(Clone, Copy)]
pub(super) struct Span {
    pub start: usize,
    pub end: usize,
    pub ink: Ink,
}

/// 计算一行在整段输入中的样式.
///
/// `full` 是已提交前缀加当前行的完整文本, `line_start` 与 `line_end` 是当前行
/// 在 `full` 中的字节区间. `cursor` 为绝对字节位置, 只在编辑当前行时给出;
/// 给出时额外计算括号配对. 返回的区间覆盖整行.
pub(super) fn line_spans(
    full: &str,
    line_start: usize,
    line_end: usize,
    cursor: Option<usize>,
    builtins: &HashSet<Symbol>,
) -> Vec<Span> {
    let line_len = line_end.saturating_sub(line_start);
    if line_len == 0 {
        return Vec::new();
    }
    let mut sources = Sources::default();
    let id = sources.add("<高亮>", full);
    let Ok(tokens) = lex(full, id) else {
        return vec![Span {
            start: 0,
            end: line_len,
            ink: Ink::Plain,
        }];
    };
    let mut inks = vec![Ink::Plain; line_len];
    for token in &tokens {
        if matches!(token.kind, TokenKind::Eof) {
            break;
        }
        let start = token.span.start.max(line_start);
        let end = token.span.end.min(line_end);
        if start >= end {
            continue;
        }
        let ink = token_ink(&token.kind, builtins);
        for slot in &mut inks[start - line_start..end - line_start] {
            *slot = ink;
        }
    }
    // 注释从 `#` 到行尾; 只处理未被记号覆盖的位置, 字符串与字符里的 `#` 不受影响.
    let line = &full[line_start..line_end];
    let mut in_comment = false;
    for (offset, byte) in line.bytes().enumerate() {
        if in_comment {
            inks[offset] = Ink::Gray;
        } else if byte == b'#' && inks[offset] == Ink::Plain {
            inks[offset] = Ink::Gray;
            in_comment = true;
        }
    }
    if let Some(cursor) = cursor {
        apply_brackets(&tokens, cursor, line_start, line_end, &mut inks);
    }
    compress(&inks)
}

/// 括号强调的判定结果.
#[derive(Clone, Copy)]
enum BracketMarks {
    /// 光标处不是括号.
    None,
    /// 括号与配对都在整段输入内; 两个值都是字节偏移.
    Pair(usize, usize),
    /// 光标处的括号没有配对.
    Unmatched(usize),
}

/// 括号类型; 只用同一类型参与配对, 不同括号互不干扰.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BracketKind {
    Square,
    Brace,
    Paren,
}

impl BracketKind {
    /// 由记号判定括号类型与开闭; 非括号返回 None.
    fn of(kind: &TokenKind) -> Option<(Self, bool)> {
        match kind {
            TokenKind::LBracket => Some((Self::Square, true)),
            TokenKind::RBracket => Some((Self::Square, false)),
            TokenKind::LBrace => Some((Self::Brace, true)),
            TokenKind::RBrace => Some((Self::Brace, false)),
            TokenKind::LParen => Some((Self::Paren, true)),
            TokenKind::RParen => Some((Self::Paren, false)),
            _ => None,
        }
    }
}

/// 找出光标处的括号及其配对.
///
/// 只走词法记号: 字符串与注释里的括号不是括号记号, 因此不会被误算.
fn bracket_marks(tokens: &[Token], pos: usize) -> BracketMarks {
    let brackets: Vec<(usize, usize, BracketKind, bool)> = tokens
        .iter()
        .filter_map(|token| {
            BracketKind::of(&token.kind)
                .map(|(kind, open)| (token.span.start, token.span.end, kind, open))
        })
        .collect();
    // 光标覆盖括号起点, 或紧邻括号右侧.
    let Some(index) = brackets
        .iter()
        .position(|(start, ..)| *start == pos)
        .or_else(|| brackets.iter().position(|(_, end, ..)| *end == pos))
    else {
        return BracketMarks::None;
    };
    let (start, _, kind, open) = brackets[index];
    let partner = if open {
        // 向后累加同类括号的深度, 回到 0 处即配对.
        let mut depth = 0_usize;
        brackets[index..]
            .iter()
            .find_map(|(other_start, _, other, is_open)| {
                if *other != kind {
                    return None;
                }
                depth = if *is_open {
                    depth + 1
                } else {
                    depth.saturating_sub(1)
                };
                (depth == 0).then_some(*other_start)
            })
    } else {
        // 向前累加同类括号的深度, 回到 0 处即配对.
        let mut depth = 0_usize;
        brackets[..=index]
            .iter()
            .rev()
            .find_map(|(other_start, _, other, is_open)| {
                if *other != kind {
                    return None;
                }
                depth = if *is_open {
                    depth.saturating_sub(1)
                } else {
                    depth + 1
                };
                (depth == 0).then_some(*other_start)
            })
    };
    partner.map_or(BracketMarks::Unmatched(start), |other| {
        BracketMarks::Pair(start, other)
    })
}

/// 把光标处的括号与配对覆盖为强调色.
fn apply_brackets(
    tokens: &[Token],
    cursor: usize,
    line_start: usize,
    line_end: usize,
    inks: &mut [Ink],
) {
    let (first, second, ink) = match bracket_marks(tokens, cursor) {
        BracketMarks::None => return,
        BracketMarks::Pair(first, second) => (first, Some(second), Ink::Bracket),
        BracketMarks::Unmatched(first) => (first, None, Ink::BracketError),
    };
    for start in std::iter::once(first).chain(second) {
        let Some(token) = tokens.iter().find(|token| token.span.start == start) else {
            continue;
        };
        let start = token.span.start.max(line_start);
        let end = token.span.end.min(line_end);
        if start < end {
            for slot in &mut inks[start - line_start..end - line_start] {
                *slot = ink;
            }
        }
    }
}

/// 把逐字节样式压缩为连续区间.
fn compress(inks: &[Ink]) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut start = 0;
    for (index, ink) in inks.iter().enumerate().skip(1) {
        if *ink != inks[start] {
            spans.push(Span {
                start,
                end: index,
                ink: inks[start],
            });
            start = index;
        }
    }
    spans.push(Span {
        start,
        end: inks.len(),
        ink: inks[start],
    });
    spans
}

/// 记号配色: 字面量, 内置函数, 保留名与其余标识符.
fn token_ink(kind: &TokenKind, builtins: &HashSet<Symbol>) -> Ink {
    match kind {
        TokenKind::I64(_) | TokenKind::U8(_) | TokenKind::U64(_) | TokenKind::F64(_) => {
            Ink::Yellow
        },
        TokenKind::Char(_) | TokenKind::Str(_) | TokenKind::RawStr(_) | TokenKind::FStr(_) => {
            Ink::Green
        },
        TokenKind::Ident(name) => {
            if builtins.contains(name) {
                Ink::Cyan
            } else if ksl_core::check::is_reserved(*name) {
                Ink::Purple
            } else {
                Ink::Plain
            }
        },
        _ => Ink::Plain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtins() -> HashSet<Symbol> {
        HashSet::new()
    }

    #[test]
    fn brackets_pair_inside_one_line() {
        let text = "AddI64[1, 2]";
        let spans = line_spans(text, 0, text.len(), Some(6), &builtins());
        assert!(spans.iter().any(|span| span.ink == Ink::Bracket));
    }

    #[test]
    fn brackets_respect_lexical_context() {
        // 注释中的括号不参与配对, 仍按注释着色.
        let text = "1 # [";
        let spans = line_spans(text, 0, text.len(), Some(text.len()), &builtins());
        assert!(spans.iter().all(|span| span.ink != Ink::Bracket));
        assert!(spans.iter().any(|span| span.ink == Ink::Gray));

        // 跨行配对仍以完整输入为准.
        let text = "@Block[\n1]";
        let spans = line_spans(text, 8, text.len(), Some(9), &builtins());
        assert!(spans.iter().any(|span| span.ink == Ink::Bracket));
    }
}
