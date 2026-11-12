//! 交互输入的记号配色与括号配对.
//!
//! 语法着色与括号匹配都以语言词法器为准, 多行字符串的颜色也能跨行延续.
//! 词法失败时退回普通文本, 由调用方决定是否继续显示.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use ksl_base::source::Sources;
use ksl_base::symbol::Symbol;
use ksl_frontend::lexer::{Token, TokenKind, lex};
use ksl_runtime::terminal::Ink;

/// 内置类型名; 与保留名同源, 高亮时单独配色.
static BUILTIN_TYPES: LazyLock<HashSet<Symbol>> = LazyLock::new(|| {
    ksl_frontend::check::builtin_type_names()
        .iter()
        .map(|name| ksl_base::symbol::intern(name))
        .collect()
});

/// 一行内的样式区间; 偏移相对该行起点.
#[derive(Clone, Copy)]
pub(super) struct Span {
    pub start: usize,
    pub end: usize,
    pub ink: Ink,
}

/// 高亮缓存.
///
/// 文本未变时复用记号; 光标移动与尺寸变化只重算样式, 不重新做词法分析.
pub(super) struct Highlighter {
    builtins: HashSet<Symbol>,
    cache: RefCell<Option<Cache>>,
}

struct Cache {
    text: String,
    tokens: Option<Arc<Vec<Token>>>,
}

impl Highlighter {
    pub(super) fn new(builtins: HashSet<Symbol>) -> Self {
        Self {
            builtins,
            cache: RefCell::new(None),
        }
    }

    /// 计算整段输入的逐行样式.
    pub(super) fn spans(
        &self,
        full: &str,
        lines: &[(usize, usize)],
        cursor: Option<usize>,
    ) -> Vec<Vec<Span>> {
        let tokens = self.tokens(full);
        spans(
            full,
            lines,
            cursor,
            &self.builtins,
            tokens.as_ref().map(|tokens| tokens.as_slice()),
        )
    }

    fn tokens(&self, full: &str) -> Option<Arc<Vec<Token>>> {
        let mut slot = self.cache.borrow_mut();
        if let Some(cache) = &*slot
            && cache.text == full
        {
            return cache.tokens.clone();
        }
        let tokens = lex_tokens(full).map(Arc::new);
        *slot = Some(Cache {
            text: full.to_owned(),
            tokens: tokens.clone(),
        });
        tokens
    }
}

/// 对整段输入做一次词法分析; 失败时返回 None.
pub(super) fn lex_tokens(full: &str) -> Option<Vec<Token>> {
    let mut sources = Sources::default();
    let id = sources.add("<高亮>", full);
    lex(full, id).ok()
}

/// 计算整段输入的逐行样式.
///
/// `lines` 是各行在 `full` 中的字节区间, 不含行尾换行, 按顺序排列.
/// `cursor` 为绝对字节位置; 给出时只对光标所在行计算括号配对.
/// `tokens` 为整段输入的词法结果; 为 None 时按普通文本处理.
/// 返回与 `lines` 等长的区间表, 每项覆盖该行.
pub(super) fn spans(
    full: &str,
    lines: &[(usize, usize)],
    cursor: Option<usize>,
    builtins: &HashSet<Symbol>,
    tokens: Option<&[Token]>,
) -> Vec<Vec<Span>> {
    if lines.is_empty() {
        return Vec::new();
    }
    let Some(tokens) = tokens else {
        return lines
            .iter()
            .map(|&(start, end)| plain_line(end.saturating_sub(start)))
            .collect();
    };
    let mut inks = vec![Ink::Plain; full.len()];
    for (index, token) in tokens.iter().enumerate() {
        if matches!(token.kind, TokenKind::Eof) {
            break;
        }
        let ink = token_ink_at(tokens, index, builtins);
        for slot in &mut inks[token.span.start..token.span.end] {
            *slot = ink;
        }
    }
    // 注释从 `#` 到行尾. 只处理未被记号覆盖的位置, 字符串与字符里的 `#` 不受影响.
    for &(start, end) in lines {
        let line = &full[start..end];
        let mut in_comment = false;
        for (offset, byte) in line.bytes().enumerate() {
            let index = start + offset;
            if in_comment {
                inks[index] = Ink::Gray;
            } else if byte == b'#' && inks[index] == Ink::Plain {
                inks[index] = Ink::Gray;
                in_comment = true;
            }
        }
    }
    if let Some(cursor) = cursor
        && let Some(&(start, end)) = lines
            .iter()
            .find(|&&(start, end)| start <= cursor && cursor <= end)
    {
        apply_brackets(tokens, cursor, start, end, &mut inks[start..end]);
    }
    lines
        .iter()
        .map(|&(start, end)| {
            if start >= end {
                Vec::new()
            } else {
                compress(&inks[start..end])
            }
        })
        .collect()
}

