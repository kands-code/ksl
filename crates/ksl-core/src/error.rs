//! 错误目录: 全部用户可见错误的唯一来源.
//!
//! 词法, 语法, 类型检查, 运行时四阶段的错误归入同一枚举; 每种错误有稳定
//! 编号与规范标题, 编号分段为 `E01xx` 词法, `E02xx` 语法, `E03xx` 类型
//! 检查, `E04xx` 运行时.
//!
//! 编号按阶段分段; 退役的编号不再复用. 编号由 `ksl_macros::ErrorNumber`
//! 按阶段基数与声明顺序生成, 调整顺序即自动重排.
//! 内部错误即实现缺陷, 不入目录, 单独渲染.
//!
//! 种类可携带数据, 如期望与实得的类型, 实参个数; 细节由种类生成, 措辞
//! 集中在一处.

use std::fmt;

use ksl_macros::ErrorNumber;

use crate::ast::FailKind;
use crate::check::Type;
use crate::source::Span;

/// 错误种类: 目录的唯一条目.
#[derive(Debug, Clone, PartialEq, Eq, ErrorNumber)]
pub enum ErrorKind {
    // 词法
    /// 出现无法构成任何记号的字符, 包括落单的 `+`, `-`.
    #[phase(101)]
    UnexpectedChar,
    /// 字符串, 原始字符串, 格式化字符串或字符字面量未闭合.
    UnterminatedString,
    /// 数字字面量形态非法.
    InvalidNumber,
    /// 数值超出目标类型范围.
    NumberOutOfRange,
    /// 非法转义序列.
    InvalidEscape,
    /// `\u{...}` 不是合法 Unicode 标量值.
    InvalidUnicodeScalar,
    /// 格式化字符串的花括号插值为空.
    EmptyFStrInterp,
    // 语法
    /// 记号与语法预期不符.
    #[phase(201)]
    UnexpectedToken,
    /// 输入在结构中途结束.
    Unterminated,
    /// 未知的 `@` 特殊形式名.
    UnknownSpecialForm,
    /// 格式化字符串的插值不是变量或字面值.
    InvalidFStrInterp,
    /// `@Data` 出现在非顶层位置.
    DataNotTopLevel,
    /// 定义形式出现在普通表达式位置.
    DefinitionNotAllowed,
    /// `@Fix` 的条目不是 `@Def`.
    ExpectedDef,
    /// `@Exit` 出现在非顶层位置.
    ExitNotTopLevel,
    /// 模块声明出现在非顶层位置.
    ModuleFormNotTopLevel,
    // 类型检查
    /// 期望类型与实际类型不符.
    #[phase(301)]
    TypeMismatch { expected: Type, found: Type },
    /// 推断期类型冲突; 未定类型变量以 `_` 呈现.
    InferMismatch { expected: String, found: String },
    /// 名字未定义, 也不是内置函数或构造器.
    UnknownName,
    /// 未知的类型名.
    UnknownType,
    /// 调用的目标不是函数.
    NotCallable { found: Type },
    /// 实参个数不符.
    ArityMismatch { expected: usize, found: usize },
    /// `@Match` 的对象不是可匹配的 Data 值.
    NotMatchable { found: Type },
    /// `@Match` 未覆盖全部变体.
    NonExhaustiveMatch { missing: Vec<String> },
    /// `@Match` 出现重复变体分支.
    DuplicateMatchArm { variant: String },
    /// 绑定的名字是内置函数保留名.
    ReservedName,
    /// 类型无法由值自身确定, 也没有期望给定.
    UndeterminedType { detail: String },
    /// 格式化字符串的格式参数非法.
    InvalidFormatSpec { spec: String },
    /// 顶层语句产生了未被绑定的非 nil 值.
    UnusedValue { found: Type },
    /// 变体与类型重名.
    VariantNameConflict,
    /// 重复声明同名的 Data 类型.
    DuplicateData,
    /// Data 类型没有声明任何变体.
    EmptyData,
    /// 同一 Data 类型内重复声明变体.
    DuplicateVariant,
    /// `@Fix` 组内重复定义同名函数.
    DuplicateFixName,
    /// `@Match` 的通配分支之后仍有分支, 后者不可达.
    UnreachableMatchArm,
    /// 函数参数名重复.
    DuplicateParam,
    /// `@Match` 模式字段名重复.
    DuplicatePatternField { name: String },
    /// Pid 的信箱数不合法.
    InvalidPidType { mailboxes: usize },
    /// Handler 的第一个类型参数不是 Pid.
    InvalidHandlerType,
    /// @Handler 的首参数不是 Pid.
    InvalidHandlerParam,
    /// @Continue 等原语出现在 Handler 体外.
    ActorPrimitiveOutsideHandler,
    /// 信箱数或信箱索引不是非负整数字面量.
    LiteralRequired,
    /// 信箱索引超出 Pid 的信箱数.
    MailboxIndexOutOfRange { index: usize, count: usize },
    /// @Spawn 的 handler 不是 Handler 类型.
    NotHandler { found: Type },
    /// @Send/@Join/@Kill 的实参不是 Pid.
    NotPid { found: Type },
    /// 模块声明尚未经过加载与链接.
    ModuleFormNotResolved,
    /// 该类型没有可打印表示.
    UnprintableType { found: Type },
    /// Dict 的键类型不在支持范围内.
    InvalidDictKey { found: Type },
    /// 模块文件无法读取.
    ModuleReadFailed { path: String, message: String },
    /// 模块路径越出根目录.
    ModulePathEscapesRoot { path: String },
    /// 模块路径不是相对路径.
    InvalidModulePath { path: String },
    /// 同一模块被重复 @Use.
    DuplicateUse { path: String },
    /// 模块依赖成环.
    CyclicImport { chain: Vec<String> },
    /// 导入的名字未由目标模块导出.
    ImportNotExported { module: String, name: String },
    /// 导出的名字未在模块中定义.
    ExportUnknown { name: String },
    /// 本地导入名重复或与本地定义冲突.
    DuplicateImport { name: String },
    /// 导出名重复.
    DuplicateExport { name: String },
    /// 同一模块出现多个导出声明.
    MultipleExportStatements { module: String },
    /// 导出声明出现在模块之外, 如交互输入或脚本.
    ExportOutsideModule,
    /// 导出值或类型的签名使用了未导出的 Data 类型.
    ExportPrivateType { name: String },
    /// REPL 会话中再次直接加载已加载的模块.
    ModuleAlreadyLoaded { path: String },
    /// `@Main` 的参数不是唯一的 Pid self.
    InvalidMainParam,
    /// `@Main` 的返回类型或 Pid 返回类型不是 nil.
    InvalidMainReturn,
    /// `@Main` 出现在模块之外.
    MainOutsideModule,
    /// 同一模块出现多个 `@Main`.
    MultipleMainStatements { module: String },
    /// `@Halt` 出现在 `@Main` 体内.
    HaltInMain,
    /// `@Continue` 出现在 `@Main` 体内.
    ContinueInMain,
    // 运行时
    /// 用户主动中断: 断言失败.
    #[phase(401)]
    AssertionFailed { message: String },
    /// 用户主动中断: 主动中止.
    Panicked { message: String },
    /// 用户主动中断: 尚未实现.
    Unimplemented { message: String },
    /// 用户主动中断: 不该到达的分支.
    UnreachableReached { message: String },
    /// 整数运算溢出.
    IntegerOverflow,
    /// u8 运算溢出.
    U8Overflow,
    /// u64 运算溢出.
    U64Overflow,
    /// 浮点值超出目标整数类型的范围.
    FloatOutOfRange,
    /// 整数转换超出目标类型范围.
    IntConversionOutOfRange,
    /// 除数为零, 或商的绝对值超出范围.
    DivisionByZero,
    /// 字符标量值非法.
    InvalidCharScalar,
    /// 列表下标越界.
    IndexOutOfRange,
    /// 计数超出可表示范围.
    CountOutOfRange,
    /// 对空列表取首元素.
    EmptyListHead,
    /// 插入间隔小于 1.
    InvalidInterval,
    /// 窗口大小小于 1.
    InvalidWindowSize,
    /// 分组大小小于 1.
    InvalidChunkSize,
    /// 尝试 Join 自身.
    JoinSelf,
    /// 所有任务都在等待且无法推进.
    Deadlock,
    /// 等待时间超出平台计时器可表示范围.
    TimeoutOutOfRange,
    // 时间与日期
    /// 日期时间超出可表示范围.
    DateTimeOutOfRange,
    /// 尝试 Join 根 Actor.
    JoinRoot,
    /// 尝试终止根 Actor.
    KillRoot,
}

