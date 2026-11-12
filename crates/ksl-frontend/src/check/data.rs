//! Data 表, 内置 Data 类型与类型名解析.

use std::collections::HashSet;

use indexmap::IndexMap;
use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, intern, keywords, resolve};

use super::reserved;
use super::types::Type;
use crate::ast::{ExprKind, Spanned, Special, TypeExpr};
use crate::error::{ErrorKind, KslError};

#[derive(Debug, Clone)]
pub struct DataInfo {
    pub variants: IndexMap<Symbol, Vec<Type>>,
}

/// 已声明的 Data 类型表.
///
/// 映射使用持久化结构, 会话克隆只做结构共享.
/// 类型声明顺序由收集阶段的临时表保证; 本表只按名字查找.
#[derive(Debug, Clone)]
pub struct DataTable {
    types: imbl::HashMap<Symbol, DataInfo>,
}

impl DataTable {
    /// 空表.
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: imbl::HashMap::new(),
        }
    }

    /// 按名字查找.
    #[must_use]
    pub fn get(&self, name: &Symbol) -> Option<&DataInfo> {
        self.types.get(name)
    }

    /// 名字是否已声明.
    #[must_use]
    pub fn contains_key(&self, name: &Symbol) -> bool {
        self.types.contains_key(name)
    }

    /// 插入或更新一个类型声明.
    pub fn insert(&mut self, name: Symbol, info: DataInfo) {
        self.types.insert(name, info);
    }

    /// 合并另一张表; 已有名字不覆盖.
    pub fn merge(&mut self, other: &Self) {
        for (name, info) in &other.types {
            if !self.types.contains_key(name) {
                self.types.insert(*name, info.clone());
            }
        }
    }
}

/// 由变体表构造内置 Data 类型; 项顺序即变体顺序.
macro_rules! data_info {
    ($($name:literal => [$($field:expr),*]),* $(,)?) => {
        DataInfo {
            variants: [$((intern($name), vec![$($field),*])),*]
                .into_iter()
                .collect(),
        }
    };
}

impl Default for DataTable {
    fn default() -> Self {
        // 内置 Data 类型与用户 Data 同样可标注与匹配.
        let mut table = Self::new();
        for (name, info) in [
            ("ExitReason", exit_reason()),
            ("FileError", file_error()),
            ("InputError", input_error()),
            ("NumberParseError", number_parse_error()),
            ("Utf8Error", utf8_error()),
            ("SocketError", socket_error()),
            ("TimeError", time_error()),
            ("KeyEvent", key_event()),
            ("RegexError", regex_error()),
            ("CommandInput", command_input()),
            ("CommandOption", command_option()),
            ("CommandStatus", command_status()),
            ("CommandError", command_error()),
        ] {
            table.insert(intern(name), info);
        }
        table
    }
}

fn exit_reason() -> DataInfo {
    data_info! {
        "Halted" => [],
        "Killed" => [],
        "Failed" => [Type::I64, Type::Str],
    }
}

fn file_error() -> DataInfo {
    data_info! {
        "NotFound" => [Type::Str],
        "PermissionDenied" => [Type::Str],
        "AlreadyExists" => [Type::Str],
        "InvalidUtf8" => [Type::Str],
        "NotADirectory" => [Type::Str],
        "IsADirectory" => [Type::Str],
        "DirectoryNotEmpty" => [Type::Str],
        // 句柄已关闭或无效; 无路径可报, 故不带载荷.
        "InvalidHandle" => [],
        // 句柄方向不符; 无路径或消息可报.
        "NotReadable" => [],
        "NotWritable" => [],
        "Other" => [Type::Str],
    }
}

fn input_error() -> DataInfo {
    data_info! {
        // 以下变体均不带载荷, 没有路径或消息可报.
        // 标准输入不是终端.
        "NotATerminal" => [],
        // 用户中断当前行读取.
        "Interrupted" => [],
        // 原始模式下调用了需要行规程的读取.
        "RawModeActive" => [],
        // 原始模式已由其他持有者占用.
        "RawModeHeld" => [],
        // 当前 Actor 不是原始模式持有者.
        "NotRawModeHolder" => [],
        // 读取因原始模式持有被释放而中止.
        "RawModeReleased" => [],
        "Other" => [Type::Str],
    }
}

