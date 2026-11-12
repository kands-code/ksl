//! 插件导出宏.
//!
//! 属性与生成规则见 `docs/topics/implementation/plugins.md` 第 8 节.
//! 宏只生成入口, 描述符与 C ABI 包装; 插件作者书写普通 Rust 函数与不透明类型.

use std::collections::{HashMap, HashSet};

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Expr, ExprLit, FnArg, GenericArgument, Item, ItemMod, Lit, LitStr, Meta,
    PathArguments, ReturnType, Token, Type, parse_macro_input,
};

/// 展开插件模块.
pub(super) fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as PluginArgs);
    let module = parse_macro_input!(input as ItemMod);
    match expand_plugin(&args, module) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

struct PluginArgs {
    name: String,
    version: String,
}

impl Parse for PluginArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut name = None;
        let mut version = None;
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        for meta in metas {
            match &meta {
                Meta::NameValue(value) if value.path.is_ident("name") => {
                    name = Some(string_value(&value.value)?);
                },
                Meta::NameValue(value) if value.path.is_ident("version") => {
                    version = Some(string_value(&value.value)?);
                },
                other => {
                    return Err(syn::Error::new_spanned(other, "只接受 name 与 version 参数"));
                },
            }
        }
        let name = name.ok_or_else(|| syn::Error::new(input.span(), "缺少 name 参数"))?;
        if name.is_empty() {
            return Err(syn::Error::new(input.span(), "name 不能为空"));
        }
        let version = version.unwrap_or_else(|| {
            std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string())
        });
        Ok(Self { name, version })
    }
}

fn export_name(attr: NameAttr, ident: &syn::Ident) -> Option<String> {
    match attr {
        NameAttr::Absent => None,
        NameAttr::Bare => Some(ident.to_string()),
        NameAttr::Named(name) => Some(name),
    }
}

fn string_value(expr: &Expr) -> syn::Result<String> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(value),
        ..
    }) = expr
    {
        Ok(value.value())
    } else {
        Err(syn::Error::new_spanned(expr, "需要字符串字面量"))
    }
}

/// 导出属性的三种形态.
enum NameAttr {
    Absent,
    Bare,
    Named(String),
}

/// 从属性列表中取出并移除指定属性.
fn take_name_attr(attrs: &mut Vec<Attribute>, name: &str) -> syn::Result<NameAttr> {
    let mut found = NameAttr::Absent;
    let mut index = 0;
    while index < attrs.len() {
        if !attrs[index].path().is_ident(name) {
            index += 1;
            continue;
        }
        let attr = attrs.remove(index);
        let value = match &attr.meta {
            Meta::Path(_) => NameAttr::Bare,
            Meta::List(_) => NameAttr::Named(attr.parse_args::<LitStr>()?.value()),
            Meta::NameValue(_) => {
                return Err(syn::Error::new_spanned(&attr, "不接受等号形式, 直接写字符串"));
            },
        };
        if !matches!(found, NameAttr::Absent) {
            return Err(syn::Error::new_spanned(&attr, "属性重复"));
        }
        found = value;
    }
    Ok(found)
}

/// 函数与全局值的执行类别.
#[derive(Clone, Copy)]
enum Execution {
    Current,
    Io,
    Cpu,
    /// 只用于生命周期入口与事件泵的线程亲和性.
    Main,
}

impl Execution {
    /// 生命周期入口的线程亲和性; I/O 与 CPU 类别不适用.
    fn thread(self, ident: &syn::Ident) -> syn::Result<TokenStream2> {
        match self {
            Self::Current => Ok(quote! { ::ksl_plugin::THREAD_ANY }),
            Self::Main => Ok(quote! { ::ksl_plugin::THREAD_RUNTIME }),
            Self::Io | Self::Cpu => Err(syn::Error::new_spanned(
                ident,
                "生命周期入口只接受默认类别或 main",
            )),
        }
    }

    fn tokens(self) -> TokenStream2 {
        match self {
            Self::Current => quote! { ::ksl_plugin::EXECUTION_CURRENT },
            Self::Io => quote! { ::ksl_plugin::EXECUTION_IO },
            Self::Cpu => quote! { ::ksl_plugin::EXECUTION_CPU },
            Self::Main => {
                unreachable!("main 只用于生命周期入口与事件泵, 不生成函数描述符")
            },
        }
    }
}

/// 取出并移除 `#[ksl_exec(...)]`; 缺省为当前调用.
fn take_execution_attr(attrs: &mut Vec<Attribute>) -> syn::Result<Execution> {
    let mut found = None;
    let mut index = 0;
    while index < attrs.len() {
        if !attrs[index].path().is_ident("ksl_exec") {
            index += 1;
            continue;
        }
        let attr = attrs.remove(index);
        let name = attr.parse_args::<syn::Ident>()?;
        let execution = match name.to_string().as_str() {
            "current" => Execution::Current,
            "io" => Execution::Io,
            "cpu" => Execution::Cpu,
            "main" => Execution::Main,
            other => {
                return Err(syn::Error::new_spanned(
                    &name,
                    format!("未知的执行类别: {other}"),
                ));
            },
        };
        if found.replace(execution).is_some() {
            return Err(syn::Error::new_spanned(attr, "执行类别重复"));
        }
    }
    Ok(found.unwrap_or(Execution::Current))
}