impl ErrorKind {
    /// 用户主动中断的类别转错误条目; 信息字符串原样作为详情.
    ///
    /// 四个原语共用一条失败通道, 但在目录里各自独立, 报告时能看出类别.
    #[must_use]
    pub fn from_fail(kind: FailKind, message: String) -> Self {
        match kind {
            FailKind::Assertion => Self::AssertionFailed { message },
            FailKind::Panic => Self::Panicked { message },
            FailKind::Unimplemented => Self::Unimplemented { message },
            FailKind::Unreachable => Self::UnreachableReached { message },
        }
    }

    /// 规范标题: 报告首行的固定部分.
    #[must_use]
    pub fn title(&self) -> &'static str {
        match self {
            // 词法
            Self::UnexpectedChar => "无法识别的字符",
            Self::UnterminatedString => "字面量未闭合",
            Self::InvalidNumber => "数字字面量形态非法",
            Self::NumberOutOfRange => "数值超出类型范围",
            Self::InvalidEscape => "非法转义序列",
            Self::InvalidUnicodeScalar => "非法的 Unicode 标量值",
            Self::EmptyFStrInterp => "格式化字符串的插值为空",
            // 语法
            Self::UnexpectedToken => "记号与语法预期不符",
            Self::Unterminated => "输入在结构中途结束",
            Self::UnknownSpecialForm => "未知的特殊形式",
            Self::InvalidFStrInterp => "格式化字符串插值非法",
            Self::DataNotTopLevel => "数据声明不在顶层",
            Self::DefinitionNotAllowed => "定义只能位于顶层或 @Block 语句位置",
            Self::ExpectedDef => "此处需要 @Def 定义",
            Self::ExitNotTopLevel => "@Exit 只能出现在顶层或 @Main 体内",
            Self::ModuleFormNotTopLevel => "模块声明只能出现在顶层",
            // 类型检查
            Self::TypeMismatch { .. } => "类型不符",
            Self::InferMismatch { .. } => "推断类型冲突",
            Self::UnknownName => "未定义的名字",
            Self::UnknownType => "未知的类型名",
            Self::NotCallable { .. } => "不可调用的值",
            Self::ArityMismatch { .. } => "实参个数不符",
            Self::NotMatchable { .. } => "该值不支持 @Match",
            Self::NonExhaustiveMatch { .. } => "@Match 未覆盖全部变体",
            Self::DuplicateMatchArm { .. } => "@Match 存在重复分支",
            Self::ReservedName => "名字为保留名",
            Self::UndeterminedType { .. } => "类型无法确定",
            Self::InvalidFormatSpec { .. } => "非法的格式参数",
            Self::UnusedValue { .. } => "语句的值未被使用",
            Self::VariantNameConflict => "变体不得与类型重名",
            Self::DuplicateData => "重复声明了同名的 Data 类型",
            Self::EmptyData => "Data 类型没有变体",
            Self::DuplicateVariant => "同一 Data 类型内重复声明了变体",
            Self::DuplicateFixName => "@Fix 组内名字重复",
            Self::UnreachableMatchArm => "@Match 存在不可达分支",
            Self::DuplicateParam => "函数参数名重复",
            Self::DuplicatePatternField { .. } => "模式字段名重复",
            Self::InvalidPidType { .. } => "Pid 的信箱数不合法",
            Self::InvalidHandlerType => "Handler 的第一个类型参数必须是 Pid",
            Self::InvalidHandlerParam => "@Handler 的首参数必须是 Pid",
            Self::ActorPrimitiveOutsideHandler => {
                "该 Actor 原语只能在 @Handler 或 @Main 体内使用"
            },
            Self::LiteralRequired => "此处需要非负整数字面量",
            Self::MailboxIndexOutOfRange { .. } => "信箱索引越界",
            Self::NotHandler { .. } => "该值不是 Handler",
            Self::NotPid { .. } => "该值不是 Pid",
            Self::ModuleFormNotResolved => "模块声明尚未经过加载与链接",
            Self::ModuleReadFailed { .. } => "无法读取模块",
            Self::ModulePathEscapesRoot { .. } => "模块路径越出根目录",
            Self::InvalidModulePath { .. } => "模块路径必须是相对路径",
            Self::DuplicateUse { .. } => "同一模块被重复 @Use",
            Self::CyclicImport { .. } => "模块依赖成环",
            Self::ImportNotExported { .. } => "导入的名字未导出",
            Self::ExportUnknown { .. } => "导出的名字未定义",
            Self::DuplicateImport { .. } => "本地导入名重复",
            Self::DuplicateExport { .. } => "导出名重复",
            Self::MultipleExportStatements { .. } => "同一模块只能有一个导出声明",
            Self::ExportOutsideModule => "导出声明只允许出现在模块中",
            Self::ExportPrivateType { .. } => "导出签名使用了未导出的类型",
            Self::ModuleAlreadyLoaded { .. } => "模块已加载",
            Self::InvalidMainParam => "@Main 必须只有一个 Pid self 参数",
            Self::InvalidMainReturn => "@Main 的返回类型必须是 nil",
            Self::MainOutsideModule => "@Main 只允许出现在模块中",
            Self::MultipleMainStatements { .. } => "同一模块只能有一个 @Main",
            Self::HaltInMain => "@Halt 不能出现在 @Main 体内",
            Self::ContinueInMain => "@Continue 不能出现在 @Main 体内",
            Self::UnprintableType { .. } => "该类型不可打印",
            Self::InvalidDictKey { .. } => "Dict 的键类型不受支持",
            // 运行时
            Self::AssertionFailed { .. } => "断言失败",
            Self::Panicked { .. } => "主动中止",
            Self::Unimplemented { .. } => "尚未实现",
            Self::UnreachableReached { .. } => "不可达分支被执行",
            Self::IntegerOverflow => "整数运算溢出",
            Self::U8Overflow => "u8 运算溢出",
            Self::U64Overflow => "u64 运算溢出",
            Self::FloatOutOfRange => "浮点值超出整数范围",
            Self::DivisionByZero => "除数为零",
            Self::IndexOutOfRange => "列表下标越界",
            Self::CountOutOfRange => "计数超出范围",
            Self::EmptyListHead => "对空列表取首元素",
            Self::InvalidInterval => "插入间隔不能小于 1",
            Self::InvalidWindowSize => "窗口大小不能小于 1",
            Self::InvalidChunkSize => "分组大小不能小于 1",
            Self::IntConversionOutOfRange => "整数转换超出目标类型范围",
            Self::InvalidCharScalar => "非法的字符标量值",
            Self::JoinSelf => "不能 Join 自身",
            Self::Deadlock => "所有任务都在等待",
            Self::TimeoutOutOfRange => "等待时间超出平台范围",
            Self::DateTimeOutOfRange => "日期时间超出可表示范围",
            Self::JoinRoot => "不能 Join 根 Actor",
            Self::KillRoot => "不能终止根 Actor",
        }
    }

    /// 具体信息: 由种类携带的数据生成; 无数据时为空.
    #[must_use]
    pub fn detail(&self) -> String {
        match self {
            Self::TypeMismatch { expected, found } => {
                format!("期望 {expected}, 实际 {found}")
            },
            Self::InferMismatch { expected, found } => {
                format!("期望 {expected}, 实际 {found}")
            },
            Self::NotCallable { found }
            | Self::NotHandler { found }
            | Self::NotPid { found }
            | Self::UnprintableType { found } => {
                format!("类型 {found}")
            },
            Self::InvalidDictKey { found } => {
                format!("键类型 {found}, 可用 i64, u8, u64, char, bool 或 string")
            },
            Self::ArityMismatch { expected, found } => {
                format!("期望 {expected} 个, 实际 {found} 个")
            },
            Self::NotMatchable { found } => format!("类型 {found}, 只可匹配 Data 值"),
            Self::NonExhaustiveMatch { missing } => {
                format!("缺少变体 {}", missing.join(", "))
            },
            Self::DuplicateMatchArm { variant } => variant.clone(),
            Self::UndeterminedType { detail } => {
                format!("{detail}, 请添加类型标注")
            },
            Self::InvalidFormatSpec { spec } => spec.clone(),
            Self::UnusedValue { found } => {
                format!("类型 {found}, 请绑定或显式丢弃")
            },
            Self::UnreachableMatchArm => "通配分支之后不可再有分支".to_string(),
            Self::InvalidPidType { mailboxes } => {
                format!("信箱数 {mailboxes}, 必须在 1 到 16 之间")
            },
            Self::MailboxIndexOutOfRange { index, count } => {
                format!("索引 {index}, 信箱数 {count}")
            },
            Self::ModuleReadFailed { path, message } => {
                format!("{path}: {message}")
            },
            Self::ModulePathEscapesRoot { path }
            | Self::InvalidModulePath { path }
            | Self::DuplicateUse { path }
            | Self::ModuleAlreadyLoaded { path } => path.clone(),
            Self::CyclicImport { chain } => chain.join(" -> "),
            Self::ImportNotExported { module, name } => format!("{module}/{name}"),
            Self::ExportUnknown { name }
            | Self::DuplicateImport { name }
            | Self::DuplicateExport { name }
            | Self::DuplicatePatternField { name }
            | Self::ExportPrivateType { name } => name.clone(),
            Self::MultipleExportStatements { module } => module.clone(),
            Self::AssertionFailed { message }
            | Self::Panicked { message }
            | Self::Unimplemented { message }
            | Self::UnreachableReached { message } => message.clone(),

            _ => String::new(),
        }
    }
}