fn number_parse_error() -> DataInfo {
    data_info! {
        "Empty" => [],
        "Invalid" => [],
        "OutOfRange" => [],
    }
}

fn utf8_error() -> DataInfo {
    data_info! {
        "Invalid" => [Type::U64],
        "Incomplete" => [Type::U64],
    }
}

fn socket_error() -> DataInfo {
    data_info! {
        "TimedOut" => [],
        "ConnectionRefused" => [],
        "ConnectionReset" => [],
        "ConnectionClosed" => [],
        "Closed" => [],
        "AddrInUse" => [],
        "AddrNotAvailable" => [],
        "PermissionDenied" => [],
        "InvalidUtf8" => [],
        "LineTooLong" => [],
        "TooLarge" => [],
        "UnexpectedEof" => [],
        "InvalidArgument" => [Type::Str],
        "Other" => [Type::Str],
    }
}

fn time_error() -> DataInfo {
    data_info! {
        "InvalidFormat" => [Type::Str],
        "UnknownZone" => [Type::Str],
        "InvalidField" => [Type::Str],
    }
}

fn key_event() -> DataInfo {
    data_info! {
        // 修饰键折进变体, F 键带编号.
        "Char" => [Type::Char],
        "Ctrl" => [Type::Char],
        "Alt" => [Type::Char],
        "F" => [Type::U8],
        "Enter" => [],
        "Escape" => [],
        "Backspace" => [],
        "Tab" => [],
        "BackTab" => [],
        "Up" => [],
        "Down" => [],
        "Left" => [],
        "Right" => [],
        "Home" => [],
        "End" => [],
        "PageUp" => [],
        "PageDown" => [],
        "Delete" => [],
        "Insert" => [],
    }
}

fn regex_error() -> DataInfo {
    data_info! {
        "Invalid" => [Type::U64],
        "UnclosedGroup" => [Type::U64],
        "UnopenedGroup" => [Type::U64],
        "UnclosedClass" => [Type::U64],
        "InvalidClass" => [Type::U64],
        "InvalidEscape" => [Type::U64],
        "InvalidNumber" => [Type::U64],
        "InvalidRepeat" => [Type::U64],
        "InvalidName" => [Type::U64],
        "DuplicateName" => [Type::U64],
        "InvalidFlag" => [Type::U64],
        "InvalidUnicode" => [Type::U64],
        "UnsupportedFeature" => [Type::U64],
        "TooComplex" => [Type::U64],
        // 编译体积超限针对整个模式, 没有位置可报.
        "TooLarge" => [],
    }
}

fn command_input() -> DataInfo {
    data_info! {
        "Text" => [Type::Str],
        "Binary" => [Type::list(Type::U8)],
    }
}

fn command_option() -> DataInfo {
    data_info! {
        "Cwd" => [Type::Str],
        "Env" => [Type::list(Type::pair(Type::Str, Type::Str))],
        "Input" => [Type::data(intern("CommandInput"))],
    }
}

fn command_status() -> DataInfo {
    data_info! {
        "Exit" => [Type::U64],
        "Signal" => [Type::U64],
    }
}

fn command_error() -> DataInfo {
    data_info! {
        "NotFound" => [Type::Str],
        "PermissionDenied" => [Type::Str],
        "InvalidCwd" => [Type::Str],
        "DuplicateOption" => [Type::Str],
        "DuplicateEnv" => [Type::Str],
        "OutputTooLarge" => [Type::Str],
        "Other" => [Type::Str],
    }
}