/// 取出并移除无参数标记属性; 重复出现报错.
fn take_flag_attr(attrs: &mut Vec<Attribute>, name: &str) -> syn::Result<bool> {
    let mut found = false;
    let mut index = 0;
    while index < attrs.len() {
        if !attrs[index].path().is_ident(name) {
            index += 1;
            continue;
        }
        let attr = attrs.remove(index);
        if found {
            return Err(syn::Error::new_spanned(attr, "属性重复"));
        }
        found = true;
    }
    Ok(found)
}

/// 值类型在插件 ABI 中的类别.
#[derive(Clone)]
enum ValueType {
    I64,
    U64,
    U8,
    F64,
    Bool,
    Char,
    String,
    Str,
    Nil,
    Opaque(Box<Type>),
    Result(Box<Self>),
}

/// 收集不透明类型并分配类型表下标.
#[derive(Default)]
struct OpaqueTypes {
    types: Vec<Type>,
    indexes: HashMap<String, usize>,
}

impl OpaqueTypes {
    fn index(&mut self, ty: &Type) -> usize {
        let key = quote!(#ty).to_string();
        if let Some(index) = self.indexes.get(&key) {
            return *index;
        }
        let index = self.types.len();
        self.types.push(ty.clone());
        self.indexes.insert(key, index);
        index
    }
}

struct ExportedFunction {
    ident: syn::Ident,
    name: String,
    params: Vec<ValueType>,
    ret: ValueType,
    execution: Execution,
}

struct ExportedGlobal {
    ident: syn::Ident,
    name: String,
    kind: GlobalKind,
    value_type: ValueType,
    execution: Execution,
}

enum GlobalKind {
    Const,
    Static,
}

struct Pump {
    ident: syn::Ident,
}

struct Lifecycle {
    ident: syn::Ident,
    /// 线程亲和性常量; 在解析期完成校验.
    thread: TokenStream2,
    /// 初始化入口是否可以返回 `Result[(), string]`.
    fallible: bool,
}

#[allow(clippy::too_many_lines)]
fn expand_plugin(args: &PluginArgs, mut module: ItemMod) -> syn::Result<TokenStream2> {
    let Some((brace, items)) = module.content.take() else {
        return Err(syn::Error::new_spanned(&module, "#[ksl_plugin] 需要内联模块"));
    };
    let mut kept = Vec::with_capacity(items.len());
    let mut functions = Vec::new();
    let mut globals = Vec::new();
    let mut init = None;
    let mut unload = None;
    let mut pump = None;
    let mut names = HashSet::new();
    let mut opaque = OpaqueTypes::default();
    for mut item in items {
        match &mut item {
            Item::Fn(function) => {
                let is_init = take_flag_attr(&mut function.attrs, "ksl_init")?;
                let is_unload = take_flag_attr(&mut function.attrs, "ksl_unload")?;
                let is_pump = take_flag_attr(&mut function.attrs, "ksl_event_pump")?;
                let export = take_name_attr(&mut function.attrs, "ksl_export")?;
                let execution = take_execution_attr(&mut function.attrs)?;
                if is_pump {
                    if is_init || is_unload {
                        return Err(syn::Error::new_spanned(
                            function,
                            "事件泵不能同时是生命周期入口",
                        ));
                    }
                    if !matches!(export, NameAttr::Absent) {
                        return Err(syn::Error::new_spanned(function, "事件泵不能导出"));
                    }
                    if matches!(execution, Execution::Io | Execution::Cpu) {
                        return Err(syn::Error::new_spanned(
                            function,
                            "事件泵固定在调度线程执行",
                        ));
                    }
                    reject_unsupported_function(function)?;
                    check_lifecycle_signature(function, false)?;
                    if pump
                        .replace(Pump {
                            ident: function.sig.ident.clone(),
                        })
                        .is_some()
                    {
                        return Err(syn::Error::new_spanned(function, "事件泵重复"));
                    }
                    kept.push(item);
                    continue;
                }
                if is_init || is_unload {
                    if is_init && is_unload {
                        return Err(syn::Error::new_spanned(
                            function,
                            "同一函数不能同时是初始化与卸载入口",
                        ));
                    }
                    if !matches!(export, NameAttr::Absent) {
                        return Err(syn::Error::new_spanned(function, "生命周期入口不能导出"));
                    }
                    reject_unsupported_function(function)?;
                    check_lifecycle_signature(function, is_init)?;
                    if is_init && !matches!(execution, Execution::Current) {
                        return Err(syn::Error::new_spanned(
                            function,
                            "初始化入口不接受执行类别声明; 加载线程由宿主决定",
                        ));
                    }
                    let entry = Lifecycle {
                        ident: function.sig.ident.clone(),
                        thread: execution.thread(&function.sig.ident)?,
                        fallible: match &function.sig.output {
                            ReturnType::Type(_, ty) => is_unit_result(ty),
                            ReturnType::Default => false,
                        },
                    };
                    let slot = if is_init { &mut init } else { &mut unload };
                    let what = if is_init {
                        "初始化入口"
                    } else {
                        "卸载入口"
                    };
                    if slot.replace(entry).is_some() {
                        return Err(syn::Error::new_spanned(function, format!("{what}重复")));
                    }
                    kept.push(item);
                    continue;
                }
                if let Some(name) = export_name(export, &function.sig.ident) {
                    reject_unsupported_function(function)?;
                    if name.is_empty() {
                        return Err(syn::Error::new_spanned(function, "导出名不能为空"));
                    }
                    if !names.insert(name.clone()) {
                        return Err(syn::Error::new_spanned(
                            function,
                            format!("导出名重复: {name}"),
                        ));
                    }
                    let params = function
                        .sig
                        .inputs
                        .iter()
                        .map(|arg| match arg {
                            FnArg::Typed(arg) => param_type(&arg.ty, &mut opaque),
                            FnArg::Receiver(receiver) => {
                                Err(syn::Error::new_spanned(receiver, "插件函数不能带 self"))
                            },
                        })
                        .collect::<syn::Result<Vec<_>>>()?;
                    let ret = match &function.sig.output {
                        ReturnType::Default => ValueType::Nil,
                        ReturnType::Type(_, ty) => return_type(ty, &mut opaque)?,
                    };
                    functions.push(ExportedFunction {
                        ident: function.sig.ident.clone(),
                        name,
                        params,
                        ret,
                        execution,
                    });
                }
            },
            Item::Const(item) => {
                let export = take_name_attr(&mut item.attrs, "ksl_global")?;
                let execution = take_execution_attr(&mut item.attrs)?;
                if let Some(name) = export_name(export, &item.ident) {
                    add_global(
                        &mut globals,
                        &mut names,
                        &mut opaque,
                        &item.ident,
                        name,
                        &item.ty,
                        GlobalKind::Const,
                        execution,
                    )?;
                }
            },
            Item::Static(item) => {
                let export = take_name_attr(&mut item.attrs, "ksl_global")?;
                let execution = take_execution_attr(&mut item.attrs)?;
                if let Some(name) = export_name(export, &item.ident) {
                    if matches!(item.mutability, syn::StaticMutability::Mut(_)) {
                        return Err(syn::Error::new_spanned(
                            item,
                            "外来全局值不能是 static mut",
                        ));
                    }
                    add_global(
                        &mut globals,
                        &mut names,
                        &mut opaque,
                        &item.ident,
                        name,
                        &item.ty,
                        GlobalKind::Static,
                        execution,
                    )?;
                }
            },
            _ => {},
        }
        kept.push(item);
    }

    let type_items = expand_types(&opaque);
    let function_items = expand_functions(&functions, &opaque);
    let global_items = expand_globals(&globals, &opaque);
    let lifecycle_items = expand_lifecycle(init.as_ref(), unload.as_ref());
    let pump_items = expand_pump(pump.as_ref());
    let descriptor = expand_descriptor(
        args,
        &opaque,
        &functions,
        &globals,
        init.as_ref(),
        unload.as_ref(),
        pump.as_ref(),
    );
    module.content = Some((brace, {
        let mut all = kept;
        all.extend(type_items);
        all.extend(function_items);
        all.extend(global_items);
        all.extend(lifecycle_items);
        all.extend(pump_items);
        all.extend(descriptor);
        all
    }));
    Ok(quote! { #module })
}

fn reject_unsupported_function(function: &syn::ItemFn) -> syn::Result<()> {
    if function.sig.generics.params.iter().next().is_some()
        || function.sig.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(function, "插件函数不能是泛型函数"));
    }
    if function.sig.asyncness.is_some() {
        return Err(syn::Error::new_spanned(function, "插件函数不能是 async"));
    }
    if !matches!(function.sig.safety, syn::Safety::Default) {
        return Err(syn::Error::new_spanned(
            function,
            "插件函数不能声明为 unsafe 或 safe",
        ));
    }
    if function.sig.abi.is_some() {
        return Err(syn::Error::new_spanned(function, "插件函数不写 extern 声明"));
    }
    Ok(())
}

// 参数与模块声明字段一一对应, 拆分会割裂规则.
#[allow(clippy::too_many_arguments)]
fn add_global(
    globals: &mut Vec<ExportedGlobal>,
    names: &mut HashSet<String>,
    opaque: &mut OpaqueTypes,
    ident: &syn::Ident,
    name: String,
    ty: &Type,
    kind: GlobalKind,
    execution: Execution,
) -> syn::Result<()> {
    if matches!(execution, Execution::Io | Execution::Cpu) {
        return Err(syn::Error::new_spanned(ident, "全局值只能以当前调用类别读取"));
    }
    if matches!(execution, Execution::Main) {
        return Err(syn::Error::new_spanned(
            ident,
            "全局值不需要 main 类别; 运行时在调度线程读取",
        ));
    }
    if name.is_empty() {
        return Err(syn::Error::new_spanned(ident, "导出名不能为空"));
    }
    if !names.insert(name.clone()) {
        return Err(syn::Error::new_spanned(ident, format!("导出名重复: {name}")));
    }
    let value_type = global_type(ty, opaque)?;
    if matches!(&kind, GlobalKind::Const) && matches!(&value_type, ValueType::Opaque(_)) {
        return Err(syn::Error::new_spanned(ident, "不透明全局值需要 static"));
    }
    globals.push(ExportedGlobal {
        ident: ident.clone(),
        name,
        kind,
        value_type,
        execution,
    });
    Ok(())
}

fn param_type(ty: &Type, opaque: &mut OpaqueTypes) -> syn::Result<ValueType> {
    match ty {
        Type::Reference(reference) => {
            if reference.mutability.is_some() {
                return Err(syn::Error::new_spanned(ty, "不支持 &mut 参数"));
            }
            let inner = &reference.elem;
            if is_str(inner) {
                return Ok(ValueType::Str);
            }
            if is_scalar_path(inner) || is_string_path(inner) {
                return Err(syn::Error::new_spanned(
                    ty,
                    "标量参数按值传递, 只有 &str 与不透明类型使用引用",
                ));
            }
            if has_generic_args(inner) {
                return Err(syn::Error::new_spanned(ty, "不支持带泛型参数的类型"));
            }
            let _ = opaque.index(inner);
            Ok(ValueType::Opaque(Box::new((**inner).clone())))
        },
        Type::Path(path) => {
            if is_i64(path) {
                Ok(ValueType::I64)
            } else if is_u64(path) {
                Ok(ValueType::U64)
            } else if is_u8(path) {
                Ok(ValueType::U8)
            } else if is_f64(path) {
                Ok(ValueType::F64)
            } else if is_bool(path) {
                Ok(ValueType::Bool)
            } else if is_char(path) {
                Ok(ValueType::Char)
            } else if is_string_path(ty) {
                Ok(ValueType::String)
            } else if is_str(ty) {
                Err(syn::Error::new_spanned(ty, "str 需要写成 &str"))
            } else if has_generic_args(ty) {
                Err(syn::Error::new_spanned(ty, "不支持带泛型参数的类型"))
            } else {
                let index = opaque.index(ty);
                let _ = index;
                Ok(ValueType::Opaque(Box::new(ty.clone())))
            }
        },
        Type::Tuple(tuple) if tuple.elems.is_empty() => {
            Err(syn::Error::new_spanned(ty, "nil 参数无意义"))
        },
        Type::Paren(paren) => param_type(&paren.elem, opaque),
        Type::Group(group) => param_type(&group.elem, opaque),
        other => Err(syn::Error::new_spanned(other, "不支持的插件参数类型")),
    }
}

fn return_type(ty: &Type, opaque: &mut OpaqueTypes) -> syn::Result<ValueType> {
    match ty {
        Type::Reference(reference) => {
            if is_str(&reference.elem) {
                Ok(ValueType::Str)
            } else {
                Err(syn::Error::new_spanned(
                    ty,
                    "不透明返回值按值返回, 不能返回引用",
                ))
            }
        },
        Type::Path(path) => {
            if let Some(result) = result_return(ty, opaque)? {
                Ok(result)
            } else if is_i64(path) {
                Ok(ValueType::I64)
            } else if is_u64(path) {
                Ok(ValueType::U64)
            } else if is_u8(path) {
                Ok(ValueType::U8)
            } else if is_f64(path) {
                Ok(ValueType::F64)
            } else if is_bool(path) {
                Ok(ValueType::Bool)
            } else if is_char(path) {
                Ok(ValueType::Char)
            } else if is_string_path(ty) {
                Ok(ValueType::String)
            } else if is_str(ty) {
                Ok(ValueType::Str)
            } else if has_generic_args(ty) {
                Err(syn::Error::new_spanned(ty, "不支持带泛型参数的类型"))
            } else {
                let _ = opaque.index(ty);
                Ok(ValueType::Opaque(Box::new(ty.clone())))
            }
        },
        Type::Tuple(tuple) if tuple.elems.is_empty() => Ok(ValueType::Nil),
        Type::Paren(paren) => return_type(&paren.elem, opaque),
        Type::Group(group) => return_type(&group.elem, opaque),
        other => Err(syn::Error::new_spanned(other, "不支持的插件返回类型")),
    }
}

/// 解析 `Result<T, String>` 返回类型.
fn result_return(ty: &Type, opaque: &mut OpaqueTypes) -> syn::Result<Option<ValueType>> {
    let Type::Path(path) = ty else {
        return Ok(None);
    };
    let Some(segment) = path.path.segments.last() else {
        return Ok(None);
    };
    if segment.ident != "Result" {
        return Ok(None);
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Err(syn::Error::new_spanned(ty, "Result 需要两个类型参数"));
    };
    let mut types = Vec::new();
    for argument in &arguments.args {
        let GenericArgument::Type(ty) = argument else {
            return Err(syn::Error::new_spanned(argument, "Result 只接受类型参数"));
        };
        types.push(ty);
    }
    let [inner, error] = types.as_slice() else {
        return Err(syn::Error::new_spanned(ty, "Result 需要两个类型参数"));
    };
    if !is_string_path(error) {
        return Err(syn::Error::new_spanned(error, "Result 的错误类型必须是 String"));
    }
    let inner_type = inner;
    let inner = return_type(inner_type, opaque)?;
    if matches!(inner, ValueType::Result(_)) {
        return Err(syn::Error::new_spanned(inner_type, "不支持嵌套 Result"));
    }
    Ok(Some(ValueType::Result(Box::new(inner))))
}

fn global_type(ty: &Type, opaque: &mut OpaqueTypes) -> syn::Result<ValueType> {
    match ty {
        Type::Reference(reference) if is_str(&reference.elem) => Ok(ValueType::Str),
        Type::Path(path) => {
            if is_i64(path) {
                Ok(ValueType::I64)
            } else if is_u64(path) {
                Ok(ValueType::U64)
            } else if is_u8(path) {
                Ok(ValueType::U8)
            } else if is_f64(path) {
                Ok(ValueType::F64)
            } else if is_bool(path) {
                Ok(ValueType::Bool)
            } else if is_char(path) {
                Ok(ValueType::Char)
            } else if is_string_path(ty) {
                Ok(ValueType::String)
            } else if is_str(ty) {
                Err(syn::Error::new_spanned(ty, "全局字符串需要写成 &str"))
            } else if has_generic_args(ty) {
                Err(syn::Error::new_spanned(ty, "不支持带泛型参数的类型"))
            } else {
                let _ = opaque.index(ty);
                Ok(ValueType::Opaque(Box::new(ty.clone())))
            }
        },
        Type::Tuple(tuple) if tuple.elems.is_empty() => Ok(ValueType::Nil),
        Type::Paren(paren) => global_type(&paren.elem, opaque),
        Type::Group(group) => global_type(&group.elem, opaque),
        other => Err(syn::Error::new_spanned(other, "不支持的外来全局值类型")),
    }
}

fn path_ident(ty: &Type) -> Option<&syn::Ident> {
    let Type::Path(path) = ty else {
        return None;
    };
    path.path.segments.last().map(|segment| &segment.ident)
}

fn is_i64(path: &syn::TypePath) -> bool {
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "i64")
}

