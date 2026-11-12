//! 正则表达式的编译, 匹配与错误映射.
//!
//! 引擎为 regex-automata 的 meta 正则: 匹配是线性时间且不回溯,
//! 匹配阶段不会失败, 只有编译会失败.
//! 编译失败按类别归并为内置 Data 类型 `RegexError`.
//!
//! 引擎不支持环视与反向引用, 模式里出现这两者时报 `UnsupportedFeature`.

use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::name::Name;
use ksl_value::value::{Value, data_value, data_value0};
use regex_automata::PatternID;
use regex_automata::meta::{BuildError, Regex as MetaRegex};
use regex_automata::util::captures::Captures;
use regex_syntax::ast::{ErrorKind as AstKind, Span};
use regex_syntax::hir::ErrorKind as HirKind;

/// 编译后的正则.
///
/// 克隆只增加引用计数, 可作为值在 Actor 之间传递.
#[derive(Debug, Clone)]
pub struct CompiledRegex {
    /// 引擎的正则.
    inner: MetaRegex,
    /// 命名捕获组的名称与组下标, 按组下标升序.
    named: Arc<[(ArcStr, u64)]>,
}

/// 一处匹配的结果.
///
/// 偏移按 Unicode 标量计数, 与 `StringLength` 的计数方式一致.
#[derive(Debug, Clone)]
pub struct MatchRecord {
    text: ArcStr,
    start: u64,
    end: u64,
    /// 捕获组文本; 下标 0 是整体匹配, 1 起是捕获组.
    ///
    /// 组未参与匹配时为 None, 与匹配到空串区分.
    groups: Arc<[Option<ArcStr>]>,
    /// 命名捕获组的名称与组下标.
    named: Arc<[(ArcStr, u64)]>,
}

impl CompiledRegex {
    /// 编译正则.
    ///
    /// # Errors
    ///
    /// 模式非法时返回带错误类别与出错位置的 `CompileError`.
    pub fn compile(pattern: &str) -> Result<Self, CompileError> {
        let inner = MetaRegex::new(pattern).map_err(|error| CompileError::from(&error))?;
        let named = collect_named(&inner);
        Ok(Self { inner, named })
    }

    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.inner.is_match(text)
    }

    #[must_use]
    pub fn find(&self, text: &str) -> Option<MatchRecord> {
        let mut counter = CharCounter::default();
        let captures = self.inner.captures_iter(text).next()?;
        Some(self.record(text, &captures, &mut counter))
    }

    #[must_use]
    pub fn find_all(&self, text: &str) -> Vec<MatchRecord> {
        let mut counter = CharCounter::default();
        self.inner
            .captures_iter(text)
            .map(|captures| self.record(text, &captures, &mut counter))
            .collect()
    }

    /// 替换匹配; `limit` 为 0 时替换全部.
    ///
    /// # Panics
    ///
    /// 捕获结果缺少整体匹配时 panic; 迭代器只产出成功匹配, 正常不会发生.
    #[must_use]
    pub fn replace(&self, text: &str, replacement: &str, limit: usize) -> String {
        let mut out = String::new();
        let mut last = 0;
        for (count, captures) in self.inner.captures_iter(text).enumerate() {
            if limit > 0 && count >= limit {
                break;
            }
            let whole = captures.get_match().expect("捕获结果必含整体匹配");
            out.push_str(&text[last..whole.start()]);
            captures.interpolate_string_into(text, replacement, &mut out);
            last = whole.end();
        }
        out.push_str(&text[last..]);
        out
    }

    /// 按匹配切分; 保留空片段.
    #[must_use]
    pub fn split(&self, text: &str) -> Vec<String> {
        self.inner
            .split(text)
            .map(|span| text[span.range()].to_string())
            .collect()
    }

    /// 命名捕获组的名称, 按组下标升序.
    #[must_use]
    pub fn capture_names(&self) -> Vec<String> {
        self.named.iter().map(|(name, _)| name.to_string()).collect()
    }

    /// 由捕获结果构造匹配记录.
    fn record(
        &self,
        text: &str,
        captures: &Captures,
        counter: &mut CharCounter,
    ) -> MatchRecord {
        let whole = captures.get_match().expect("捕获结果必含整体匹配");
        let start = counter.at(text, whole.start());
        let end = counter.at(text, whole.end());
        let mut groups = Vec::with_capacity(captures.group_len());
        for index in 0..captures.group_len() {
            let group = captures
                .get_group(index)
                .map(|span| ArcStr::from(&text[span.range()]));
            groups.push(group);
        }
        MatchRecord {
            text: ArcStr::from(&text[whole.range()]),
            start,
            end,
            groups: Arc::from(groups),
            named: Arc::clone(&self.named),
        }
    }
}

