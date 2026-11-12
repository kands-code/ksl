//! 交互输入的记号配色与括号配对.
//!
//! 语法着色与括号匹配都以语言词法器为准, 多行字符串的颜色也能跨行延续.
//! 词法失败时退回普通文本, 由调用方决定是否继续显示.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use ksl_base::name::Name;
use ksl_base::source::Sources;
use ksl_catalog::{builtin, names, reserved};
use ksl_frontend::check::{Type, TypeTarget};
use ksl_frontend::lexer::{Token, TokenKind, lex};
use ksl_host::Host;
use ksl_io::terminal::Ink;

/// 一行内的样式区间; 偏移相对该行起点.
#[derive(Clone, Copy)]
pub(super) struct Span {
    pub start: usize,
    pub end: usize,
    pub ink: Ink,
}

/// 用户定义的名字; 来自会话与当前输入.
#[derive(Default, Clone)]
pub(super) struct HighlightEnv {
    functions: HashSet<Name>,
    types: HashSet<Name>,
    constructors: HashSet<Name>,
}

impl HighlightEnv {
    /// 从宿主会话取全局函数与可见类型.
    pub(super) fn from_host(host: &Host) -> Self {
        let mut env = Self::default();
        let session = host.session();
        for (name, ty) in session.bindings() {
            if matches!(ty, Type::Fn(..)) {
                env.functions.insert(name);
            }
        }
        for (name, binding) in session.type_env().bindings().iter() {
            if matches!(&binding.target, TypeTarget::Data(_) | TypeTarget::Opaque(_)) {
                env.types.insert(name.clone());
            }
        }
        env
    }

    /// 合并当前输入中的定义; 只做词法级识别, 不依赖检查结果.
    fn with_declarations(&self, tokens: &[Token]) -> Self {
        let mut env = self.clone();
        let mut index = 0;
        while index < tokens.len() {
            let Some(declaration) = declaration_head(tokens, index) else {
                index += 1;
                continue;
            };
            match declaration.head {
                "Def" => {
                    env.functions.insert(declaration.name.clone());
                },
                "Data" => {
                    env.types.insert(declaration.name.clone());
                    env.collect_variants(tokens, declaration.after_name);
                },
                "Let" if is_function_binding(tokens, declaration.after_name) => {
                    env.functions.insert(declaration.name.clone());
                },
                _ => {},
            }
            index += 1;
        }
        env
    }

    /// 收集 `@Data` 变体块顶层的变体名.
    fn collect_variants(&mut self, tokens: &[Token], after_name: usize) {
        let Some(rest) = tokens.get(after_name..) else {
            return;
        };
        let Some(brace) = rest.iter().position(|token| token.kind == TokenKind::LBrace) else {
            return;
        };
        let mut depth = 0_usize;
        let mut index = after_name + brace;
        while let Some(token) = tokens.get(index) {
            match &token.kind {
                TokenKind::LBrace | TokenKind::LBracket | TokenKind::LParen => depth += 1,
                TokenKind::RBrace | TokenKind::RBracket | TokenKind::RParen => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                },
                TokenKind::Ident(name)
                    if depth == 1
                        && tokens.get(index + 1).map(|next| &next.kind)
                            == Some(&TokenKind::LBracket) =>
                {
                    self.constructors.insert(name.clone());
                },
                _ => {},
            }
            index += 1;
        }
    }
}

/// 词法层的声明头 `@Head[Name`; 只用于着色, 不校验完整语法.
struct Declaration<'a> {
    head: &'a str,
    name: &'a Name,
    /// 声明名之后第一个记号的下标.
    after_name: usize,
}

/// 尝试从 `index` 读取声明头; 不匹配时返回 None.
fn declaration_head(tokens: &[Token], index: usize) -> Option<Declaration<'_>> {
    let [
        Token {
            kind: TokenKind::At,
            ..
        },
        Token {
            kind: TokenKind::Ident(head),
            ..
        },
        Token {
            kind: TokenKind::LBracket,
            ..
        },
        Token {
            kind: TokenKind::Ident(name),
            ..
        },
        ..,
    ] = &tokens[index..]
    else {
        return None;
    };
    Some(Declaration {
        head: head.as_str(),
        name,
        after_name: index + 4,
    })
}

/// `@Let[Name, @Fn ...]` 的绑定是函数.
fn is_function_binding(tokens: &[Token], after_name: usize) -> bool {
    matches!(
        &tokens[after_name..],
        [
            Token {
                kind: TokenKind::Comma, ..
            },
            Token {
                kind: TokenKind::At, ..
            },
            Token {
                kind: TokenKind::Ident(head), ..
            },
            ..,
        ] if head.as_str() == "Fn"
    )
}

