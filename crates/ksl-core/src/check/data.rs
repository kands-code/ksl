//! Data 表, 类型解析与操作元数据.

use std::collections::HashSet;

use indexmap::IndexMap;

use super::display::is_printable;
use super::reserved;
use super::types::Type;
use crate::ast::{ExprKind, Spanned, Special, TypeExpr};
use crate::error::{ErrorKind, KslError};
use crate::fmtstr;
use crate::source::Span;
use crate::symbol::{Symbol, intern, keywords, resolve};

#[derive(Debug, Clone)]
pub(crate) struct DataInfo {
    pub(crate) variants: IndexMap<Symbol, Vec<Type>>,
}

/// 已声明的 Data 类型表; 保持声明顺序, 使 Data 阶段的首个错误选择与后续枚举稳定.
#[derive(Debug, Clone)]
pub(crate) struct DataTable {
    pub(crate) types: IndexMap<Symbol, DataInfo>,
}

impl Default for DataTable {
    fn default() -> Self {
        // 内置 Data 类型与用户 Data 同样可标注与匹配.
        let mut variants = IndexMap::new();
        variants.insert(intern("Halted"), Vec::new());
        variants.insert(intern("Killed"), Vec::new());
        variants.insert(intern("Failed"), vec![Type::I64, Type::Str]);
        let mut types = IndexMap::new();
        types.insert(intern("ExitReason"), DataInfo { variants });
        let mut io_variants = IndexMap::new();
        io_variants.insert(intern("NotFound"), vec![Type::Str]);
        io_variants.insert(intern("PermissionDenied"), vec![Type::Str]);
        io_variants.insert(intern("AlreadyExists"), vec![Type::Str]);
        io_variants.insert(intern("InvalidUtf8"), vec![Type::Str]);
        io_variants.insert(intern("NotADirectory"), vec![Type::Str]);
        io_variants.insert(intern("IsADirectory"), vec![Type::Str]);
        io_variants.insert(intern("DirectoryNotEmpty"), vec![Type::Str]);
        // 句柄已关闭或无效; 无路径可报, 故不带载荷.
        io_variants.insert(intern("InvalidHandle"), Vec::new());
        // 句柄方向不符; 与 InvalidHandle 一样没有路径或消息可报.
        io_variants.insert(intern("NotReadable"), Vec::new());
        io_variants.insert(intern("NotWritable"), Vec::new());
        // 标准输入不是终端; 与 InvalidHandle 一样没有路径或消息可报.
        io_variants.insert(intern("NotATerminal"), Vec::new());
        // 用户中断当前行读取; 没有路径或消息可报.
        io_variants.insert(intern("Interrupted"), Vec::new());
        io_variants.insert(intern("Other"), vec![Type::Str]);
        types.insert(
            intern("IOError"),
            DataInfo {
                variants: io_variants,
            },
        );
        types.insert(
            intern("SocketError"),
            DataInfo {
                variants: socket_error_variants(),
            },
        );
        let mut time_variants = IndexMap::new();
        time_variants.insert(intern("InvalidFormat"), vec![Type::Str]);
        time_variants.insert(intern("UnknownZone"), vec![Type::Str]);
        time_variants.insert(intern("InvalidField"), vec![Type::Str]);
        types.insert(
            intern("TimeError"),
            DataInfo {
                variants: time_variants,
            },
        );
        // 按键: 修饰键折进变体, F 键带编号.
        let mut key_variants = IndexMap::new();
        key_variants.insert(intern("Char"), vec![Type::Char]);
        key_variants.insert(intern("Ctrl"), vec![Type::Char]);
        key_variants.insert(intern("Alt"), vec![Type::Char]);
        key_variants.insert(intern("F"), vec![Type::U8]);
        for name in [
            "Enter",
            "Escape",
            "Backspace",
            "Tab",
            "BackTab",
            "Up",
            "Down",
            "Left",
            "Right",
            "Home",
            "End",
            "PageUp",
            "PageDown",
            "Delete",
            "Insert",
        ] {
            key_variants.insert(intern(name), Vec::new());
        }
        types.insert(
            intern("KeyEvent"),
            DataInfo {
                variants: key_variants,
            },
        );
        // 标准 IO 服务的请求协议; 回复句柄在运行时信封中, 不属于请求值.
        let mut io_request_variants = IndexMap::new();
        io_request_variants.insert(intern("Print"), vec![Type::Str]);
        io_request_variants.insert(intern("PrintRaw"), vec![Type::Str]);
        io_request_variants.insert(intern("PrintError"), vec![Type::Str]);
        io_request_variants.insert(intern("ReadLine"), vec![Type::Str]);
        io_request_variants.insert(intern("ReadKey"), Vec::new());
        types.insert(
            intern("IORequest"),
            DataInfo {
                variants: io_request_variants,
            },
        );
        Self { types }
    }
}

/// 套接字操作的错误变体; 与文件 I/O 分开, 便于模式匹配与诊断.
fn socket_error_variants() -> IndexMap<Symbol, Vec<Type>> {
    let mut variants = IndexMap::new();
    for name in [
        "TimedOut",
        "ConnectionRefused",
        "ConnectionReset",
        "ConnectionClosed",
        "Closed",
        "AddrInUse",
        "AddrNotAvailable",
        "PermissionDenied",
        "InvalidUtf8",
        "LineTooLong",
        "TooLarge",
        "UnexpectedEof",
    ] {
        variants.insert(intern(name), Vec::new());
    }
    variants.insert(intern("InvalidArgument"), vec![Type::Str]);
    variants.insert(intern("Other"), vec![Type::Str]);
    variants
}

