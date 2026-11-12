//! 原生插件导出宏.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::{
    FnArg, GenericArgument, Item, ItemFn, ItemMod, LitStr, Meta, PathArguments, ReturnType,
    Type,
};

/// 展开 `#[ksl_native_plugin]`.
pub fn expand(args: TokenStream, input: TokenStream) -> TokenStream {
    let mut name = None;
    let mut version = None;
    let mut types = Vec::new();
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            name = Some(meta.value()?.parse::<LitStr>()?);
            Ok(())
        } else if meta.path.is_ident("version") {
            version = Some(meta.value()?.parse::<LitStr>()?);
            Ok(())
        } else if meta.path.is_ident("types") {
            let array: syn::ExprArray = meta.value()?.parse()?;
            for element in array.elems {
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(text),
                    ..
                }) = element
                else {
                    return Err(syn::Error::new_spanned(element, "类型名写成字符串字面值"));
                };
                types.push(text);
            }
            Ok(())
        } else {
            Err(meta.error("不支持的参数"))
        }
    });
    parser.parse2(args).map_or_else(
        |error| error.to_compile_error(),
        |()| match expand_module(name, version, &types, input) {
            Ok(tokens) => tokens,
            Err(error) => error.to_compile_error(),
        },
    )
}

fn expand_module(
    name: Option<LitStr>,
    version: Option<LitStr>,
    types: &[LitStr],
    input: TokenStream,
) -> syn::Result<TokenStream> {
    let Some(name) = name else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "缺少 name 参数",
        ));
    };
    let mut module: ItemMod = syn::parse2(input)?;
    let parts = collect_parts(&mut module)?;
    let module_ident = module.ident.clone();
    let content = &mut module.content.as_mut().expect("已校验内联模块").1;
    add_lifecycle(content, &parts.lifecycle);
    let definitions = build_function_definitions(content, &parts.exports, &module_ident);
    let global_definitions = build_global_definitions(content, &parts.globals, &module_ident);
    let version = version.map_or_else(
        || quote! { ::ksl_value::native::KslStr::from_static(env!("CARGO_PKG_VERSION")) },
        |version| quote! { ::ksl_value::native::KslStr::from_static(#version) },
    );
    let count = parts.exports.len();
    let type_count = types.len();
    let global_count = parts.globals.len();
    let init_entry = lifecycle_entry(
        parts.lifecycle.init.as_ref(),
        "__ksl_native_init",
        &module_ident,
    );
    let unload_entry = lifecycle_entry(
        parts.lifecycle.unload.as_ref(),
        "__ksl_native_unload",
        &module_ident,
    );
    let pump_entry = lifecycle_entry(
        parts.lifecycle.pump.as_ref(),
        "__ksl_native_pump",
        &module_ident,
    );
    let type_definitions = types.iter().map(|name| {
        quote! {
            ::ksl_value::native::NativeType {
                name: ::ksl_value::native::KslStr::from_static(#name),
            }
        }
    });

    Ok(quote! {
        #module

        #[doc(hidden)]
        static __KSL_NATIVE_FUNCTIONS: &[::ksl_value::native::NativeFunction] = &[
            #(#definitions),*
        ];

        #[doc(hidden)]
        static __KSL_NATIVE_TYPES: &[::ksl_value::native::NativeType] = &[
            #(#type_definitions),*
        ];

        #[doc(hidden)]
        static __KSL_NATIVE_GLOBALS: &[::ksl_value::native::NativeGlobal] = &[
            #(#global_definitions),*
        ];

        #[unsafe(no_mangle)]
        pub extern "C" fn ksl_native_plugin() -> *const ::ksl_value::native::NativePlugin {
            static PLUGIN: ::ksl_value::native::NativePlugin =
                ::ksl_value::native::NativePlugin {
                    abi: ::ksl_value::native::NATIVE_ABI_VERSION,
                    name: ::ksl_value::native::KslStr::from_static(#name),
                    version: #version,
                    functions: __KSL_NATIVE_FUNCTIONS.as_ptr(),
                    function_count: #count as u32,
                    types: __KSL_NATIVE_TYPES.as_ptr(),
                    type_count: #type_count as u32,
                    globals: __KSL_NATIVE_GLOBALS.as_ptr(),
                    global_count: #global_count as u32,
                    init: #init_entry,
                    unload: #unload_entry,
                    pump: #pump_entry,
                };
            &PLUGIN
        }
    })
}

type Items = Vec<Item>;

#[derive(Default)]
struct PluginParts {
    exports: Vec<FunctionExport>,
    globals: Vec<(syn::Ident, String, GlobalCtor)>,
    lifecycle: Lifecycle,
}

#[derive(Default)]
struct Lifecycle {
    init: Option<syn::Ident>,
    unload: Option<syn::Ident>,
    pump: Option<syn::Ident>,
}

fn collect_parts(module: &mut ItemMod) -> syn::Result<PluginParts> {
    let content = module
        .content
        .as_mut()
        .ok_or_else(|| syn::Error::new_spanned(&module.ident, "只能用于内联模块"))?;
    let mut parts = PluginParts::default();
    for item in &mut content.1 {
        match item {
            Item::Fn(function) => {
                if let Some(export) = collect_function(function, &mut parts.lifecycle)? {
                    parts.exports.push(export);
                }
            },
            Item::Const(item) => {
                if let Some(global) = collect_global(&mut item.attrs, &item.ident, &item.ty)? {
                    parts.globals.push(global);
                }
            },
            Item::Static(item) => {
                if let Some(global) = collect_global(&mut item.attrs, &item.ident, &item.ty)? {
                    parts.globals.push(global);
                }
            },
            _ => {},
        }
    }
    Ok(parts)
}

fn collect_function(
    function: &mut ItemFn,
    lifecycle: &mut Lifecycle,
) -> syn::Result<Option<FunctionExport>> {
    let mut marker = 0;
    for (name, slot) in [
        ("ksl_native_init", &mut lifecycle.init),
        ("ksl_native_unload", &mut lifecycle.unload),
        ("ksl_native_event_pump", &mut lifecycle.pump),
    ] {
        if take_marker(&mut function.attrs, name).is_some() {
            if slot.is_some() {
                return Err(syn::Error::new_spanned(&function.sig.ident, "生命周期入口重复"));
            }
            *slot = Some(function.sig.ident.clone());
            marker += 1;
        }
    }
    if marker > 1 {
        return Err(syn::Error::new_spanned(
            &function.sig.ident,
            "一个函数只能标记一种生命周期入口",
        ));
    }
    if marker == 1 {
        return Ok(None);
    }
    let mut export = None;
    function.attrs.retain(|attr| {
        if attr.path().is_ident("ksl_native_export") {
            export = Some(attr.clone());
            false
        } else {
            true
        }
    });
    let Some(attribute) = export else {
        return Ok(None);
    };
    let link = export_name(&attribute, &function.sig.ident)?;
    let mut execution = "EXECUTION_CURRENT";
    let mut invalid = None;
    function.attrs.retain(|attr| {
        if attr.path().is_ident("ksl_native_exec") {
            match attr.parse_args::<syn::Ident>() {
                Ok(kind) => match kind.to_string().as_str() {
                    "io" => execution = "EXECUTION_IO",
                    "cpu" => execution = "EXECUTION_CPU",
                    "main" => {},
                    _ => {
                        invalid =
                            Some(syn::Error::new_spanned(&kind, "执行类别只能是 io 或 cpu"));
                    },
                },
                Err(error) => invalid = Some(error),
            }
            false
        } else {
            true
        }
    });
    if let Some(error) = invalid {
        return Err(error);
    }
    function_export(function, link, execution).map(Some)
}

fn collect_global(
    attrs: &mut Vec<syn::Attribute>,
    ident: &syn::Ident,
    ty: &Type,
) -> syn::Result<Option<(syn::Ident, String, GlobalCtor)>> {
    let mut export = None;
    attrs.retain(|attr| {
        if attr.path().is_ident("ksl_native_global") {
            export = Some(attr.clone());
            false
        } else {
            true
        }
    });
    let Some(attribute) = export else {
        return Ok(None);
    };
    let link = export_name(&attribute, ident)?;
    let ctor = global_ctor(ty)?;
    Ok(Some((ident.clone(), link, ctor)))
}

fn add_lifecycle(items: &mut Items, lifecycle: &Lifecycle) {
    if let Some(ident) = &lifecycle.init {
        let item: ItemFn = syn::parse_quote! {
            pub(super) extern "Rust" fn __ksl_native_init()
                -> ::core::result::Result<(), ::std::string::String>
            {
                match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    #ident()
                })) {
                    Ok(result) => result,
                    Err(_) => Err("插件初始化 panic".to_string()),
                }
            }
        };
        items.push(Item::Fn(item));
    }
    if let Some(ident) = &lifecycle.unload {
        let item: ItemFn = syn::parse_quote! {
            pub(super) extern "Rust" fn __ksl_native_unload() {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    #ident();
                }));
            }
        };
        items.push(Item::Fn(item));
    }
    if let Some(ident) = &lifecycle.pump {
        let item: ItemFn = syn::parse_quote! {
            pub(super) extern "Rust" fn __ksl_native_pump() {
                let _ = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    #ident();
                }));
            }
        };
        items.push(Item::Fn(item));
    }
}