/// 高亮缓存.
///
/// 文本未变时复用记号; 光标移动与尺寸变化只重算样式, 不重新做词法分析.
pub(super) struct Highlighter {
    cache: RefCell<Option<Cache>>,
    env: HighlightEnv,
}

struct Cache {
    text: String,
    tokens: Option<Arc<Vec<Token>>>,
}

impl Highlighter {
    pub(super) fn new() -> Self {
        Self {
            cache: RefCell::new(None),
            env: HighlightEnv::default(),
        }
    }

    /// 更新会话中的用户定义名字.
    pub(super) fn set_env(&mut self, env: HighlightEnv) {
        self.env = env;
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
            tokens.as_ref().map(|tokens| tokens.as_slice()),
            &self.env,
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
    tokens: Option<&[Token]>,
    env: &HighlightEnv,
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
    let env = env.with_declarations(tokens);
    let mut inks = vec![Ink::Plain; full.len()];
    let type_mask = type_positions(tokens);
    for (index, token) in tokens.iter().enumerate() {
        if matches!(token.kind, TokenKind::Eof) {
            break;
        }
        let ink = token_ink_at(tokens, index, &env, type_mask[index]);
        for slot in &mut inks[token.span.start()..token.span.end()] {
            *slot = ink;
        }
    }
    apply_rainbow(tokens, &mut inks);
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
        && lines
            .iter()
            .any(|&(start, end)| start <= cursor && cursor <= end)
    {
        apply_brackets(tokens, cursor, &mut inks);
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
/// 只走词法记号; 字符串与注释里的括号不是括号记号.
fn bracket_marks(tokens: &[Token], pos: usize) -> BracketMarks {
    let brackets: Vec<(usize, usize, BracketKind, bool)> = tokens
        .iter()
        .filter_map(|token| {
            BracketKind::of(&token.kind)
                .map(|(kind, open)| (token.span.start(), token.span.end(), kind, open))
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
fn apply_brackets(tokens: &[Token], cursor: usize, inks: &mut [Ink]) {
    let (first, second, ink) = match bracket_marks(tokens, cursor) {
        BracketMarks::None => return,
        BracketMarks::Pair(first, second) => (first, Some(second), Ink::Bracket),
        BracketMarks::Unmatched(first) => (first, None, Ink::BracketError),
    };
    for start in std::iter::once(first).chain(second) {
        let Some(token) = tokens.iter().find(|token| token.span.start() == start) else {
            continue;
        };
        paint_token(inks, token, ink);
    }
}

/// 按嵌套深度给所有括号着色; 未配对或多出的闭括号标错误色.
fn apply_rainbow(tokens: &[Token], inks: &mut [Ink]) {
    let mut stack: Vec<(BracketKind, usize)> = Vec::new();
    for token in tokens {
        let Some((kind, open)) = BracketKind::of(&token.kind) else {
            continue;
        };
        if open {
            let depth = stack.len();
            paint_token(inks, token, Ink::Rainbow(depth));
            stack.push((kind, depth));
        } else if let Some((opened, depth)) = stack.pop() {
            if opened == kind {
                paint_token(inks, token, Ink::Rainbow(depth));
            } else {
                paint_token(inks, token, Ink::BracketError);
            }
        } else {
            paint_token(inks, token, Ink::BracketError);
        }
    }
}

/// 用一个样式覆盖记号覆盖的全部字节.
fn paint_token(inks: &mut [Ink], token: &Token, ink: Ink) {
    for slot in &mut inks[token.span.start()..token.span.end()] {
        *slot = ink;
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
fn is_literal_constant(name: &Name) -> bool {
    let keywords = ksl_base::name::keywords();
    name.as_str() == keywords.true_.as_str()
        || name.as_str() == keywords.false_.as_str()
        || name.as_str() == keywords.nil_.as_str()
}

/// 类型范围的起点.
#[derive(Clone, Copy)]
struct TypeStart {
    /// 起点处的括号深度.
    depth: usize,
    /// 同级逗号是否结束类型; 类型标注为真, @Data 变体字段为假.
    comma_ends: bool,
}

/// 标记类型位置的记号; 之后按类型着色, 不受变量或特殊形式规则影响.
fn type_positions(tokens: &[Token]) -> Vec<bool> {
    let mut mask = vec![false; tokens.len()];
    let mut depth = 0_usize;
    let mut start: Option<TypeStart> = None;
    let mut data_brace: Option<usize> = None;
    for (index, token) in tokens.iter().enumerate() {
        if matches!(token.kind, TokenKind::Eof) {
            break;
        }
        match &token.kind {
            TokenKind::LBrace => {
                if starts_data_brace(tokens, index) {
                    data_brace = Some(depth + 1);
                }
                depth += 1;
            },
            TokenKind::RBrace => {
                if data_brace == Some(depth) {
                    data_brace = None;
                }
                end_type_at(&mut start, &mut mask, index, depth);
                depth = depth.saturating_sub(1);
            },
            TokenKind::LBracket => {
                if start.is_none()
                    && data_brace == Some(depth)
                    && index > 0
                    && matches!(tokens[index - 1].kind, TokenKind::Ident(_))
                {
                    // 变体字段的全部记号都属于类型, 包括字段之间的逗号.
                    start = Some(TypeStart {
                        depth: depth + 1,
                        comma_ends: false,
                    });
                } else if start.is_some() {
                    mask[index] = true;
                }
                depth += 1;
            },
            TokenKind::RBracket | TokenKind::RParen => {
                end_type_at(&mut start, &mut mask, index, depth);
                depth = depth.saturating_sub(1);
            },
            TokenKind::LParen => {
                if start.is_some() {
                    mask[index] = true;
                }
                depth += 1;
            },
            TokenKind::Comma
                if start.is_some_and(|range| range.comma_ends && range.depth == depth) =>
            {
                start = None;
            },
            TokenKind::Colon | TokenKind::Caret => {
                if start.is_none() {
                    start = Some(TypeStart {
                        depth,
                        comma_ends: true,
                    });
                }
            },
            _ => {
                if start.is_some() {
                    mask[index] = true;
                }
            },
        }
    }
    mask
}

/// 当前括号闭合时结束类型, 否则把闭合括号也标入类型范围.
fn end_type_at(start: &mut Option<TypeStart>, mask: &mut [bool], index: usize, depth: usize) {
    if let Some(range) = *start {
        if depth == range.depth {
            *start = None;
        } else {
            mask[index] = true;
        }
    }
}

/// `{` 是否属于 `@Data[Name, { ... }]` 的变体块.
fn starts_data_brace(tokens: &[Token], index: usize) -> bool {
    let Some(base) = index.checked_sub(5) else {
        return false;
    };
    matches!(
        &tokens[base..index],
        [
            Token {
                kind: TokenKind::At,
                ..
            },
            Token {
                kind: TokenKind::Ident(name),
                ..
            },
            Token {
                kind: TokenKind::LBracket,
                ..
            },
            Token {
                kind: TokenKind::Ident(_),
                ..
            },
            Token {
                kind: TokenKind::Comma,
                ..
            },
        ] if name.as_str() == "Data"
    )
}

/// 类型位置的记号配色.
fn type_ink(name: &Name) -> Ink {
    let text = name.as_str();
    if reserved::PRIMITIVE_TYPE_NAMES.contains(&text) {
        Ink::TypePrimitive
    } else if reserved::TYPE_CONSTRUCTOR_NAMES.contains(&text) || text == "Fn" {
        Ink::TypeConstructor
    } else {
        Ink::Type
    }
}

/// 记号配色: 字面量, 内置函数, 类型, 常量, 特殊形式与其余标识符.
fn token_ink(kind: &TokenKind) -> Ink {
    match kind {
        TokenKind::I64(_)
        | TokenKind::I32(_)
        | TokenKind::U8(_)
        | TokenKind::U32(_)
        | TokenKind::U64(_)
        | TokenKind::F32(_)
        | TokenKind::F64(_) => Ink::Yellow,
        TokenKind::Char(_) | TokenKind::Str(_) | TokenKind::RawStr(_) | TokenKind::FStr(_) => {
            Ink::Green
        },
        // `@` 与 `^` 是前缀符号, 不属于名字本身.
        TokenKind::At | TokenKind::Caret => Ink::Symbol,
        TokenKind::Discard => Ink::Gray,
        TokenKind::Ident(name) => {
            let text = name.as_str();
            if is_literal_constant(name) {
                Ink::Constant
            } else if builtin::id(name).is_some() {
                Ink::Cyan
            } else if reserved::PRIMITIVE_TYPE_NAMES.contains(&text) {
                Ink::TypePrimitive
            } else if reserved::TYPE_CONSTRUCTOR_NAMES.contains(&text) {
                Ink::TypeConstructor
            } else if reserved::BUILTIN_TYPE_NAMES.contains(&text) {
                Ink::Type
            } else if names::highlight_group(text).is_some() {
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
fn token_ink_at(tokens: &[Token], index: usize, env: &HighlightEnv, in_type: bool) -> Ink {
    let token = &tokens[index];
    match &token.kind {
        TokenKind::Slash => Ink::Delimiter,
        TokenKind::Ident(name) if in_type => type_ink(name),
        TokenKind::Ident(name)
            if index > 0 && matches!(tokens[index - 1].kind, TokenKind::At) =>
        {
            if names::highlight_group(name.as_str()).is_some() {
                Ink::Purple
            } else {
                token_ink(&token.kind)
            }
        },
        TokenKind::Ident(name)
            if tokens
                .get(index + 1)
                .is_some_and(|next| matches!(next.kind, TokenKind::Slash)) =>
        {
            type_ink(name)
        },
        TokenKind::Ident(_)
            if index > 0 && matches!(tokens[index - 1].kind, TokenKind::Slash) =>
        {
            Ink::Blue
        },
        TokenKind::Ident(name) => {
            let ink = token_ink(&token.kind);
            if ink != Ink::Plain {
                return ink;
            }
            if env.types.contains(name) {
                Ink::Type
            } else if env.functions.contains(name) {
                Ink::Function
            } else if env.constructors.contains(name) {
                Ink::Blue
            } else {
                Ink::Plain
            }
        },
        _ => token_ink(&token.kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexed(text: &str) -> Vec<Token> {
        lex_tokens(text).expect("测试输入应可词法分析")
    }

    fn single(text: &str, cursor: Option<usize>) -> Vec<Span> {
        let tokens = lexed(text);
        spans(
            text,
            &[(0, text.len())],
            cursor,
            Some(&tokens),
            &HighlightEnv::default(),
        )
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
        let all = spans(text, &lines, Some(9), Some(&tokens), &HighlightEnv::default());
        assert!(all[1].iter().any(|span| span.ink == Ink::Bracket));
    }

    #[test]
    fn cursor_moves_recompute_bracket_emphasis() {
        let highlighter = Highlighter::new();
        let text = "AddI64[1, 2]";
        let lines = [(0, text.len())];
        let head = highlighter.spans(text, &lines, Some(0));
        assert!(head[0].iter().all(|span| span.ink != Ink::Bracket));
        // 同一文本只移动光标: 复用缓存记号, 仍重新计算括号强调.
        let bracket = highlighter.spans(text, &lines, Some(6));
        assert!(bracket[0].iter().any(|span| span.ink == Ink::Bracket));
    }

    #[test]
    fn user_declarations_are_highlighted() {
        let text = "@Def[f, [x : i64] : i64, x]; @Let[y, f[1]];";
        let spans = single(text, None);
        let declaration = text.find("f,").expect("定义名");
        assert_eq!(ink_at(&spans, declaration), Ink::Function);
        let call = text.find("f[1]").expect("调用");
        assert_eq!(ink_at(&spans, call), Ink::Function);
    }

    #[test]
    fn data_declarations_are_highlighted() {
        let text = "@Data[Shape, {Point[], Circle[f64]}]; @Let[s, Shape/Circle[1.0]];";
        let spans = single(text, None);
        let shape = text.find("Shape,").expect("类型名");
        assert_eq!(ink_at(&spans, shape), Ink::Type);
        let point = text.find("Point").expect("变体");
        assert_eq!(ink_at(&spans, point), Ink::Blue);
        let circle = text.find("Circle[f64]").expect("变体");
        assert_eq!(ink_at(&spans, circle), Ink::Blue);
        let qualified = text.rfind("Circle").expect("限定构造器");
        assert_eq!(ink_at(&spans, qualified), Ink::Blue);
    }

    #[test]
    fn session_env_colors_user_functions_and_types() {
        let text = "@Let[y, f[1]]; @Let[s : Shape, Shape/Circle[1.0]];";
        let mut env = HighlightEnv::default();
        env.functions.insert(Name::new("f"));
        env.types.insert(Name::new("Shape"));
        let tokens = lexed(text);
        let spans = spans(text, &[(0, text.len())], None, Some(&tokens), &env)
            .pop()
            .unwrap_or_default();
        let call = text.find("f[1]").expect("调用");
        assert_eq!(ink_at(&spans, call), Ink::Function);
        let annotation = text.find(": Shape").expect("类型标注") + 2;
        assert_eq!(ink_at(&spans, annotation), Ink::Type);
    }

    #[test]
    fn nested_brackets_use_rainbow_colors() {
        let text = "AddI64[1, AddI64[2, 3]]";
        let spans = single(text, None);
        let outer = text.find('[').expect("外层");
        let inner = text.rfind('[').expect("内层");
        assert_eq!(ink_at(&spans, outer), Ink::Rainbow(0));
        assert_eq!(ink_at(&spans, inner), Ink::Rainbow(1));
        let inner_close = text.find(']').expect("内层闭合");
        let outer_close = text.rfind(']').expect("外层闭合");
        assert_eq!(ink_at(&spans, inner_close), Ink::Rainbow(1));
        assert_eq!(ink_at(&spans, outer_close), Ink::Rainbow(0));
    }

    #[test]
    fn tokens_use_role_inks() {
        let spans = single("@Let[x : List[i64], 1]", None);
        assert_eq!(ink_at(&spans, 9), Ink::TypeConstructor, "List");
        assert_eq!(ink_at(&spans, 14), Ink::TypePrimitive, "i64");
        assert_eq!(ink_at(&spans, 0), Ink::Symbol, "@");

        let spans = single("@Export[^Shape]", None);
        assert_eq!(ink_at(&spans, 0), Ink::Symbol, "@");
        assert_eq!(ink_at(&spans, 8), Ink::Symbol, "^");

        let spans = single("_", None);
        assert_eq!(spans[0].ink, Ink::Gray);

        let spans = single("@Block[AddI64[1, 2]]", None);
        assert_eq!(ink_at(&spans, 1), Ink::Purple, "特殊形式");
        assert_eq!(ink_at(&spans, 7), Ink::Cyan, "内置函数");

        let spans = single("@ListMap[xs, f]", None);
        assert_eq!(ink_at(&spans, 1), Ink::Purple, "容器操作");

        let spans = single("true false nil", None);
        assert_eq!(ink_at(&spans, 0), Ink::Constant);
        assert_eq!(ink_at(&spans, 5), Ink::Constant);
        assert_eq!(ink_at(&spans, 11), Ink::Constant);
    }

    #[test]
    fn type_names_use_semantic_inks() {
        let text = r"
@Let[a : i64, a];
@Let[b : List[Shape], b];
@Let[c : Pid[nil, nil], c];
@Let[d, nil];
";
        let spans = single(text, None);
        let ink = |needle: &str| ink_at(&spans, text.find(needle).expect(needle));
        assert_eq!(ink("i64"), Ink::TypePrimitive);
        assert_eq!(ink("List"), Ink::TypeConstructor);
        assert_eq!(ink("Shape"), Ink::Type);
        assert_eq!(ink("Pid"), Ink::TypeConstructor);
        assert_eq!(ink("nil"), Ink::TypePrimitive);
        assert_eq!(
            ink_at(&spans, text.rfind("nil").expect("值 nil")),
            Ink::Constant
        );
    }

    #[test]
    fn type_constructor_names_keep_special_forms() {
        let text = "@Dict[]; @Let[d : Dict[i64, string], d];";
        let spans = single(text, None);
        assert_eq!(ink_at(&spans, 1), Ink::Purple);
        assert_eq!(
            ink_at(&spans, text.rfind("Dict").expect("类型构造器")),
            Ink::TypeConstructor
        );
    }

    #[test]
    fn data_field_types_are_types() {
        let text = "@Data[Shape, {Point[], Circle[f64], Wrap[Fn[[nil] : string]]}];";
        let spans = single(text, None);
        let ink = |needle: &str| ink_at(&spans, text.find(needle).expect(needle));
        assert_eq!(ink("Shape"), Ink::Type);
        assert_eq!(ink("Point"), Ink::Blue);
        assert_eq!(ink("f64"), Ink::TypePrimitive);
        assert_eq!(ink("Fn"), Ink::TypeConstructor);
        assert_eq!(ink("nil"), Ink::TypePrimitive);
    }

    #[test]
    fn function_type_constructor_differs_from_special_form() {
        let text = "@Let[f : Fn[[i64] : string], @Fn[[x : i64] : i64, x]];";
        let spans = single(text, None);
        assert_eq!(
            ink_at(&spans, text.find("Fn").expect("类型构造器")),
            Ink::TypeConstructor
        );
        assert_eq!(ink_at(&spans, text.rfind("Fn").expect("特殊形式")), Ink::Purple);
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