impl MatchRecord {
    /// 空匹配; 匹配文本为空, 只有整体匹配组.
    ///
    /// 没有匹配时给出的占位值.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            text: ArcStr::new(),
            start: 0,
            end: 0,
            groups: Arc::from([Some(ArcStr::new())]),
            named: Arc::from([]),
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn start(&self) -> u64 {
        self.start
    }

    #[must_use]
    pub fn end(&self) -> u64 {
        self.end
    }

    /// 按下标取捕获组文本; 下标 0 是整体匹配.
    ///
    /// 下标越界或组未参与匹配时返回 None.
    #[must_use]
    pub fn group(&self, index: u64) -> Option<&str> {
        let index = usize::try_from(index).ok()?;
        self.groups.get(index)?.as_deref()
    }

    /// 按名称取捕获组文本.
    ///
    /// 名称不存在或组未参与匹配时返回 None.
    #[must_use]
    pub fn named_group(&self, name: &str) -> Option<&str> {
        let (_, index) = self.named.iter().find(|(group, _)| &**group == name)?;
        self.group(*index)
    }
}

/// 编译失败的原因; 与语言层 `RegexError` 的变体一一对应.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileError {
    /// 模式无效, 具体原因未细分.
    Invalid(u64),
    /// 括号未闭合.
    UnclosedGroup(u64),
    /// 括号未打开.
    UnopenedGroup(u64),
    /// 字符类未闭合.
    UnclosedClass(u64),
    /// 字符类无效.
    InvalidClass(u64),
    /// 转义无效.
    InvalidEscape(u64),
    /// 数字字面量无效.
    InvalidNumber(u64),
    /// 量词无效.
    InvalidRepeat(u64),
    /// 组名无效.
    InvalidName(u64),
    /// 组名重复.
    DuplicateName(u64),
    /// 内联标志无效.
    InvalidFlag(u64),
    /// Unicode 类无效.
    InvalidUnicode(u64),
    /// 模式使用了不支持的特性, 即环视与反向引用.
    UnsupportedFeature(u64),
    /// 嵌套过深或捕获组过多.
    TooComplex(u64),
    /// 编译体积超出上限.
    TooLarge,
}

impl CompileError {
    /// 由引擎的构建错误映射.
    fn from(error: &BuildError) -> Self {
        if error.size_limit().is_some() {
            return Self::TooLarge;
        }
        let Some(syntax) = error.syntax_error() else {
            // 既非语法错误也未超体积; 引擎不会给出这种组合, 归入无效一类.
            return Self::Invalid(0);
        };
        match syntax {
            regex_syntax::Error::Parse(err) => {
                Self::from_ast(err.kind(), err.span(), err.pattern())
            },
            regex_syntax::Error::Translate(err) => {
                Self::from_hir(err.kind(), err.span(), err.pattern())
            },
            _ => Self::Invalid(0),
        }
    }

    /// 由语法阶段的错误类别映射.
    fn from_ast(kind: &AstKind, span: &Span, pattern: &str) -> Self {
        let offset = char_offset(pattern, span);
        match kind {
            AstKind::GroupUnclosed => Self::UnclosedGroup(offset),
            AstKind::GroupUnopened => Self::UnopenedGroup(offset),
            AstKind::ClassUnclosed => Self::UnclosedClass(offset),
            AstKind::ClassEscapeInvalid
            | AstKind::ClassRangeInvalid
            | AstKind::ClassRangeLiteral => Self::InvalidClass(offset),
            AstKind::EscapeHexEmpty
            | AstKind::EscapeHexInvalid
            | AstKind::EscapeHexInvalidDigit
            | AstKind::EscapeUnexpectedEof
            | AstKind::EscapeUnrecognized
            | AstKind::SpecialWordBoundaryUnclosed
            | AstKind::SpecialWordBoundaryUnrecognized
            | AstKind::SpecialWordOrRepetitionUnexpectedEof => Self::InvalidEscape(offset),
            AstKind::DecimalEmpty | AstKind::DecimalInvalid => Self::InvalidNumber(offset),
            AstKind::RepetitionCountInvalid
            | AstKind::RepetitionCountDecimalEmpty
            | AstKind::RepetitionCountUnclosed
            | AstKind::RepetitionMissing => Self::InvalidRepeat(offset),
            AstKind::GroupNameEmpty
            | AstKind::GroupNameInvalid
            | AstKind::GroupNameUnexpectedEof => Self::InvalidName(offset),
            AstKind::GroupNameDuplicate { .. } => Self::DuplicateName(offset),
            AstKind::FlagDanglingNegation
            | AstKind::FlagDuplicate { .. }
            | AstKind::FlagRepeatedNegation { .. }
            | AstKind::FlagUnexpectedEof
            | AstKind::FlagUnrecognized => Self::InvalidFlag(offset),
            AstKind::UnicodeClassInvalid => Self::InvalidUnicode(offset),
            AstKind::UnsupportedBackreference | AstKind::UnsupportedLookAround => {
                Self::UnsupportedFeature(offset)
            },
            AstKind::CaptureLimitExceeded | AstKind::NestLimitExceeded(_) => {
                Self::TooComplex(offset)
            },
            _ => Self::Invalid(offset),
        }
    }