/// 带位置的错误: 目录条目加出错名字.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KslError {
    /// 目录条目: 决定编号, 标题与具体信息.
    pub kind: ErrorKind,
    /// 出错的名字等上下文; 可为空.
    pub subject: Option<String>,
    /// 出错位置; None 仅属无位置的路径.
    pub span: Option<Span>,
}

impl KslError {
    /// 构造无位置, 无出错名字的错误.
    #[must_use]
    pub fn without_span(kind: ErrorKind) -> Self {
        Self {
            kind,
            subject: None,
            span: None,
        }
    }

    /// 构造无出错名字的错误.
    #[must_use]
    pub fn new(kind: ErrorKind, span: Span) -> Self {
        Self {
            kind,
            subject: None,
            span: Some(span),
        }
    }

    /// 构造带出错名字的错误.
    #[must_use]
    pub fn with_subject(kind: ErrorKind, span: Span, subject: impl Into<String>) -> Self {
        Self {
            kind,
            subject: Some(subject.into()),
            span: Some(span),
        }
    }

    /// 错误编号: `E` 加四位数字, 供报告首行标注.
    #[must_use]
    pub fn code(&self) -> String {
        format!("E{:04}", self.kind.number())
    }

    /// 报告标题: 规范标题, 其后是出错名字与具体信息.
    #[must_use]
    pub fn title(&self) -> String {
        let mut title = self.kind.title().to_string();
        if let Some(subject) = &self.subject {
            title.push(' ');
            title.push('`');
            title.push_str(subject);
            title.push('`');
        }
        let detail = self.kind.detail();
        if !detail.is_empty() {
            title.push_str(": ");
            title.push_str(&detail);
        }
        title
    }
}

impl fmt::Display for KslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "error[{}]: {}", self.code(), self.title())
    }
}

impl std::error::Error for KslError {}

/// 运行时错误.
#[derive(Debug, Clone)]
pub enum RuntimeError {
    /// ksl 级错误: 溢出, 越界, 除零, 非法转换等.
    /// 位置来自控制状态或续延帧携带的源码区间; None 仅属内部错误路径.
    Runtime(KslError),
    /// `@Exit[code]` 终止程序; 不是错误, 由宿主转为退出码.
    Exit(u8),
    /// 标准输出或标准错误已关闭; 由宿主静默结束, 不渲染诊断.
    BrokenPipe,
    /// 检查器或抽象机的担保被打破; 实现缺陷, 不面向用户定位.
    Internal(String),
}

impl RuntimeError {
    /// 构造带位置的 ksl 级错误.
    pub(crate) fn runtime(kind: ErrorKind, span: Option<Span>) -> Self {
        Self::Runtime(KslError {
            kind,
            subject: None,
            span,
        })
    }

    /// 构造内部错误.
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}
