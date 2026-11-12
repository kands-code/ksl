//! 错误种类.

use ksl_base::types::Type;
use ksl_macros::ErrorNumber;

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
    /// 表达式或类型的嵌套层数超出上限.
    NestingTooDeep { limit: usize },
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
    /// `@Exit` 出现在函数体或 Handler 体内.
    ExitOutsideRoot,
    /// 模块声明出现在非顶层位置.
    ModuleFormNotTopLevel,
    // 模块: 路径, 读取与加载
    /// 模块文件读取失败.
    #[phase(301)]
    ModuleReadFailed { path: String, message: String },
    /// 模块路径超出根目录.
    ModulePathEscapesRoot { path: String },
    /// 模块路径不是相对路径.
    InvalidModulePath { path: String },
    /// 同一模块被重复 `@Use`.
    DuplicateUse { path: String },
    /// 模块依赖成环.
    CyclicImport { chain: Vec<String> },
    /// REPL 会话中再次直接加载已加载的模块.
    ModuleAlreadyLoaded { path: String },
    // 模块: 导入
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
    // 模块: 模块内置
    /// `GetModuleDirectory` 出现在模块之外.
    ModuleDirectoryOutsideModule,
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
    /// `@Match` 的对象不是可匹配的 Data 值.
    NotMatchable { found: Type },
    /// `@Match` 模式字段名重复.
    DuplicatePatternField { name: String },
    /// `@Match` 未覆盖全部变体.
    NonExhaustiveMatch { missing: Vec<String> },
    /// `@Match` 出现重复变体分支.
    DuplicateMatchArm { variant: String },
    /// `@Match` 的通配分支之后仍有分支, 后者不可达.
    UnreachableMatchArm,
    /// `@Which` 的分支条件是字面量布尔值.
    WhichLiteralCondition { value: bool },
    // 类型检查: Data 访问
    /// `@DataGet` 的对象不是 Data 值.
    NotData { found: Type },
    /// 值的类型不在当前环境中可见, 无法读取其变体或字段.
    TypeNotVisible,
    /// `@DataGet` 的对象有多个变体, 字段位置不唯一.
    DataGetOnMultiVariant { variants: usize },
    /// `@DataGet` 的字段下标越界.
    FieldIndexOutOfRange { index: usize, count: usize },
    // 类型检查: 数据类型声明
    /// 重复声明同名的 Data 类型.
    DuplicateData,
    /// Data 类型没有声明任何变体.
    EmptyData,
    /// 同一 Data 类型内重复声明变体.
    DuplicateVariant,
    /// 变体与类型重名.
    VariantNameConflict,
    /// Data 变体字段数超过上限.
    TooManyVariantFields { count: usize, limit: usize },
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
    /// `@Link` 声明的返回类型无法映射到插件调用约定.
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
    /// i32 运算溢出.
    I32Overflow,
    /// u8 运算溢出.
    U8Overflow,
    /// u32 运算溢出.
    U32Overflow,
    /// u64 运算溢出.
    U64Overflow,
    /// 浮点值超出目标整数类型的范围.
    FloatOutOfRange { source: Type, target: Type },
    /// 整数转换超出目标类型范围.
    IntConversionOutOfRange { source: Type, target: Type },
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
