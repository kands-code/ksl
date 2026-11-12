//! 语言级错误对象与构造器.
//!
//! 错误对象是薄句柄: 内容装箱, `Result<T, KslError>` 只额外占一个指针.
//! 构造只发生在失败路径; 出错名字以 `ArcStr` 共享, 克隆错误时不复制字符串内容.

use std::fmt;
use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::display::TypeNames;
use ksl_base::name::Name;
use ksl_base::source::Span;

use super::kind::ErrorKind;

/// 错误渲染上下文: 出错名字与类型名显示表.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ErrorContext {
    subject: Option<ArcStr>,
    type_names: Option<TypeNames>,
}

/// 错误内容的唯一存储形态.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Repr {
    /// 用户可见错误: 进入错误目录并编号.
    User {
        kind: ErrorKind,
        span: Option<Span>,
        context: Option<Arc<ErrorContext>>,
    },
    /// 实现缺陷: 不编号, 单独渲染.
    Internal { message: String, span: Option<Span> },
}

/// 语言级错误: 用户可见错误或内部错误.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KslError(Box<Repr>);

impl KslError {
    /// 构造无位置, 无出错名字的用户错误.
    #[must_use]
    pub fn without_span(kind: ErrorKind) -> Self {
        Self::with_span(kind, None)
    }

    /// 以给定位置构造用户错误, 不带出错名字; 位置可以为空.
    #[must_use]
    pub fn with_span(kind: ErrorKind, span: Option<Span>) -> Self {
        Self(Box::new(Repr::User {
            kind,
            span,
            context: None,
        }))
    }

    /// 构造无出错名字的用户错误.
    #[must_use]
    pub fn new(kind: ErrorKind, span: Span) -> Self {
        Self::with_span(kind, Some(span))
    }

    /// 构造带出错名字的用户错误.
    #[must_use]
    pub fn with_subject(kind: ErrorKind, span: Span, subject: impl Into<ArcStr>) -> Self {
        Self::with_subject_span(kind, Some(span), subject)
    }

    /// 构造带可选位置与出错名字的用户错误.
    #[must_use]
    pub fn with_subject_span(
        kind: ErrorKind,
        span: Option<Span>,
        subject: impl Into<ArcStr>,
    ) -> Self {
        Self(Box::new(Repr::User {
            kind,
            span,
            context: Some(Arc::new(ErrorContext {
                subject: Some(subject.into()),
                type_names: None,
            })),
        }))
    }

    /// 构造带名字与位置的用户错误.
    #[must_use]
    pub fn with_name(kind: ErrorKind, span: Span, name: &Name) -> Self {
        Self::with_subject(kind, span, name.as_str())
    }

    /// 未定义的名字.
    #[must_use]
    pub fn unknown_name(name: &Name, span: Span) -> Self {
        Self::with_name(ErrorKind::UnknownName, span, name)
    }

    /// 未知的类型名.
    #[must_use]
    pub fn unknown_type(name: &Name, span: Span) -> Self {
        Self::with_name(ErrorKind::UnknownType, span, name)
    }

    /// 值的类型未在当前环境中导入; 显示声明名, 不显示模块路径.
    #[must_use]
    pub fn type_not_visible(identity: &Name, span: Span) -> Self {
        let declared = identity
            .as_str()
            .rsplit('/')
            .next()
            .unwrap_or(identity.as_str());
        Self::with_subject(ErrorKind::TypeNotVisible, span, declared)
    }

    /// 名字是保留名.
    #[must_use]
    pub fn reserved_name(name: &Name, span: Span) -> Self {
        Self::with_name(ErrorKind::ReservedName, span, name)
    }

