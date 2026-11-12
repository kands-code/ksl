//! 错误目录: 全部用户可见错误的唯一来源.
//!
//! 词法, 语法, 模块, 类型检查, 运行时五类的错误归入同一枚举.
//! 每种错误有编号与规范标题.
//! 编号分段为 `E01xx` 词法, `E02xx` 语法, `E03xx` 模块, `E04xx` 类型检查, `E05xx` 运行时.
//! 编号由过程宏按段基数与枚举声明顺序生成, 调整顺序即自动重排.
//! 编号只用于报告与编辑器诊断, 不是稳定契约.
//!
//! 内部错误属实现缺陷, 不入目录, 单独渲染.
//! 种类可携带数据, 如期望与实得的类型, 实参个数. 细节由种类生成,
//! 措辞集中在一处.

use std::fmt;

use ksl_base::source::Span;
use ksl_macros::ErrorNumber;

use crate::ast::FailKind;
use crate::check::Type;

/// 错误种类: 目录的唯一条目.
#[derive(Debug, Clone, PartialEq, Eq, ErrorNumber)]
pub enum ErrorKind {
    // 词法
    /// 出现无法构成任何记号的字符, 包括落单的 `+`, `-`.
    #[phase(101)]
    UnexpectedChar,
    /// 字符串, 原始字符串, 格式化字符串或字符字面量未闭合.
    UnterminatedString,
    /// 非法转义序列.
    InvalidEscape,
    /// `\u{...}` 不是合法 Unicode 标量值.
    InvalidUnicodeScalar,
    /// 数字字面量形态非法.
    InvalidNumber,
    /// 数值超出目标类型范围.
    NumberOutOfRange,
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
    // 模块: 路径与读取
    /// 模块文件读取失败.
    #[phase(301)]
    ModuleReadFailed { path: String, message: String },
    /// 模块路径超出根目录.
    ModulePathEscapesRoot { path: String },
    /// 模块路径不是相对路径.
    InvalidModulePath { path: String },
    // 模块: 导入
    /// 同一模块被重复 `@Use`.
    DuplicateUse { path: String },
    /// 模块依赖成环.
    CyclicImport { chain: Vec<String> },
    /// REPL 会话中再次直接加载已加载的模块.
    ModuleAlreadyLoaded { path: String },
    /// 导入的名字未由目标模块导出.
    ImportNotExported { module: String, name: String },
    /// 本地导入名重复或与本地定义冲突.
    DuplicateImport { name: String },
    // 模块: 导出
    /// 导出的名字未在模块中定义.
    ExportUnknown { name: String },
    /// 导出名重复.
    DuplicateExport { name: String },
    /// 同一模块出现多个导出声明.
    MultipleExportStatements { module: String },
    /// 导出声明出现在模块之外, 如交互输入或脚本.
    ExportOutsideModule,
    /// 导出值或类型的签名使用了未导出的 Data 类型.
    ExportPrivateType { name: String },
    // 模块: 模块级形式
    /// 模块尚未加载.
    ModuleFormNotResolved,
    /// `@Main` 出现在模块之外.
    MainOutsideModule,
    /// 同一模块出现多个 `@Main`.
    MultipleMainStatements { module: String },
    /// `@Main` 的参数不是唯一的 Pid self.
    InvalidMainParam,
    /// `@Main` 的返回类型或 Pid 返回类型不是 nil.
    InvalidMainReturn,
    // 模块: Link
    /// `@Link` 出现在模块之外.
    LinkOutsideModule,
    /// Link 名字与已有名字冲突.
    LinkNameConflict { name: String },
    /// `@Export` 列出了本模块 `@Link` 声明的名字.
    LinkExportConflict { name: String },
    // 类型检查: 类型
    /// 期望类型与实际类型不符.
    #[phase(401)]
    TypeMismatch { expected: Type, found: Type },
    /// 推断期类型冲突; 未定类型变量以 `_` 呈现.
    InferMismatch { expected: String, found: String },
    /// 类型不能由值自身确定, 也没有期望给定.
    UndeterminedType { detail: String },
    /// 调用的目标不是函数.
    NotCallable { found: Type },
    /// 实参个数不符.
    ArityMismatch { expected: usize, found: usize },
    // 类型检查: 名字与绑定
    /// 名字未定义, 也不是内置函数或构造器.
    UnknownName,
    /// 未知的类型名.
    UnknownType,
    /// 绑定的名字是内置函数保留名.
    ReservedName,
    /// 函数参数名重复.
    DuplicateParam,
    /// `@Fix` 组内重复定义同名函数.
    DuplicateFixName,
    // 类型检查: 模式与分支
    /// `@Match` 模式字段名重复.
    DuplicatePatternField { name: String },
    /// `@Match` 的对象不是可匹配的 Data 值.
    NotMatchable { found: Type },
    /// `@Match` 未覆盖全部变体.
    NonExhaustiveMatch { missing: Vec<String> },
    /// `@Match` 出现重复变体分支.
    DuplicateMatchArm { variant: String },
    /// `@Match` 的通配分支之后仍有分支, 后者不可达.
    UnreachableMatchArm,
    /// `@Which` 的分支条件是字面量布尔值.
    WhichLiteralCondition { value: bool },
    // 类型检查: 数据类型声明
    /// 重复声明同名的 Data 类型.
    DuplicateData,
    /// Data 类型没有声明任何变体.
    EmptyData,
    /// 同一 Data 类型内重复声明变体.
    DuplicateVariant,
    /// 变体与类型重名.
    VariantNameConflict,
    // 类型检查: Actor 与 Pid
    /// Pid 的信箱数不合法.
    InvalidPidType { mailboxes: usize },
    /// Handler 类型的第一个类型参数不是 Pid.
    InvalidHandlerType,
    /// `@Handler` 的第一个参数不是 Pid.
    InvalidHandlerParam,
    /// `@Spawn` 的 handler 不是 Handler 类型.
    NotHandler { found: Type },
    /// `@Send`/`@Join`/`@Kill` 的实参不是 Pid.
    NotPid { found: Type },
    /// Actor 原语出现在 `@Handler` 或 `@Main` 体外.
    ActorPrimitiveOutsideHandler,
    /// `@Halt` 出现在 `@Main` 体内.
    HaltInMain,
    /// `@Continue` 出现在 `@Main` 体内.
    ContinueInMain,
    /// 信箱索引不是 u8 字面量.
    U8LiteralRequired,
    /// 信箱索引超出 Pid 的信箱数.
    MailboxIndexOutOfRange { index: usize, count: usize },
    // 类型检查: 值与显示
    /// 顶层语句产生了未被绑定的非 nil 值.
    UnusedValue { found: Type },
    /// 格式化字符串的格式参数非法.
    InvalidFormatSpec { spec: String },
    /// 类型没有可打印表示.
    UnprintableType { found: Type },
    // 类型检查: 容器
    /// Dict 的键或集合的元素类型不在支持范围内.
    InvalidKeyType { found: Type },
    // 类型检查: 插件
    /// `@Link` 声明的类型不在插件 ABI 支持范围内.
    PluginUnsupportedSignature { detail: String },
    // 运行时: 用户主动中断
    /// 断言失败.
    #[phase(501)]
    AssertionFailed { message: String },
    /// 主动中止.
    Panicked { message: String },
    /// 尚未实现.
    Unimplemented { message: String },
    /// 不该到达的分支.
    UnreachableReached { message: String },
    // 运行时: 数值与算术
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
    // 运行时: 容器与索引
    /// 列表下标越界.
    IndexOutOfRange,
    /// 计数超出可表示范围.
    CountOutOfRange,
    /// 空列表没有首元素.
    EmptyListHead,
    /// 插入间隔小于 1.
    InvalidInterval,
    /// 窗口大小小于 1.
    InvalidWindowSize,
    /// 分组大小小于 1.
    InvalidChunkSize,
    // 运行时: 调度与等待
    /// 尝试 Join 自身.
    JoinSelf,
    /// 尝试 Join 根 Actor.
    JoinRoot,
    /// 尝试终止根 Actor.
    KillRoot,
    /// 检测到死锁.
    Deadlock,
    /// 等待时间超出平台计时器可表示范围.
    TimeoutOutOfRange,
    // 运行时: 时间与日期
    /// 日期时间超出可表示范围.
    DateTimeOutOfRange,
    // 运行时: 插件
    /// 动态库加载失败.
    PluginLoadFailed { message: String },
    /// 动态库中缺少符号.
    PluginSymbolMissing { name: String },
    /// 外来函数调用失败.
    PluginCallFailed { message: String },
    /// 插件 ABI 版本不符.
    PluginAbiMismatch { expected: u32, found: u32 },
    /// 插件名与描述符不符.
    PluginNameMismatch { expected: String, found: String },
    /// 描述符缺少 `@Link` 声明的导出.
    PluginMissingExport { name: String },
    /// 插件导出与 `@Link` 声明的类型不符.
    PluginSignatureMismatch { name: String },
    /// 插件名不合法.
    PluginNameInvalid { name: String },
    /// 描述符存在重复导出.
    PluginDuplicateExport { name: String },
    /// 描述符存在重复类型.
    PluginDuplicateType { name: String },
    /// 插件初始化入口失败.
    PluginInitFailed { message: String },
}