fn lifecycle_entry(
    ident: Option<&syn::Ident>,
    name: &str,
    module_ident: &syn::Ident,
) -> TokenStream {
    ident.map_or_else(
        || quote! { None },
        |_| {
            let name = format_ident!("{name}");
            quote! { Some(#module_ident::#name) }
        },
    )
}

fn build_function_definitions(
    items: &mut Items,
    exports: &[FunctionExport],
    module_ident: &syn::Ident,
) -> Vec<TokenStream> {
    let mut definitions = Vec::with_capacity(exports.len());
    for (index, export) in exports.iter().enumerate() {
        let wrapper = format_ident!("__ksl_native_entry_{index}");
        let ident = &export.ident;
        let link = &export.link;
        let execution = syn::Ident::new(export.execution, proc_macro2::Span::call_site());
        let source = ident.to_string();
        let body = export.body(ident);
        // 包装函数与插件函数同模块, 可直接调用私有函数; 父模块通过 pub(super) 引用.
        let item: ItemFn = syn::parse_quote! {
            pub(super) extern "Rust" fn #wrapper(args: &[::ksl_value::value::Value])
                -> ::core::result::Result<::ksl_value::value::Value, ::std::string::String>
            {
                #body
            }
        };
        items.push(Item::Fn(item));
        definitions.push(quote! {
            ::ksl_value::native::NativeFunction {
                name: ::ksl_value::native::KslStr::from_static(#link),
                source_name: ::ksl_value::native::KslStr::from_static(#source),
                entry: #module_ident::#wrapper,
                execution: ::ksl_value::native::#execution,
            }
        });
    }
    definitions
}

fn build_global_definitions(
    items: &mut Items,
    globals: &[(syn::Ident, String, GlobalCtor)],
    module_ident: &syn::Ident,
) -> Vec<TokenStream> {
    let mut definitions = Vec::with_capacity(globals.len());
    for (index, (ident, link, ctor)) in globals.iter().enumerate() {
        let getter = format_ident!("__ksl_native_global_{index}");
        let source = ident.to_string();
        let value = match ctor {
            GlobalCtor::Scalar(ctor) => quote! {
                ::ksl_value::value::Value::#ctor(#ident)
            },
            GlobalCtor::Str => quote! {
                ::ksl_value::value::Value::string(#ident)
            },
            GlobalCtor::String => quote! {
                ::ksl_value::value::Value::string(#ident.as_str())
            },
        };
        let item: ItemFn = syn::parse_quote! {
            pub(super) extern "Rust" fn #getter() -> ::ksl_value::value::Value {
                #value
            }
        };
        items.push(Item::Fn(item));
        definitions.push(quote! {
            ::ksl_value::native::NativeGlobal {
                name: ::ksl_value::native::KslStr::from_static(#link),
                source_name: ::ksl_value::native::KslStr::from_static(#source),
                get: #module_ident::#getter,
            }
        });
    }
    definitions
}

/// 一个导出函数的签名.
struct FunctionExport {
    ident: syn::Ident,
    link: String,
    execution: &'static str,
    raw: bool,
    params: Vec<Param>,
    ret: Return,
}

impl FunctionExport {
    /// 生成入口函数体.
    fn body(&self, ident: &syn::Ident) -> TokenStream {
        if self.raw {
            return quote! {
                match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                    #ident(args)
                })) {
                    Ok(result) => result,
                    Err(_) => Err("插件函数 panic".to_string()),
                }
            };
        }
        let count = self.params.len();
        let names: Vec<_> = (0..count).map(|index| format_ident!("__arg{index}")).collect();
        let decodes = self.params.iter().enumerate().map(|(index, param)| {
            let name = &names[index];
            let decode = param.decode(index);
            quote! { let #name = #decode; }
        });
        let call = quote! { #ident(#(#names),*) };
        let encode = if matches!(&self.ret, Return::Result(_)) {
            self.ret.encode(&call)
        } else {
            let encoded = self.ret.encode(&quote! { __value });
            quote! {
                let __value = #call;
                #encoded
            }
        };
        quote! {
            match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(
                || -> ::core::result::Result<
                    ::ksl_value::value::Value,
                    ::std::string::String,
                > {
                    if args.len() != #count {
                        return Err(format!("参数个数不符: 需要 {} 个", #count));
                    }
                    #(#decodes)*
                    #encode
                },
            )) {
                Ok(result) => result,
                Err(_) => Err("插件函数 panic".to_string()),
            }
        }
    }
}

/// 一个参数的类型.
enum Param {
    Scalar {
        method: &'static str,
        name: &'static str,
    },
    Str,
    String,
    Opaque(Box<Type>),
    Value,
    ValueRef,
}

impl Param {
    fn decode(&self, index: usize) -> TokenStream {
        let arg = quote! { args[#index] };
        match self {
            Self::Scalar { method, name } => {
                let method = syn::Ident::new(method, proc_macro2::Span::call_site());
                quote! {
                    #arg.#method()
                        .ok_or_else(|| format!("参数 {} 应为 {}", #index, #name))?
                }
            },
            Self::Str => quote! {
                #arg.as_str()
                    .ok_or_else(|| format!("参数 {} 应为 string", #index))?
            },
            Self::String => quote! {
                #arg.as_str()
                    .ok_or_else(|| format!("参数 {} 应为 string", #index))?
                    .to_string()
            },
            Self::Opaque(ty) => quote! {
                #arg.as_opaque::<#ty>()
                    .ok_or_else(|| format!("参数 {} 类型不符", #index))?
            },
            Self::Value => quote! { #arg.clone() },
            Self::ValueRef => quote! { &#arg },
        }
    }
}

/// 导出函数的返回类型形态.
enum Return {
    Scalar(&'static str),
    Str,
    String,
    Opaque,
    Value,
    Nil,
    Result(Box<Self>),
}

impl Return {
    fn encode(&self, value: &TokenStream) -> TokenStream {
        match self {
            Self::Scalar(variant) => {
                let variant = syn::Ident::new(variant, proc_macro2::Span::call_site());
                quote! { Ok(::ksl_value::value::Value::#variant(#value)) }
            },
            Self::Str => quote! { Ok(::ksl_value::value::Value::string(#value)) },
            Self::String => quote! { Ok(::ksl_value::value::Value::string(#value.as_str())) },
            Self::Opaque => quote! { Ok(::ksl_value::value::Value::native_opaque(#value)) },
            Self::Value => quote! { Ok(#value) },
            Self::Nil => quote! { Ok(::ksl_value::value::Value::Nil) },
            Self::Result(inner) => {
                let encoded = inner.encode(&quote! { __value });
                quote! {
                    match #value {
                        Ok(__value) => #encoded,
                        Err(message) => Err(message),
                    }
                }
            },
        }
    }
}

fn function_export(
    function: &ItemFn,
    link: String,
    execution: &'static str,
) -> syn::Result<FunctionExport> {
    let ident = function.sig.ident.clone();
    if let Some(FnArg::Receiver(_)) = function.sig.inputs.first() {
        return Err(syn::Error::new_spanned(&function.sig, "插件函数不能带 self"));
    }
    if is_raw_signature(function) {
        return Ok(FunctionExport {
            ident,
            link,
            execution,
            raw: true,
            params: Vec::new(),
            ret: Return::Nil,
        });
    }
    let mut params = Vec::with_capacity(function.sig.inputs.len());
    for input in &function.sig.inputs {
        let FnArg::Typed(pat) = input else {
            return Err(syn::Error::new_spanned(input, "插件函数不能带 self"));
        };
        params.push(parse_param(&pat.ty)?);
    }
    let ret = match &function.sig.output {
        ReturnType::Default => Return::Nil,
        ReturnType::Type(_, ty) => parse_return(ty)?,
    };
    Ok(FunctionExport {
        ident,
        link,
        execution,
        raw: false,
        params,
        ret,
    })
}

fn is_raw_signature(function: &ItemFn) -> bool {
    if function.sig.inputs.len() != 1 {
        return false;
    }
    let Some(FnArg::Typed(pat)) = function.sig.inputs.first() else {
        return false;
    };
    let Type::Reference(reference) = &*pat.ty else {
        return false;
    };
    let Type::Slice(slice) = &*reference.elem else {
        return false;
    };
    is_path(&slice.elem, "Value")
}

fn parse_param(ty: &Type) -> syn::Result<Param> {
    match ty {
        Type::Reference(reference) => {
            if is_path(&reference.elem, "str") {
                return Ok(Param::Str);
            }
            if is_path(&reference.elem, "Value") {
                return Ok(Param::ValueRef);
            }
            if matches!(&*reference.elem, Type::Slice(_)) {
                return Err(syn::Error::new_spanned(ty, "列表参数写成 &Value, 由插件读取"));
            }
            Ok(Param::Opaque(Box::new((*reference.elem).clone())))
        },
        Type::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return Err(syn::Error::new_spanned(ty, "不支持的参数类型"));
            };
            let name = segment.ident.to_string();
            match name.as_str() {
                "i64" => Ok(Param::Scalar {
                    method: "as_i64",
                    name: "i64",
                }),
                "i32" => Ok(Param::Scalar {
                    method: "as_i32",
                    name: "i32",
                }),
                "u8" => Ok(Param::Scalar {
                    method: "as_u8",
                    name: "u8",
                }),
                "u32" => Ok(Param::Scalar {
                    method: "as_u32",
                    name: "u32",
                }),
                "u64" => Ok(Param::Scalar {
                    method: "as_u64",
                    name: "u64",
                }),
                "f32" => Ok(Param::Scalar {
                    method: "as_f32",
                    name: "f32",
                }),
                "f64" => Ok(Param::Scalar {
                    method: "as_f64",
                    name: "f64",
                }),
                "bool" => Ok(Param::Scalar {
                    method: "as_bool",
                    name: "bool",
                }),
                "char" => Ok(Param::Scalar {
                    method: "as_char",
                    name: "char",
                }),
                "String" => Ok(Param::String),
                "Value" => Ok(Param::Value),
                _ => Err(syn::Error::new_spanned(ty, "不透明参数需要写成 &T")),
            }
        },
        _ => Err(syn::Error::new_spanned(ty, "不支持的参数类型")),
    }
}

fn parse_return(ty: &Type) -> syn::Result<Return> {
    if let Some(inner) = result_return(ty)? {
        return Ok(inner);
    }
    match ty {
        Type::Reference(reference) if is_path(&reference.elem, "str") => Ok(Return::Str),
        Type::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return Err(syn::Error::new_spanned(ty, "不支持的返回类型"));
            };
            let name = segment.ident.to_string();
            let scalar = match name.as_str() {
                "i64" => Some("I64"),
                "i32" => Some("I32"),
                "u8" => Some("U8"),
                "u32" => Some("U32"),
                "u64" => Some("U64"),
                "f32" => Some("F32"),
                "f64" => Some("F64"),
                "bool" => Some("Bool"),
                "char" => Some("Char"),
                _ => None,
            };
            if let Some(variant) = scalar {
                return Ok(Return::Scalar(variant));
            }
            if name == "String" {
                return Ok(Return::String);
            }
            if name == "Value" {
                return Ok(Return::Value);
            }
            Ok(Return::Opaque)
        },
        Type::Tuple(tuple) if tuple.elems.is_empty() => Ok(Return::Nil),
        _ => Err(syn::Error::new_spanned(ty, "不支持的返回类型")),
    }
}

fn result_return(ty: &Type) -> syn::Result<Option<Return>> {
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
    let [ok, error] = types.as_slice() else {
        return Err(syn::Error::new_spanned(ty, "Result 需要两个类型参数"));
    };
    if !is_path(error, "String") {
        return Err(syn::Error::new_spanned(error, "Result 的错误类型必须是 String"));
    }
    let inner = parse_return(ok)?;
    if matches!(inner, Return::Result(_)) {
        return Err(syn::Error::new_spanned(ok, "不支持嵌套 Result"));
    }
    Ok(Some(Return::Result(Box::new(inner))))
}

fn is_path(ty: &Type, name: &str) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}

/// 全局值的值构造方式.
enum GlobalCtor {
    Scalar(syn::Ident),
    Str,
    String,
}

fn export_name(attribute: &syn::Attribute, ident: &syn::Ident) -> syn::Result<String> {
    match &attribute.meta {
        Meta::Path(_) => Ok(ident.to_string()),
        Meta::List(list) => Ok(list.parse_args::<LitStr>()?.value()),
        Meta::NameValue(_) => Err(syn::Error::new_spanned(attribute, "导出名写成字符串字面值")),
    }
}

fn global_ctor(ty: &Type) -> syn::Result<GlobalCtor> {
    if let Type::Reference(reference) = ty
        && is_path(&reference.elem, "str")
    {
        return Ok(GlobalCtor::Str);
    }
    let Type::Path(path) = ty else {
        return Err(syn::Error::new_spanned(ty, "不支持的全局值类型"));
    };
    let Some(segment) = path.path.segments.last() else {
        return Err(syn::Error::new_spanned(ty, "不支持的全局值类型"));
    };
    let ctor = match segment.ident.to_string().as_str() {
        "i64" => "I64",
        "i32" => "I32",
        "u8" => "U8",
        "u32" => "U32",
        "u64" => "U64",
        "f32" => "F32",
        "f64" => "F64",
        "bool" => "Bool",
        "char" => "Char",
        "String" => return Ok(GlobalCtor::String),
        _ => return Err(syn::Error::new_spanned(ty, "不支持的全局值类型")),
    };
    let ctor = syn::Ident::new(ctor, segment.ident.span());
    Ok(GlobalCtor::Scalar(ctor))
}

fn take_marker(attrs: &mut Vec<syn::Attribute>, name: &str) -> Option<syn::Attribute> {
    let mut found = None;
    attrs.retain(|attr| {
        if attr.path().is_ident(name) {
            found = Some(attr.clone());
            false
        } else {
            true
        }
    });
    found
}
