//! 错误标题, 详情与附注的渲染.

use ksl_base::display::{TypeFormatter, TypeNames};
use ksl_base::fail::FailKind;
use ksl_base::types::Type;

use super::kind::ErrorKind;

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
    // 错误标题按类别列出的固定表.
    #[must_use]
    #[expect(clippy::too_many_lines)]
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
            Self::NestingTooDeep { .. } => "嵌套层数超出上限",
            Self::UnknownSpecialForm => "未知的特殊形式",
            Self::InvalidFStrInterp => "格式化字符串插值非法",
            Self::DataNotTopLevel => "数据声明不在顶层",
            Self::DefinitionNotAllowed => "定义只能位于顶层或 @Block 语句位置",
            Self::ExpectedDef => "此处只接受 @Def",
            Self::ExitOutsideRoot => "@Exit 只能出现在程序根或 @Main 体内",
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
            Self::ModuleDirectoryOutsideModule => "GetModuleDirectory 只允许出现在模块中",
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
            Self::DuplicateFixName => "@Fix 组内名字重复",
            Self::NotMatchable { .. } => "值不支持 @Match",
            Self::DuplicatePatternField { .. } => "模式字段名重复",
            Self::NonExhaustiveMatch { .. } => "@Match 未覆盖全部变体",
            Self::DuplicateMatchArm { .. } => "@Match 存在重复分支",
            Self::UnreachableMatchArm => "@Match 存在不可达分支",
            Self::WhichLiteralCondition { .. } => "@Which 的字面量条件",
            Self::NotData { .. } => "@DataGet 的对象不是 Data",
            Self::TypeNotVisible => "类型不可见",
            Self::DataGetOnMultiVariant { .. } => "多变体 Data 不支持字段访问",
            Self::FieldIndexOutOfRange { .. } => "字段下标越界",
            Self::DuplicateData => "重复声明了同名的 Data 类型",
            Self::EmptyData => "Data 类型没有变体",
            Self::DuplicateVariant => "同一 Data 类型内重复声明了变体",
            Self::VariantNameConflict => "变体不得与类型重名",
            Self::TooManyVariantFields { .. } => "Data 变体字段数超限",
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
            Self::InvalidKeyType { .. } => "Dict 键或 Set 元素类型不受支持",
            // 运行时
            Self::AssertionFailed { .. } => "断言失败",
            Self::Panicked { .. } => "主动中止",
            Self::Unimplemented { .. } => "尚未实现",
            Self::UnreachableReached { .. } => "不可达分支被执行",
            Self::IntegerOverflow => "i64 运算溢出",
            Self::I32Overflow => "i32 运算溢出",
            Self::U8Overflow => "u8 运算溢出",
            Self::U32Overflow => "u32 运算溢出",
            Self::U64Overflow => "u64 运算溢出",
            Self::FloatOutOfRange { .. } => "浮点值超出整数范围",
            Self::IntConversionOutOfRange { .. } => "整数转换超出目标类型范围",
            Self::DivisionByZero => "除数为零",
            Self::InvalidCharScalar => "非法 char 标量值",
            Self::IndexOutOfRange => "List 下标越界",
            Self::CountOutOfRange => "计数超出范围",
            Self::EmptyListHead => "空 List 没有首元素",
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
    ///
    /// `subject` 已负责显示出错名字或路径, 这里不重复.
    #[must_use]
    pub fn detail(&self, names: Option<&TypeNames>) -> String {
        match self {
            Self::TypeMismatch { expected, found } => {
                format!(
                    "期望 {}, 实际 {}",
                    type_text(expected, names),
                    type_text(found, names)
                )
            },
            Self::FloatOutOfRange { source, target }
            | Self::IntConversionOutOfRange { source, target } => {
                format!(
                    "{} 值超出 {} 范围",
                    type_text(source, names),
                    type_text(target, names)
                )
            },
            Self::InferMismatch { expected, found } => {
                format!("{expected} 与 {found} 不一致")
            },
            Self::NotCallable { found }
            | Self::NotHandler { found }
            | Self::NotPid { found }
            | Self::UnprintableType { found }
            | Self::UnusedValue { found } => {
                format!("类型 {}", type_text(found, names))
            },
            Self::InvalidKeyType { found } => {
                format!(
                    "类型 {}, 仅支持 i64, i32, u8, u32, u64, char, bool 或 string",
                    type_text(found, names)
                )
            },
            Self::ArityMismatch { expected, found } => {
                format!("期望 {expected} 个, 实际 {found} 个")
            },
            Self::NotMatchable { found } => {
                format!("类型 {}, 只能匹配 Data 值", type_text(found, names))
            },
            Self::NotData { found } => {
                format!("类型 {}, 只有 Data 支持 @DataGet", type_text(found, names))
            },
            Self::DataGetOnMultiVariant { variants } => {
                format!("Data 有 {variants} 个变体, 只有单变体 Data 支持 @DataGet")
            },
            Self::FieldIndexOutOfRange { index, count } => {
                format!("字段下标 {index}, 变体只有 {count} 个字段")
            },
            Self::TooManyVariantFields { count, limit } => {
                format!("字段数 {count}, 上限 {limit}")
            },
            Self::NonExhaustiveMatch { missing } => {
                format!("缺少变体 {}", missing.join(", "))
            },
            Self::DuplicateMatchArm { variant } => variant.clone(),
            Self::UndeterminedType { detail } | Self::PluginUnsupportedSignature { detail } => {
                detail.clone()
            },
            Self::NestingTooDeep { limit } => format!("上限 {limit} 层"),
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
            Self::CyclicImport { chain } => chain.join(" -> "),
            Self::ImportNotExported { module, .. } => module.clone(),
            Self::PluginSymbolMissing { name }
            | Self::PluginMissingExport { name }
            | Self::PluginSignatureMismatch { name }
            | Self::PluginDuplicateExport { name }
            | Self::PluginDuplicateType { name } => name.clone(),
            Self::PluginAbiMismatch { expected, found } => {
                format!("期望 {expected}, 实际 {found}")
            },
            Self::PluginNameMismatch { expected, found } => {
                format!("期望 {expected}, 实际 {found}")
            },
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
            Self::TypeNotVisible => Some("导入该类型后才能匹配变体或读取字段".to_string()),
            _ => None,
        }
    }
}

/// 按当前类型名显示表渲染类型; 没有显示表时使用真实名.
fn type_text(ty: &Type, names: Option<&TypeNames>) -> String {
    names.map_or_else(
        || ty.to_string(),
        |names| TypeFormatter::new(ty, Some(names)).to_string(),
    )
}