impl ErrorKind {
    /// 用户主动中断的类别转错误条目; 信息字符串原样作为详情.
    ///
    /// 四个原语共用一条失败通道, 目录中各自独立.
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
    #[allow(clippy::too_many_lines)]
    pub fn title(&self) -> &'static str {
        match self {
            // 词法
            Self::UnexpectedChar => "非法字符",
            Self::UnterminatedString => "字面量未闭合",
            Self::InvalidEscape => "非法转义序列",
            Self::InvalidUnicodeScalar => "非法的 Unicode 标量值",
            Self::InvalidNumber => "数字字面量形态非法",
            Self::NumberOutOfRange => "数值超出类型范围",
            Self::EmptyFStrInterp => "格式化字符串的插值为空",
            // 语法
            Self::UnexpectedToken => "记号与语法预期不符",
            Self::Unterminated => "输入在结构中途结束",
            Self::UnknownSpecialForm => "未知的特殊形式",
            Self::InvalidFStrInterp => "格式化字符串插值非法",
            Self::DataNotTopLevel => "数据声明不在顶层",
            Self::DefinitionNotAllowed => "定义只能位于顶层或 @Block 语句位置",
            Self::ExpectedDef => "此处只接受 @Def",
            Self::ExitNotTopLevel => "@Exit 只能出现在顶层或 @Main 体内",
            Self::ModuleFormNotTopLevel => "模块声明只能出现在顶层",
            // 模块
            Self::ModuleReadFailed { .. } => "模块读取失败",
            Self::ModulePathEscapesRoot { .. } => "模块路径超出根目录",
            Self::InvalidModulePath { .. } => "模块路径必须是相对路径",
            Self::DuplicateUse { .. } => "重复 @Use 同一模块",
            Self::CyclicImport { .. } => "模块依赖成环",
            Self::ModuleAlreadyLoaded { .. } => "模块已加载",
            Self::ImportNotExported { .. } => "导入的名字未导出",
            Self::DuplicateImport { .. } => "本地导入名重复",
            Self::ExportUnknown { .. } => "导出的名字未定义",
            Self::DuplicateExport { .. } => "导出名重复",
            Self::MultipleExportStatements { .. } => "同一模块只允许一个导出声明",
            Self::ExportOutsideModule => "导出声明只允许出现在模块中",
            Self::ExportPrivateType { .. } => "导出签名使用了未导出的类型",
            Self::ModuleFormNotResolved => "模块尚未加载",
            Self::MainOutsideModule => "@Main 只允许出现在模块中",
            Self::MultipleMainStatements { .. } => "同一模块只允许一个 @Main",
            Self::InvalidMainParam => "@Main 必须只有一个 Pid self 参数",
            Self::InvalidMainReturn => "@Main 的返回类型必须是 nil",
            Self::LinkOutsideModule => "@Link 只允许出现在模块中",
            Self::LinkNameConflict { .. } => "Link 名字冲突",
            Self::LinkExportConflict { .. } => "@Export 不能列出 @Link 声明的名字",
            // 类型检查
            Self::TypeMismatch { .. } => "类型不符",
            Self::InferMismatch { .. } => "推断类型不符",
            Self::UndeterminedType { .. } => "类型未确定",
            Self::NotCallable { .. } => "值不可调用",
            Self::ArityMismatch { .. } => "实参个数不符",
            Self::UnknownName => "未定义的名字",
            Self::UnknownType => "未知的类型名",
            Self::ReservedName => "名字为保留名",
            Self::DuplicateParam => "函数参数名重复",
            Self::DuplicatePatternField { .. } => "模式字段名重复",
            Self::NotMatchable { .. } => "值不支持 @Match",
            Self::NonExhaustiveMatch { .. } => "@Match 未覆盖全部变体",
            Self::DuplicateMatchArm { .. } => "@Match 存在重复分支",
            Self::UnreachableMatchArm => "@Match 存在不可达分支",
            Self::WhichLiteralCondition { .. } => "@Which 的字面量条件",
            Self::DuplicateData => "重复声明了同名的 Data 类型",
            Self::EmptyData => "Data 类型没有变体",
            Self::DuplicateVariant => "同一 Data 类型内重复声明了变体",
            Self::VariantNameConflict => "变体不得与类型重名",
            Self::DuplicateFixName => "@Fix 组内名字重复",
            Self::InvalidPidType { .. } => "Pid 的信箱数不合法",
            Self::InvalidHandlerType => "Handler 类型的第一个类型参数必须是 Pid",
            Self::InvalidHandlerParam => "@Handler 的第一个参数必须是 Pid",
            Self::NotHandler { .. } => "值不是 Handler",
            Self::NotPid { .. } => "值不是 Pid",
            Self::ActorPrimitiveOutsideHandler => "Actor 原语只能出现在 @Handler 或 @Main 体内",
            Self::HaltInMain => "@Halt 不能出现在 @Main 体内",
            Self::ContinueInMain => "@Continue 不能出现在 @Main 体内",
            Self::U8LiteralRequired => "需要 u8 字面量",
            Self::MailboxIndexOutOfRange { .. } => "信箱索引越界",
            Self::UnusedValue { .. } => "语句的值未被使用",
            Self::InvalidFormatSpec { .. } => "非法的格式参数",
            Self::UnprintableType { .. } => "类型不可打印",
            Self::InvalidKeyType { .. } => "键或集合元素类型不受支持",
            // 运行时
            Self::AssertionFailed { .. } => "断言失败",
            Self::Panicked { .. } => "主动中止",
            Self::Unimplemented { .. } => "尚未实现",
            Self::UnreachableReached { .. } => "不可达分支被执行",
            Self::IntegerOverflow => "整数运算溢出",
            Self::U8Overflow => "u8 运算溢出",
            Self::U64Overflow => "u64 运算溢出",
            Self::FloatOutOfRange => "浮点值超出整数范围",
            Self::IntConversionOutOfRange => "整数转换超出目标类型范围",
            Self::DivisionByZero => "除数为零",
            Self::InvalidCharScalar => "非法字符标量",
            Self::IndexOutOfRange => "列表下标越界",
            Self::CountOutOfRange => "计数超出范围",
            Self::EmptyListHead => "空列表没有首元素",
            Self::InvalidInterval => "插入间隔至少为 1",
            Self::InvalidWindowSize => "窗口大小至少为 1",
            Self::InvalidChunkSize => "分组大小至少为 1",
            Self::JoinSelf => "不能 Join 自身",
            Self::Deadlock => "检测到死锁",
            Self::TimeoutOutOfRange => "等待时间超出平台范围",
            Self::JoinRoot => "不能 Join 根 Actor",
            Self::KillRoot => "不能终止根 Actor",
            Self::DateTimeOutOfRange => "日期时间超出范围",
            Self::PluginLoadFailed { .. } => "动态库加载失败",
            Self::PluginSymbolMissing { .. } => "动态库缺少符号",
            Self::PluginUnsupportedSignature { .. } => "外来函数签名不受支持",
            Self::PluginCallFailed { .. } => "外来函数调用失败",
            Self::PluginAbiMismatch { .. } => "插件 ABI 版本不符",
            Self::PluginNameMismatch { .. } => "插件名不符",
            Self::PluginMissingExport { .. } => "插件缺少导出的名字",
            Self::PluginSignatureMismatch { .. } => "插件导出签名不符",
            Self::PluginNameInvalid { .. } => "插件名不合法",
            Self::PluginDuplicateExport { .. } => "插件导出名重复",
            Self::PluginDuplicateType { .. } => "插件类型名重复",
            Self::PluginInitFailed { .. } => "插件初始化失败",
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
                format!("{expected} 与 {found} 不一致")
            },
            Self::NotCallable { found }
            | Self::NotHandler { found }
            | Self::NotPid { found }
            | Self::UnprintableType { found }
            | Self::UnusedValue { found } => {
                format!("类型 {found}")
            },
            Self::InvalidKeyType { found } => {
                format!("类型 {found}, 可用 i64, u8, u64, char, bool 或 string")
            },
            Self::ArityMismatch { expected, found } => {
                format!("期望 {expected} 个, 实际 {found} 个")
            },
            Self::NotMatchable { found } => format!("类型 {found}, 只能匹配 Data 值"),
            Self::NonExhaustiveMatch { missing } => {
                format!("缺少变体 {}", missing.join(", "))
            },
            Self::DuplicateMatchArm { variant } => variant.clone(),
            Self::UndeterminedType { detail } => detail.clone(),
            Self::InvalidFormatSpec { spec } => spec.clone(),
            Self::UnreachableMatchArm => "通配分支后存在分支".to_string(),
            Self::WhichLiteralCondition { value: true } => {
                "条件恒为 true, 后续分支与 fallback 不可达".to_string()
            },
            Self::WhichLiteralCondition { value: false } => {
                "条件恒为 false, 分支不可达".to_string()
            },
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
            | Self::ExportPrivateType { name }
            | Self::LinkNameConflict { name }
            | Self::LinkExportConflict { name }
            | Self::PluginSymbolMissing { name }
            | Self::PluginUnsupportedSignature { detail: name }
            | Self::PluginMissingExport { name }
            | Self::PluginSignatureMismatch { name }
            | Self::PluginNameInvalid { name }
            | Self::PluginDuplicateExport { name }
            | Self::PluginDuplicateType { name } => name.clone(),
            Self::PluginAbiMismatch { expected, found } => {
                format!("期望 {expected}, 实际 {found}")
            },
            Self::PluginNameMismatch { expected, found } => {
                format!("期望 {expected}, 实际 {found}")
            },
            Self::MultipleExportStatements { module } => module.clone(),
            Self::AssertionFailed { message }
            | Self::Panicked { message }
            | Self::Unimplemented { message }
            | Self::UnreachableReached { message }
            | Self::PluginLoadFailed { message }
            | Self::PluginCallFailed { message }
            | Self::PluginInitFailed { message } => message.clone(),
            _ => String::new(),
        }
    }

    /// 诊断附注: 只放可操作建议, 不重复标题与具体信息.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        match self {
            Self::UndeterminedType { .. } => Some("可添加类型标注".to_string()),
            Self::UnusedValue { .. } => Some("绑定到名字或显式丢弃".to_string()),
            Self::NonExhaustiveMatch { .. } => Some("补全缺失分支或添加通配分支".to_string()),
            _ => None,
        }
    }
}

/// 语言级错误: 目录条目加可选位置与出错名字.
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
    #[must_use]
    pub fn runtime(kind: ErrorKind, span: Option<Span>) -> Self {
        Self::Runtime(KslError {
            kind,
            subject: None,
            span,
        })
    }

    /// 构造内部错误.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }
}