fn is_u64(path: &syn::TypePath) -> bool {
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "u64")
}

fn is_u8(path: &syn::TypePath) -> bool {
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "u8")
}

fn is_f64(path: &syn::TypePath) -> bool {
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "f64")
}

fn is_bool(path: &syn::TypePath) -> bool {
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "bool")
}

fn is_char(path: &syn::TypePath) -> bool {
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "char")
}

fn is_string_path(ty: &Type) -> bool {
    path_ident(ty).is_some_and(|ident| ident == "String")
}

fn is_str(ty: &Type) -> bool {
    path_ident(ty).is_some_and(|ident| ident == "str")
}

fn has_generic_args(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path.segments.iter().any(|segment| {
        matches!(
            segment.arguments,
            PathArguments::AngleBracketed(_) | PathArguments::Parenthesized(_)
        )
    })
}

fn is_scalar_path(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    is_scalar(path)
}

fn is_scalar(path: &syn::TypePath) -> bool {
    is_i64(path)
        || is_u64(path)
        || is_u8(path)
        || is_f64(path)
        || is_bool(path)
        || is_char(path)
}

fn abi_type(value: &ValueType, opaque: &OpaqueTypes) -> TokenStream2 {
    match value {
        ValueType::I64 => quote! { ::ksl_plugin::KslType::i64() },
        ValueType::U64 => quote! { ::ksl_plugin::KslType::u64() },
        ValueType::U8 => quote! { ::ksl_plugin::KslType::u8() },
        ValueType::F64 => quote! { ::ksl_plugin::KslType::f64() },
        ValueType::Bool => quote! { ::ksl_plugin::KslType::bool() },
        ValueType::Char => quote! { ::ksl_plugin::KslType::char() },
        ValueType::String | ValueType::Str => quote! { ::ksl_plugin::KslType::string() },
        ValueType::Nil => quote! { ::ksl_plugin::KslType::nil() },
        ValueType::Opaque(ty) => {
            let index = opaque_type_index(opaque, ty.as_ref());
            quote! { ::ksl_plugin::KslType::opaque(#index as u32) }
        },
        ValueType::Result(inner) => abi_type(inner, opaque),
    }
}

fn opaque_type_index(opaque: &OpaqueTypes, ty: &Type) -> usize {
    let key = quote!(#ty).to_string();
    opaque.indexes.get(&key).copied().expect("不透明类型已登记")
}

fn arg_expr(value: &ValueType, index: usize, opaque: &OpaqueTypes) -> TokenStream2 {
    let arg = quote! { &__args[#index] };
    let internal = quote! { ::ksl_plugin::STATUS_INTERNAL };
    match value {
        ValueType::I64 => quote! {
            ::ksl_plugin::value::as_i64(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::U64 => quote! {
            ::ksl_plugin::value::as_u64(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::U8 => quote! {
            ::ksl_plugin::value::as_u8(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::F64 => quote! {
            ::ksl_plugin::value::as_f64(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::Bool => quote! {
            ::ksl_plugin::value::as_bool(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::Char => quote! {
            ::ksl_plugin::value::as_char(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::String => quote! {
            ::ksl_plugin::value::as_string(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::Str => quote! {
            ::ksl_plugin::value::as_str(#arg)
                .map_err(|message| (#internal, message))?
        },
        ValueType::Nil => quote! { ::ksl_plugin::value::nil() },
        ValueType::Opaque(ty) => {
            let ty = ty.as_ref();
            let type_index = opaque_type_index(opaque, ty);
            quote! {
                {
                    let __opaque = ::ksl_plugin::value::as_opaque(#arg)
                        .map_err(|message| (#internal, message))?;
                    if __opaque.type_index != #type_index as u32 {
                        return Err((#internal, "不透明类型不符".to_string()));
                    }
                    unsafe { &*(__opaque.handle as *const #ty) }
                }
            }
        },
        ValueType::Result(_) => {
            unreachable!("Result 不能作为参数")
        },
    }
}

fn ret_expr(value: &ValueType, opaque: &OpaqueTypes) -> TokenStream2 {
    match value {
        ValueType::I64 => quote! { ::ksl_plugin::KslValue::i64(__value) },
        ValueType::U64 => quote! { ::ksl_plugin::KslValue::u64(__value) },
        ValueType::U8 => quote! { ::ksl_plugin::KslValue::u8(__value) },
        ValueType::F64 => quote! { ::ksl_plugin::KslValue::f64(__value) },
        ValueType::Bool => quote! { ::ksl_plugin::KslValue::bool(__value) },
        ValueType::Char => quote! { ::ksl_plugin::KslValue::char(__value) },
        ValueType::String => quote! { ::ksl_plugin::KslValue::string(&__value) },
        ValueType::Str => quote! { ::ksl_plugin::KslValue::string(__value) },
        ValueType::Nil => quote! { ::ksl_plugin::value::nil() },
        ValueType::Opaque(ty) => {
            let ty = ty.as_ref();
            let type_index = opaque_type_index(opaque, ty);
            quote! {
                ::ksl_plugin::KslValue::opaque(
                    #type_index as u32,
                    <#ty as ::ksl_plugin::KslOpaque>::into_handle(__value),
                    true,
                )
            }
        },
        ValueType::Result(_) => {
            unreachable!("Result 由函数包装层处理")
        },
    }
}

fn expand_types(opaque: &OpaqueTypes) -> Vec<Item> {
    let count = opaque.types.len();
    let mut items = Vec::with_capacity(count + 1);
    for (index, ty) in opaque.types.iter().enumerate() {
        let release = format_ident!("__ksl_release_{index}");
        items.push(syn::parse_quote! {
            unsafe extern "C" fn #release(handle: u64) {
                unsafe { <#ty as ::ksl_plugin::KslOpaque>::release(handle) }
            }
        });
    }
    let entries = opaque.types.iter().enumerate().map(|(index, ty)| {
        let release = format_ident!("__ksl_release_{index}");
        quote! {
            ::ksl_plugin::KslOpaqueType {
                name: ::ksl_plugin::KslStr::from_static(<#ty as ::ksl_plugin::KslOpaque>::NAME),
                release: Some(#release),
                release_thread: <#ty as ::ksl_plugin::KslOpaque>::RELEASE_THREAD,
            }
        }
    });
    let types: Item = syn::parse_quote! {
        static __KSL_TYPES: [::ksl_plugin::KslOpaqueType; #count] = [#(#entries),*];
    };
    items.push(types);
    items
}

#[allow(clippy::too_many_lines)]
fn expand_functions(functions: &[ExportedFunction], opaque: &OpaqueTypes) -> Vec<Item> {
    let mut items = Vec::with_capacity(functions.len() * 2 + 1);
    let mut defs = Vec::with_capacity(functions.len());
    for (index, function) in functions.iter().enumerate() {
        let wrapper = format_ident!("__ksl_fn_{index}");
        let params = format_ident!("__KSL_PARAMS_{index}");
        let ident = &function.ident;
        let arity = function.params.len();
        let arg_lets = function.params.iter().enumerate().map(|(arg_index, value)| {
            let name = format_ident!("__arg{arg_index}");
            let expr = arg_expr(value, arg_index, opaque);
            quote! { let #name = #expr; }
        });
        let arg_names: Vec<_> = (0..arity)
            .map(|arg_index| format_ident!("__arg{arg_index}"))
            .collect();
        let (call, ret) = match &function.ret {
            ValueType::Result(inner) => (
                quote! {
                    let __value = #ident(#(#arg_names),*)
                        .map_err(|message| {
                            (::ksl_plugin::STATUS_ERROR, message)
                        })?;
                },
                ret_expr(inner, opaque),
            ),
            other => (
                quote! { let __value = #ident(#(#arg_names),*); },
                ret_expr(other, opaque),
            ),
        };
        let param_types = function.params.iter().map(|value| abi_type(value, opaque));
        let ret_type = abi_type(&function.ret, opaque);
        let flags = if matches!(&function.ret, ValueType::Result(_)) {
            quote! { ::ksl_plugin::FUNCTION_FALLIBLE }
        } else {
            quote! { 0 }
        };
        let name = LitStr::new(&function.name, proc_macro2::Span::call_site());
        let source_name = LitStr::new(&function.ident.to_string(), function.ident.span());
        let execution = function.execution.tokens();
        items.push(syn::parse_quote! {
            static #params: [::ksl_plugin::KslType; #arity] = [#(#param_types),*];
        });
        items.push(syn::parse_quote! {
            unsafe extern "C" fn #wrapper(
                args: *const ::ksl_plugin::KslValue,
                argc: u32,
                out: *mut ::ksl_plugin::KslValue,
            ) -> u32 {
                unsafe {
                    if out.is_null() {
                        return ::ksl_plugin::STATUS_INTERNAL;
                    }
                    let __result = ::std::panic::catch_unwind(
                        ::std::panic::AssertUnwindSafe(
                            || -> ::std::result::Result<
                                ::ksl_plugin::KslValue,
                                (u32, ::std::string::String),
                            > {
                                if argc != #arity as u32 {
                                    return Err((
                                        ::ksl_plugin::STATUS_INTERNAL,
                                        ::std::format!(
                                            "实参个数不符: 期望 {}, 实际 {}",
                                            #arity,
                                            argc,
                                        ),
                                    ));
                                }
                                let __args: &[::ksl_plugin::KslValue] = if argc == 0 {
                                    &[]
                                } else {
                                    if args.is_null() {
                                        return Err((
                                            ::ksl_plugin::STATUS_INTERNAL,
                                            "实参指针为空".to_string(),
                                        ));
                                    }
                                    ::std::slice::from_raw_parts(args, argc as usize)
                                };
                                #(#arg_lets)*
                                #call
                                Ok(#ret)
                            },
                        ),
                    );
                    match __result {
                        Ok(Ok(__value)) => {
                            *out = __value;
                            ::ksl_plugin::STATUS_OK
                        },
                        Ok(Err((__status, __message))) => {
                            *out = ::ksl_plugin::value::error(&__message);
                            __status
                        },
                        Err(_) => {
                            *out = ::ksl_plugin::value::error("插件函数 panic");
                            ::ksl_plugin::STATUS_INTERNAL
                        },
                    }
                }
            }
        });
        defs.push(quote! {
            ::ksl_plugin::KslFunctionDef {
                name: ::ksl_plugin::KslStr::from_static(#name),
                source_name: ::ksl_plugin::KslStr::from_static(#source_name),
                params: ::core::ptr::addr_of!(#params) as *const ::ksl_plugin::KslType,
                param_count: #arity as u32,
                flags: #flags,
                ret: #ret_type,
                entry: #wrapper,
                execution: #execution,
            }
        });
    }
    let count = functions.len();
    items.push(syn::parse_quote! {
        static __KSL_FUNCTIONS: [::ksl_plugin::KslFunctionDef; #count] = [#(#defs),*];
    });
    items
}

fn expand_globals(globals: &[ExportedGlobal], opaque: &OpaqueTypes) -> Vec<Item> {
    let mut items = Vec::with_capacity(globals.len() * 2 + 1);
    let mut defs = Vec::with_capacity(globals.len());
    for (index, global) in globals.iter().enumerate() {
        let getter = format_ident!("__ksl_global_{index}");
        let ident = &global.ident;
        let name = LitStr::new(&global.name, proc_macro2::Span::call_site());
        let source_name = LitStr::new(&global.ident.to_string(), global.ident.span());
        let value = match (&global.kind, &global.value_type) {
            (GlobalKind::Const | GlobalKind::Static, ValueType::I64) => {
                quote! { ::ksl_plugin::KslValue::i64(#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::U64) => {
                quote! { ::ksl_plugin::KslValue::u64(#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::U8) => {
                quote! { ::ksl_plugin::KslValue::u8(#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::F64) => {
                quote! { ::ksl_plugin::KslValue::f64(#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::Bool) => {
                quote! { ::ksl_plugin::KslValue::bool(#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::Char) => {
                quote! { ::ksl_plugin::KslValue::char(#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::String) => {
                quote! { ::ksl_plugin::KslValue::string(&#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::Str) => {
                quote! { ::ksl_plugin::KslValue::string(#ident) }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::Nil)
            | (GlobalKind::Const, ValueType::Opaque(_)) => {
                quote! { ::ksl_plugin::KslValue::nil() }
            },
            (GlobalKind::Static, ValueType::Opaque(ty)) => {
                let type_index = opaque_type_index(opaque, ty.as_ref());
                quote! {
                    ::ksl_plugin::KslValue::opaque(
                        #type_index as u32,
                        ::core::ptr::addr_of!(#ident) as u64,
                        false,
                    )
                }
            },
            (GlobalKind::Const | GlobalKind::Static, ValueType::Result(_)) => {
                unreachable!("Result 不能作为全局值")
            },
        };
        let ty = abi_type(&global.value_type, opaque);
        items.push(syn::parse_quote! {
            unsafe extern "C" fn #getter(out: *mut ::ksl_plugin::KslValue) -> u32 {
                unsafe {
                    if out.is_null() {
                        return ::ksl_plugin::STATUS_INTERNAL;
                    }
                    *out = #value;
                    ::ksl_plugin::STATUS_OK
                }
            }
        });
        let execution = global.execution.tokens();
        defs.push(quote! {
            ::ksl_plugin::KslGlobalDef {
                name: ::ksl_plugin::KslStr::from_static(#name),
                source_name: ::ksl_plugin::KslStr::from_static(#source_name),
                ty: #ty,
                get: #getter,
                execution: #execution,
            }
        });
    }
    let count = globals.len();
    items.push(syn::parse_quote! {
        static __KSL_GLOBALS: [::ksl_plugin::KslGlobalDef; #count] = [#(#defs),*];
    });
    items
}

/// 生命周期入口只接受无参函数; 初始化可以返回 `Result[(), string]`.
fn check_lifecycle_signature(function: &syn::ItemFn, is_init: bool) -> syn::Result<()> {
    if !function.sig.inputs.is_empty() {
        return Err(syn::Error::new_spanned(
            &function.sig.inputs,
            "生命周期入口不接受参数",
        ));
    }
    match &function.sig.output {
        ReturnType::Default => Ok(()),
        ReturnType::Type(_, ty) if is_init && is_unit_result(ty) => Ok(()),
        ReturnType::Type(_, ty) => Err(syn::Error::new_spanned(
            ty,
            "生命周期入口只允许返回 () 或 Result[(), string]",
        )),
    }
}

/// 是否为 `Result[(), string]`.
fn is_unit_result(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if segment.ident != "Result" {
        return false;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return false;
    };
    let mut types = arguments.args.iter().filter_map(|argument| match argument {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let (Some(ok), Some(err), None) = (types.next(), types.next(), types.next()) else {
        return false;
    };
    matches!(ok, Type::Tuple(tuple) if tuple.elems.is_empty())
        && matches!(err, Type::Path(path) if path.path.is_ident("String"))
}

/// 生成生命周期入口的 C 包装.
fn expand_lifecycle(init: Option<&Lifecycle>, unload: Option<&Lifecycle>) -> Vec<Item> {
    let mut items = Vec::new();
    if let Some(entry) = init {
        let ident = &entry.ident;
        let body = if entry.fallible {
            quote! { #ident() }
        } else {
            quote! {
                #ident();
                ::std::result::Result::Ok(())
            }
        };
        items.push(syn::parse_quote! {
            unsafe extern "C" fn __ksl_init_entry(
                out: *mut ::ksl_plugin::KslValue,
            ) -> u32 {
                unsafe {
                    if out.is_null() {
                        return ::ksl_plugin::STATUS_INTERNAL;
                    }
                    let __result = ::std::panic::catch_unwind(
                        ::std::panic::AssertUnwindSafe(
                            || -> ::std::result::Result<(), ::std::string::String> {
                                #body
                            },
                        ),
                    );
                    match __result {
                        Ok(Ok(())) => ::ksl_plugin::STATUS_OK,
                        Ok(Err(__message)) => {
                            *out = ::ksl_plugin::value::error(&__message);
                            ::ksl_plugin::STATUS_ERROR
                        },
                        Err(_) => {
                            *out = ::ksl_plugin::value::error("插件初始化 panic");
                            ::ksl_plugin::STATUS_INTERNAL
                        },
                    }
                }
            }
        });
    }
    if let Some(entry) = unload {
        let ident = &entry.ident;
        items.push(syn::parse_quote! {
            unsafe extern "C" fn __ksl_unload_entry() {
                let _ = ::std::panic::catch_unwind(
                    ::std::panic::AssertUnwindSafe(|| #ident()),
                );
            }
        });
    }
    items
}

fn expand_pump(pump: Option<&Pump>) -> Vec<Item> {
    let Some(pump) = pump else {
        return Vec::new();
    };
    let ident = &pump.ident;
    vec![syn::parse_quote! {
        unsafe extern "C" fn __ksl_pump() {
            let _ = ::std::panic::catch_unwind(
                ::std::panic::AssertUnwindSafe(|| #ident()),
            );
        }
    }]
}

fn expand_descriptor(
    args: &PluginArgs,
    opaque: &OpaqueTypes,
    functions: &[ExportedFunction],
    globals: &[ExportedGlobal],
    init: Option<&Lifecycle>,
    unload: Option<&Lifecycle>,
    pump: Option<&Pump>,
) -> Vec<Item> {
    let name = LitStr::new(&args.name, proc_macro2::Span::call_site());
    let version = LitStr::new(&args.version, proc_macro2::Span::call_site());
    let type_count = opaque.types.len();
    let function_count = functions.len();
    let global_count = globals.len();
    let init_entry = if init.is_some() {
        quote! { ::core::option::Option::Some(__ksl_init_entry) }
    } else {
        quote! { ::core::option::Option::None }
    };
    let pump_entry = if pump.is_some() {
        quote! { ::core::option::Option::Some(__ksl_pump) }
    } else {
        quote! { ::core::option::Option::None }
    };
    let (unload_entry, unload_thread) = match unload {
        Some(entry) => {
            let thread = &entry.thread;
            (
                quote! { ::core::option::Option::Some(__ksl_unload_entry) },
                quote! { #thread },
            )
        },
        None => (
            quote! { ::core::option::Option::None },
            quote! { ::ksl_plugin::THREAD_ANY },
        ),
    };
    vec![
        syn::parse_quote! {
            static __KSL_PLUGIN: ::ksl_plugin::KslPlugin = ::ksl_plugin::KslPlugin {
                abi_version: ::ksl_plugin::ABI_VERSION,
                name: ::ksl_plugin::KslStr::from_static(#name),
                version: ::ksl_plugin::KslStr::from_static(#version),
                types: ::core::ptr::addr_of!(__KSL_TYPES) as *const ::ksl_plugin::KslOpaqueType,
                type_count: #type_count as u32,
                functions: ::core::ptr::addr_of!(__KSL_FUNCTIONS)
                    as *const ::ksl_plugin::KslFunctionDef,
                function_count: #function_count as u32,
                globals: ::core::ptr::addr_of!(__KSL_GLOBALS)
                    as *const ::ksl_plugin::KslGlobalDef,
                global_count: #global_count as u32,
                init: #init_entry,
                unload: #unload_entry,
                unload_thread: #unload_thread,
                pump: #pump_entry,
            };
        },
        syn::parse_quote! {
            #[unsafe(no_mangle)]
            pub extern "C" fn ksl_plugin_init(
                host_abi_version: u32,
            ) -> *const ::ksl_plugin::KslPlugin {
                if host_abi_version != ::ksl_plugin::ABI_VERSION {
                    return ::core::ptr::null();
                }
                ::core::ptr::addr_of!(__KSL_PLUGIN)
            }
        },
    ]
}