/// 一行无法词法分析时的普通样式.
fn plain_line(len: usize) -> Vec<Span> {
    if len == 0 {
        Vec::new()
    } else {
        vec![Span {
            start: 0,
            end: len,
            ink: Ink::Plain,
        }]
    }
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

/// 是否为字面常量 `true`, `false` 或 `nil`.
fn is_literal_constant(name: Symbol) -> bool {
    let keywords = ksl_base::symbol::keywords();
    name == keywords.true_ || name == keywords.false_ || name == keywords.nil_
}

/// 记号配色: 字面量, 内置函数, 内置类型, 常量, 保留名与其余标识符.
fn token_ink(kind: &TokenKind, builtins: &HashSet<Symbol>) -> Ink {
    match kind {
        TokenKind::I64(_) | TokenKind::U8(_) | TokenKind::U64(_) | TokenKind::F64(_) => {
            Ink::Yellow
        },
        TokenKind::Char(_) | TokenKind::Str(_) | TokenKind::RawStr(_) | TokenKind::FStr(_) => {
            Ink::Green
        },
        // `@` 与 `^` 是前缀符号, 不属于名字本身.
        TokenKind::At | TokenKind::Caret => Ink::Symbol,
        TokenKind::Discard => Ink::Gray,
        TokenKind::Ident(name) => {
            if is_literal_constant(*name) {
                Ink::Constant
            } else if builtins.contains(name) {
                Ink::Cyan
            } else if BUILTIN_TYPES.contains(name) {
                Ink::Type
            } else if ksl_frontend::check::is_reserved(*name) {
                Ink::Purple
            } else {
                Ink::Plain
            }
        },
        _ => Ink::Plain,
    }
}

/// 带上下文的记号配色.
///
/// 类型限定名 `Type/Variant` 按位置区分: 斜杠前的类型限定部分按类型配色,
/// 斜杠按分隔符配色, 斜杠后的构造器与变体名单独配色.
/// 模式中省略类型限定的 `/Variant` 同样命中.
fn token_ink_at(tokens: &[Token], index: usize, builtins: &HashSet<Symbol>) -> Ink {
    let token = &tokens[index];
    match &token.kind {
        TokenKind::Slash => Ink::Delimiter,
        TokenKind::Ident(name)
            if tokens
                .get(index + 1)
                .is_some_and(|next| matches!(next.kind, TokenKind::Slash)) =>
        {
            Ink::Type
        },
        TokenKind::Ident(_)
            if index > 0 && matches!(tokens[index - 1].kind, TokenKind::Slash) =>
        {
            Ink::Blue
        },
        _ => token_ink(&token.kind, builtins),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtins() -> HashSet<Symbol> {
        HashSet::new()
    }

    fn lexed(text: &str) -> Vec<Token> {
        lex_tokens(text).expect("测试输入应可词法分析")
    }

    fn single(text: &str, cursor: Option<usize>) -> Vec<Span> {
        let tokens = lexed(text);
        spans(text, &[(0, text.len())], cursor, &builtins(), Some(&tokens))
            .pop()
            .unwrap_or_default()
    }

    fn ink_at(spans: &[Span], offset: usize) -> Ink {
        spans
            .iter()
            .find(|span| span.start <= offset && offset < span.end)
            .map_or(Ink::Plain, |span| span.ink)
    }

    #[test]
    fn brackets_respect_lexical_context() {
        let text = "AddI64[1, 2]";
        let paired = single(text, Some(6));
        assert!(paired.iter().any(|span| span.ink == Ink::Bracket));

        // 注释中的括号不参与配对, 仍按注释着色.
        let text = "1 # [";
        let comment_spans = single(text, Some(text.len()));
        assert!(comment_spans.iter().all(|span| span.ink != Ink::Bracket));
        assert!(comment_spans.iter().any(|span| span.ink == Ink::Gray));

        // 跨行配对仍以完整输入为准.
        let text = "@Block[\n1]";
        let lines = [(0, 7), (8, text.len())];
        let tokens = lexed(text);
        let all = spans(text, &lines, Some(9), &builtins(), Some(&tokens));
        assert!(all[1].iter().any(|span| span.ink == Ink::Bracket));
    }

    #[test]
    fn highlighter_reuses_tokens_for_cursor_moves() {
        let highlighter = Highlighter::new(HashSet::new());
        let text = "AddI64[1, 2]";
        let lines = [(0, text.len())];
        let head = highlighter.spans(text, &lines, Some(0));
        assert!(head[0].iter().all(|span| span.ink != Ink::Bracket));
        // 同一文本只移动光标: 复用缓存记号, 仍重新计算括号强调.
        let bracket = highlighter.spans(text, &lines, Some(6));
        assert!(bracket[0].iter().any(|span| span.ink == Ink::Bracket));
    }

    #[test]
    fn token_inks() {
        let spans = single("@Let[x : List[i64], 1]", None);
        assert_eq!(ink_at(&spans, 9), Ink::Type, "List");
        assert_eq!(ink_at(&spans, 14), Ink::Type, "i64");
        assert_eq!(ink_at(&spans, 0), Ink::Symbol, "@");

        let spans = single("@Export[^Shape]", None);
        assert_eq!(ink_at(&spans, 0), Ink::Symbol, "@");
        assert_eq!(ink_at(&spans, 8), Ink::Symbol, "^");

        let spans = single("_", None);
        assert_eq!(spans[0].ink, Ink::Gray);

        let spans = single("true false nil", None);
        assert_eq!(ink_at(&spans, 0), Ink::Constant);
        assert_eq!(ink_at(&spans, 5), Ink::Constant);
        assert_eq!(ink_at(&spans, 11), Ink::Constant);
    }

    #[test]
    fn qualified_names_split_by_slash_position() {
        // Shape 为类型, 斜杠为分隔符, Circle 为蓝色构造器.
        let text = "Shape/Circle[3.2]";
        let spans = single(text, None);
        assert_eq!(ink_at(&spans, 0), Ink::Type, "Shape 的类型限定部分");
        assert_eq!(ink_at(&spans, 5), Ink::Delimiter, "斜杠是限定名分隔符");
        assert_eq!(ink_at(&spans, 6), Ink::Blue, "Circle 构造器名");

        // 模式里省略类型限定的变体名同样蓝色.
        let text = "(/Node[x], x)";
        let spans = single(text, None);
        assert!(spans.iter().any(|span| span.ink == Ink::Blue));
    }
}