/// 先收集并校验全部 `@Data` 声明; 字段类型在名字表齐全后统一解析.
///
/// # Errors
///
/// 声明重复, 变体重复或字段类型非法时返回首个错误.
pub fn collect_data<S>(
    program: &crate::ast::Program,
    table: &mut DataTable,
    opaque_types: &std::collections::HashSet<Symbol, S>,
) -> Result<(), KslError>
where
    S: std::hash::BuildHasher,
{
    // 第一趟: 登记类型名与变体, 字段保留为未解析的 TypeExpr.
    // 有序表保留声明顺序, 使第二趟的首个错误可复现.
    let mut raw: IndexMap<Symbol, IndexMap<Symbol, (Span, Vec<TypeExpr>)>> = IndexMap::new();
    for stmt in program.exprs() {
        let ExprKind::Special(Special::Data { name, variants }) = &stmt.node else {
            continue;
        };
        if reserved::is_reserved(name.node) {
            return Err(KslError::with_subject(
                ErrorKind::ReservedName,
                name.span,
                resolve(name.node).to_string(),
            ));
        }
        if table.contains_key(&name.node) || raw.contains_key(&name.node) {
            return Err(KslError::with_subject(
                ErrorKind::DuplicateData,
                name.span,
                resolve(name.node).to_string(),
            ));
        }
        if variants.is_empty() {
            return Err(KslError::with_subject(
                ErrorKind::EmptyData,
                name.span,
                resolve(name.node).to_string(),
            ));
        }
        let mut variant_map = IndexMap::new();
        for variant in variants {
            if variant.name.node == name.node {
                return Err(KslError::new(ErrorKind::VariantNameConflict, variant.name.span));
            }
            if reserved::is_reserved(variant.name.node) {
                return Err(KslError::with_subject(
                    ErrorKind::ReservedName,
                    variant.name.span,
                    resolve(variant.name.node).to_string(),
                ));
            }
            if variant_map.contains_key(&variant.name.node) {
                return Err(KslError::with_subject(
                    ErrorKind::DuplicateVariant,
                    variant.name.span,
                    resolve(variant.name.node).to_string(),
                ));
            }
            variant_map.insert(variant.name.node, (variant.name.span, variant.fields.clone()));
        }
        raw.insert(name.node, variant_map);
    }
    // 第二趟: 在完整名字表上解析字段类型.
    let names: HashSet<Symbol> =
        table.types.keys().copied().chain(raw.keys().copied()).collect();
    for (type_name, variants) in &raw {
        let mut resolved = IndexMap::new();
        for (variant_name, (_span, fields)) in variants {
            let mut field_types = Vec::new();
            for field in fields {
                field_types.push(resolve_type(
                    field,
                    &|name| names.contains(&name),
                    &|name| opaque_types.contains(&name),
                )?);
            }
            resolved.insert(*variant_name, field_types);
        }
        table.insert(*type_name, DataInfo { variants: resolved });
    }
    Ok(())
}

/// 校验 Dict/Set 的键类型; 与运行时 Key 的取值范围一致.
pub(super) fn ensure_dict_key(ty: &Type, span: Span) -> Result<(), KslError> {
    if matches!(
        ty,
        Type::I64 | Type::U8 | Type::U64 | Type::Char | Type::Bool | Type::Str
    ) {
        Ok(())
    } else {
        Err(KslError::new(
            ErrorKind::InvalidKeyType { found: ty.clone() },
            span,
        ))
    }
}

/// 把类型表达式解析为具体类型; 名字来源为内置类型, Data 类型与不透明类型.
pub(super) fn resolve_type(
    ty: &TypeExpr,
    is_data: &impl Fn(Symbol) -> bool,
    is_opaque: &impl Fn(Symbol) -> bool,
) -> Result<Type, KslError> {
    match ty {
        TypeExpr::Named(name) => resolve_app(name, Vec::new(), is_data, is_opaque),
        TypeExpr::Generic { name, args } => {
            if args.is_empty() {
                return Err(unknown_type(name));
            }
            let args = args
                .iter()
                .map(|arg| resolve_type(arg, is_data, is_opaque))
                .collect::<Result<Vec<_>, _>>()?;
            resolve_app(name, args, is_data, is_opaque)
        },
        TypeExpr::Fn { params, ret, .. } => {
            let params = params
                .iter()
                .map(|param| resolve_type(param, is_data, is_opaque))
                .collect::<Result<Vec<_>, _>>()?;
            let ret = resolve_type(ret, is_data, is_opaque)?;
            Ok(Type::Fn(params, Box::new(ret)))
        },
    }
}