/// 先收集并校验全部 `@Data` 声明; 字段类型在名字表齐全后统一解析.
pub(crate) fn collect_data(
    program: &crate::ast::Program,
    table: &mut DataTable,
) -> Result<(), KslError> {
    // 第一趟: 登记类型名与变体, 字段保留为未解析的 TypeExpr.
    // 用有序表保留声明顺序, 第二趟首个错误的选择才可复现.
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
        if table.types.contains_key(&name.node) || raw.contains_key(&name.node) {
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
                field_types.push(resolve_type(field, &|name| names.contains(&name))?);
            }
            resolved.insert(*variant_name, field_types);
        }
        table.types.insert(*type_name, DataInfo { variants: resolved });
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
            ErrorKind::InvalidDictKey { found: ty.clone() },
            span,
        ))
    }
}

/// 把类型表达式解析为具体类型; 名字来源为内置类型或已声明的 Data 类型名集合.
pub(super) fn resolve_type(
    ty: &TypeExpr,
    is_data: &impl Fn(Symbol) -> bool,
) -> Result<Type, KslError> {
    match ty {
        TypeExpr::Named(name) => resolve_app(name, Vec::new(), is_data),
        TypeExpr::Generic { name, args } => {
            if args.is_empty() {
                return Err(unknown_type(name));
            }
            let args = args
                .iter()
                .map(|arg| resolve_type(arg, is_data))
                .collect::<Result<Vec<_>, _>>()?;
            resolve_app(name, args, is_data)
        },
        TypeExpr::Fn { params, ret, .. } => {
            let params = params
                .iter()
                .map(|param| resolve_type(param, is_data))
                .collect::<Result<Vec<_>, _>>()?;
            let ret = resolve_type(ret, is_data)?;
            Ok(Type::Fn(params, Box::new(ret)))
        },
    }
}

/// 解析一次类型应用: 具名类型视为零参数应用.
fn resolve_app(
    name: &Spanned<Symbol>,
    args: Vec<Type>,
    is_data: &impl Fn(Symbol) -> bool,
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
        } else if node == intern("File") {
            Type::file()
        } else if node == intern("DateTime") {
            Type::date_time()
        } else if node == intern("Socket") {
            Type::socket()
        } else if node == intern("TcpListener") {
            Type::tcp_listener()
        } else if is_data(node) {
            Type::data(node)
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
    if node == intern("Dict") {
        let [key, value] = args.as_slice() else {
            return Err(KslError::new(
                ErrorKind::InvalidDictKey { found: Type::Nil },
                name.span,
            ));
        };
        ensure_dict_key(key, name.span)?;
        return Ok(Type::dict(key.clone(), value.clone()));
    }
    if node == intern("Set") {
        let [inner] = args.as_slice() else {
            return Err(KslError::new(
                ErrorKind::InvalidDictKey { found: Type::Nil },
                name.span,
            ));
        };
        ensure_dict_key(inner, name.span)?;
        return Ok(Type::set(inner.clone()));
    }
    match args.as_slice() {
        [inner] if node == kw.list_ => Ok(Type::list(inner.clone())),
        [a, b] if node == kw.pair_ => Ok(Type::pair(a.clone(), b.clone())),
        [inner] if node == kw.option_ => Ok(Type::option(inner.clone())),
        [ok, err] if node == kw.result_ => Ok(Type::result(ok.clone(), err.clone())),
        _ => Err(unknown_type(name)),
    }
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

/// 格式化字符串的格式参数校验.
///
/// `?` 仅限 string 与 char; 数值按类型接受对应格式; 其他可打印类型不接受参数;
/// 显式冒号但参数为空一律报错.
pub(super) fn check_format_spec(spec: &str, ty: &Type, span: Span) -> Result<(), KslError> {
    if !is_printable(ty) {
        return Err(KslError::new(
            ErrorKind::UnprintableType { found: ty.clone() },
            span,
        ));
    }
    if spec == "?" {
        if matches!(ty, Type::Str | Type::Char) {
            return Ok(());
        }
        return Err(KslError::with_subject(
            ErrorKind::TypeMismatch {
                expected: Type::Str,
                found: ty.clone(),
            },
            span,
            spec.to_string(),
        ));
    }
    if spec.is_empty() {
        return Err(KslError::new(
            ErrorKind::InvalidFormatSpec {
                spec: spec.to_string(),
            },
            span,
        ));
    }
    let Some(kind) = format_kind(ty) else {
        return Err(KslError::new(
            ErrorKind::InvalidFormatSpec {
                spec: spec.to_string(),
            },
            span,
        ));
    };
    if crate::fmtstr::spec_matches(kind, spec) {
        Ok(())
    } else {
        Err(KslError::new(
            ErrorKind::InvalidFormatSpec {
                spec: spec.to_string(),
            },
            span,
        ))
    }
}

/// 类型对应的格式参数类别; None 表示不接受任何格式参数.
pub(super) fn format_kind(ty: &Type) -> Option<fmtstr::FormatKind> {
    match ty {
        Type::I64 | Type::U8 | Type::U64 => Some(crate::fmtstr::FormatKind::Int),
        Type::F64 => Some(crate::fmtstr::FormatKind::Float),
        Type::Str | Type::Char => Some(crate::fmtstr::FormatKind::Text),
        _ => None,
    }
}