    /// 由翻译阶段的错误类别映射.
    fn from_hir(kind: &HirKind, span: &Span, pattern: &str) -> Self {
        let offset = char_offset(pattern, span);
        match kind {
            HirKind::UnicodeNotAllowed
            | HirKind::InvalidUtf8
            | HirKind::InvalidLineTerminator
            | HirKind::UnicodePropertyNotFound
            | HirKind::UnicodePropertyValueNotFound
            | HirKind::UnicodePerlClassNotFound
            | HirKind::UnicodeCaseUnavailable => Self::InvalidUnicode(offset),
            _ => Self::Invalid(offset),
        }
    }

    /// 转为语言层 `RegexError` Data 值.
    #[must_use]
    pub fn to_value(self) -> Value {
        let ty = Name::new("RegexError");
        let offset = |value: u64| [Value::U64(value)];
        match self {
            Self::Invalid(at) => data_value(ty, Name::new("Invalid"), offset(at)),
            Self::UnclosedGroup(at) => data_value(ty, Name::new("UnclosedGroup"), offset(at)),
            Self::UnopenedGroup(at) => data_value(ty, Name::new("UnopenedGroup"), offset(at)),
            Self::UnclosedClass(at) => data_value(ty, Name::new("UnclosedClass"), offset(at)),
            Self::InvalidClass(at) => data_value(ty, Name::new("InvalidClass"), offset(at)),
            Self::InvalidEscape(at) => data_value(ty, Name::new("InvalidEscape"), offset(at)),
            Self::InvalidNumber(at) => data_value(ty, Name::new("InvalidNumber"), offset(at)),
            Self::InvalidRepeat(at) => data_value(ty, Name::new("InvalidRepeat"), offset(at)),
            Self::InvalidName(at) => data_value(ty, Name::new("InvalidName"), offset(at)),
            Self::DuplicateName(at) => data_value(ty, Name::new("DuplicateName"), offset(at)),
            Self::InvalidFlag(at) => data_value(ty, Name::new("InvalidFlag"), offset(at)),
            Self::InvalidUnicode(at) => data_value(ty, Name::new("InvalidUnicode"), offset(at)),
            Self::UnsupportedFeature(at) => {
                data_value(ty, Name::new("UnsupportedFeature"), offset(at))
            },
            Self::TooComplex(at) => data_value(ty, Name::new("TooComplex"), offset(at)),
            Self::TooLarge => data_value0(ty, Name::new("TooLarge")),
        }
    }
}

/// 收集命名捕获组的名称与组下标.
fn collect_named(inner: &MetaRegex) -> Arc<[(ArcStr, u64)]> {
    let mut named = Vec::new();
    for (index, name) in inner.group_info().pattern_names(PatternID::ZERO).enumerate() {
        if let Some(name) = name {
            named.push((ArcStr::from(name), to_u64(index)));
        }
    }
    Arc::from(named)
}

/// 字节位置到字符位置的换算器.
///
/// 匹配位置单调不减, 故逐段推进, 总代价与文本长度同阶.
#[derive(Debug, Default)]
struct CharCounter {
    /// 已推进到的字节位置.
    byte: usize,
    /// 该字节位置对应的字符位置.
    char: u64,
}

impl CharCounter {
    /// 推进到给定字节位置, 返回其字符位置.
    ///
    /// 字节位置落在字符边界上; 引擎以 UTF-8 模式运行.
    fn at(&mut self, text: &str, byte: usize) -> u64 {
        if byte < self.byte {
            // 位置回退时从头重数.
            self.byte = 0;
            self.char = 0;
        }
        let segment = if let Some(segment) = text.get(self.byte..byte) {
            segment
        } else {
            // 字节位置不在字符边界上时从头重数.
            self.byte = 0;
            self.char = 0;
            text.get(..byte).unwrap_or_default()
        };
        self.char += count_chars(segment);
        self.byte = byte;
        self.char
    }
}

/// 出错位置换算成字符偏移.
fn char_offset(pattern: &str, span: &Span) -> u64 {
    count_chars(pattern.get(..span.start.offset).unwrap_or_default())
}

/// 计数 Unicode 标量数量; 超出 u64 上限时饱和.
fn count_chars(text: &str) -> u64 {
    to_u64(text.chars().count())
}

/// 把计数换算成 u64; 超出 u64 上限时饱和.
fn to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