/// 解析一次类型应用: 具名类型视为零参数应用.
fn resolve_app(
    name: &Spanned<Symbol>,
    args: Vec<Type>,
    is_data: &impl Fn(Symbol) -> bool,
    is_opaque: &impl Fn(Symbol) -> bool,
) -> Result<Type, KslError> {
    let kw = keywords();
    let node = name.node;
    if args.is_empty() {
        let ty = if node == kw.i64_ {
            Type::I64
        } else if node == kw.u8_ {
            Type::U8
        } else if node == kw.u64_ {
            Type::U64
        } else if node == kw.f64_ {
            Type::F64
        } else if node == kw.char_ {
            Type::Char
        } else if node == kw.bool_ {
            Type::Bool
        } else if node == kw.string_ {
            Type::Str
        } else if node == kw.nil_ {
            Type::Nil
        } else if node == keywords().file_ {
            Type::file()
        } else if node == keywords().date_time_ {
            Type::date_time()
        } else if node == keywords().socket_ {
            Type::socket()
        } else if node == keywords().tcp_listener_ {
            Type::tcp_listener()
        } else if node == keywords().regex_ {
            Type::regex()
        } else if node == keywords().regex_match_ {
            Type::regex_match()
        } else if node == keywords().command_output_ {
            Type::command_output()
        } else if is_data(node) {
            Type::data(node)
        } else if is_opaque(node) {
            Type::opaque(node)
        } else {
            return Err(unknown_type(name));
        };
        return Ok(ty);
    }
    if is_data(node) {
        // 用户 Data 不带类型参数.
        return Err(unknown_type(name));
    }
    if node == kw.pid_ {
        // 最后一个参数是返回类型, 前面是各信箱的消息类型.
        if !(2..=17).contains(&args.len()) {
            return Err(KslError::new(
                ErrorKind::InvalidPidType {
                    mailboxes: args.len().saturating_sub(1),
                },
                name.span,
            ));
        }
        let mut args = args;
        let ret = args.pop().expect("Pid 至少两个参数");
        return Ok(Type::pid(args, ret));
    }
    if node == kw.handler_ {
        let mut args = args.into_iter();
        let Some(pid) = args.next() else {
            return Err(KslError::new(ErrorKind::InvalidHandlerType, name.span));
        };
        if !pid.is_pid() {
            return Err(KslError::new(ErrorKind::InvalidHandlerType, name.span));
        }
        return Ok(Type::handler(pid, args.collect()));
    }
    if node == keywords().dict_ {
        let [key, value] = args.as_slice() else {
            return Err(type_arity(name.span, 2, args.len()));
        };
        ensure_dict_key(key, name.span)?;
        return Ok(Type::dict(key.clone(), value.clone()));
    }
    if node == keywords().set_ {
        let [inner] = args.as_slice() else {
            return Err(type_arity(name.span, 1, args.len()));
        };
        ensure_dict_key(inner, name.span)?;
        return Ok(Type::set(inner.clone()));
    }
    match args.as_slice() {
        [inner] if node == kw.list_ => Ok(Type::list(inner.clone())),
        [a, b] if node == kw.pair_ => Ok(Type::pair(a.clone(), b.clone())),
        [inner] if node == kw.option_ => Ok(Type::option(inner.clone())),
        [ok, err] if node == kw.result_ => Ok(Type::result(ok.clone(), err.clone())),
        _ if node == kw.list_ || node == kw.option_ => {
            Err(type_arity(name.span, 1, args.len()))
        },
        _ if node == kw.pair_ || node == kw.result_ => {
            Err(type_arity(name.span, 2, args.len()))
        },
        _ => Err(unknown_type(name)),
    }
}

/// 类型构造器实参个数不符.
fn type_arity(span: Span, expected: usize, found: usize) -> KslError {
    KslError::new(ErrorKind::ArityMismatch { expected, found }, span)
}

fn unknown_type(name: &Spanned<Symbol>) -> KslError {
    KslError::with_subject(
        ErrorKind::UnknownType,
        name.span,
        resolve(name.node).to_string(),
    )
}

/// 校验名字可绑定: 内置名一律保留.
pub(super) fn check_bindable(name: &Spanned<Symbol>) -> Result<(), KslError> {
    if reserved::is_reserved(name.node) {
        return Err(KslError::with_subject(
            ErrorKind::ReservedName,
            name.span,
            resolve(name.node).to_string(),
        ));
    }
    Ok(())
}