    /// 本地导入名重复.
    #[must_use]
    pub fn duplicate_import(name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::DuplicateImport {
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// 导入名未由目标模块导出.
    #[must_use]
    pub fn import_not_exported(module: &str, name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::ImportNotExported {
                module: module.to_string(),
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// 导出名重复.
    #[must_use]
    pub fn duplicate_export(name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::DuplicateExport {
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// 导出的名字未定义.
    #[must_use]
    pub fn export_unknown(name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::ExportUnknown {
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// 导出签名使用了未导出的类型.
    #[must_use]
    pub fn export_private_type(name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::ExportPrivateType {
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// Link 名字冲突.
    #[must_use]
    pub fn link_name_conflict(name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::LinkNameConflict {
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// `@Export` 列出了 `@Link` 声明的名字.
    #[must_use]
    pub fn link_export_conflict(name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::LinkExportConflict {
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// `@Match` 模式字段名重复.
    #[must_use]
    pub fn duplicate_pattern_field(name: &Name, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::DuplicatePatternField {
                name: name.as_str().to_string(),
            },
            span,
            name.as_str(),
        )
    }

    /// `@Match` 分支重复.
    #[must_use]
    pub fn duplicate_match_arm(variant: &Name, span: Span) -> Self {
        Self::new(
            ErrorKind::DuplicateMatchArm {
                variant: variant.as_str().to_string(),
            },
            span,
        )
    }

    /// 同一模块被重复 `@Use`.
    #[must_use]
    pub fn duplicate_use(path: &str, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::DuplicateUse {
                path: path.to_string(),
            },
            span,
            path,
        )
    }

    /// 模块已加载.
    #[must_use]
    pub fn module_already_loaded(path: &str, span: Option<Span>) -> Self {
        Self::with_subject_span(
            ErrorKind::ModuleAlreadyLoaded {
                path: path.to_string(),
            },
            span,
            path,
        )
    }

    /// 模块路径非法.
    #[must_use]
    pub fn invalid_module_path(path: &str, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::InvalidModulePath {
                path: path.to_string(),
            },
            span,
            path,
        )
    }

    /// 模块路径越出根目录.
    #[must_use]
    pub fn module_path_escapes_root(path: &str, span: Span) -> Self {
        Self::module_path_escapes_root_span(path, Some(span))
    }

    /// 模块路径越出根目录; 位置可为空.
    #[must_use]
    pub fn module_path_escapes_root_span(path: &str, span: Option<Span>) -> Self {
        Self::with_subject_span(
            ErrorKind::ModulePathEscapesRoot {
                path: path.to_string(),
            },
            span,
            path,
        )
    }

    /// 同一模块出现多个导出声明.
    #[must_use]
    pub fn multiple_export_statements(module: &str, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::MultipleExportStatements {
                module: module.to_string(),
            },
            span,
            module,
        )
    }

    /// 同一模块出现多个 `@Main`.
    #[must_use]
    pub fn multiple_main_statements(module: &str, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::MultipleMainStatements {
                module: module.to_string(),
            },
            span,
            module,
        )
    }

    /// 插件名不合法.
    #[must_use]
    pub fn plugin_name_invalid(name: &str, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::PluginNameInvalid {
                name: name.to_string(),
            },
            span,
            name,
        )
    }

    /// 插件加载失败.
    #[must_use]
    pub fn plugin_load_failed(message: impl Into<String>, name: &str, span: Span) -> Self {
        Self::with_subject(
            ErrorKind::PluginLoadFailed {
                message: message.into(),
            },
            span,
            name,
        )
    }

    /// 外来函数签名无法映射到插件调用约定.
    #[must_use]
    pub fn plugin_unsupported_signature(
        name: &Name,
        detail: impl Into<String>,
        span: Span,
    ) -> Self {
        Self::with_name(
            ErrorKind::PluginUnsupportedSignature {
                detail: detail.into(),
            },
            span,
            name,
        )
    }

    /// 构造内部错误.
    #[must_use]
    pub fn internal(message: impl Into<String>, span: Option<Span>) -> Self {
        Self(Box::new(Repr::Internal {
            message: message.into(),
            span,
        }))
    }

    /// 附加类型名显示表; 用于按当前模块可见名渲染类型.
    #[must_use]
    pub fn with_type_names(self, names: TypeNames) -> Self {
        if names.is_empty() {
            return self;
        }
        let Self(repr) = self;
        match *repr {
            Repr::User {
                kind,
                span,
                context,
            } => {
                let mut context =
                    context.map_or_else(ErrorContext::default, Arc::unwrap_or_clone);
                context.type_names = Some(names);
                Self(Box::new(Repr::User {
                    kind,
                    span,
                    context: Some(Arc::new(context)),
                }))
            },
            Repr::Internal { message, span } => {
                Self(Box::new(Repr::Internal { message, span }))
            },
        }
    }

    /// 用户错误种类; 内部错误返回 None.
    #[must_use]
    pub fn kind(&self) -> Option<&ErrorKind> {
        match &*self.0 {
            Repr::User { kind, .. } => Some(kind),
            Repr::Internal { .. } => None,
        }
    }

    /// 出错位置; 可为空.
    #[must_use]
    pub fn span(&self) -> Option<Span> {
        match &*self.0 {
            Repr::User { span, .. } | Repr::Internal { span, .. } => *span,
        }
    }

    /// 出错名字; 内部错误与无名用户错误返回 None.
    #[must_use]
    pub fn subject(&self) -> Option<&str> {
        match &*self.0 {
            Repr::User { context, .. } => context
                .as_deref()
                .and_then(|context| context.subject.as_deref()),
            Repr::Internal { .. } => None,
        }
    }

    /// 错误编号; 内部错误没有编号.
    #[must_use]
    pub fn number(&self) -> Option<u16> {
        self.kind().map(ErrorKind::number)
    }

    /// 错误编号文本: `E` 加四位数字; 内部错误没有编号.
    #[must_use]
    pub fn code(&self) -> Option<String> {
        self.number().map(|number| format!("E{number:04}"))
    }

    /// 诊断附注; 内部错误没有附注.
    #[must_use]
    pub fn note(&self) -> Option<String> {
        self.kind().and_then(ErrorKind::note)
    }

    /// 报告标题: 用户错误为规范标题, 内部错误为内部错误说明.
    #[must_use]
    pub fn title(&self) -> String {
        match &*self.0 {
            Repr::User { kind, context, .. } => {
                let mut title = kind.title().to_string();
                if let Some(subject) = context
                    .as_deref()
                    .and_then(|context| context.subject.as_deref())
                {
                    title.push(' ');
                    title.push('`');
                    title.push_str(subject);
                    title.push('`');
                }
                let type_names = context
                    .as_deref()
                    .and_then(|context| context.type_names.as_ref());
                let detail = kind.detail(type_names);
                if !detail.is_empty() {
                    title.push_str(": ");
                    title.push_str(&detail);
                }
                title
            },
            Repr::Internal { message, .. } => format!("内部错误: {message}"),
        }
    }
}

impl fmt::Display for KslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.0 {
            Repr::User { kind, .. } => {
                write!(f, "error[E{:04}]: {}", kind.number(), self.title())
            },
            Repr::Internal { message, .. } => write!(f, "内部错误: {message}"),
        }
    }
}

impl std::error::Error for KslError {}
