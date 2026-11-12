//! 原生插件加载与缓存.
//!
//! 描述符校验集中在本文件; 动态库保持映射到宿主结束, 插件实例随运行缓存释放.
//! 调用与错误映射由 `ksl-runtime` 负责.

use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ksl_base::plugin::PluginSource;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, RuntimeError as Error};
use ksl_runtime::PumpRegistry;
use ksl_value::ffi::{Execution, ForeignFunction};
use ksl_value::native::{
    KslStr, NATIVE_ABI_VERSION, NativeEntry, NativeGlobalEntry, NativePlugin, NativeUnloadEntry,
};
use ksl_value::value::Value;
use libloading::Library;

/// 插件中的一个函数.
struct PluginFunction {
    source_name: String,
    entry: NativeEntry,
    execution: Execution,
}

/// 插件中的一个全局值.
struct PluginGlobal {
    get: NativeGlobalEntry,
}

/// 已加载插件库的保活表.
///
/// 生命周期跟随宿主, 不随交互会话或模块根目录重置. 插件实例卸载后代码仍保持映射,
/// 仍存活的 opaque 值可以安全地调用其析构与类型名.
#[derive(Default)]
pub(crate) struct LibraryKeeper {
    libraries: HashMap<PathBuf, Arc<Library>>,
}

impl LibraryKeeper {
    /// 记录规范化路径对应的库; 同一路径只保留一份句柄.
    pub(crate) fn keep(&mut self, path: &Path, library: &Arc<Library>) {
        self.libraries
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::clone(library));
    }
}

/// 已加载的插件.
pub(crate) struct PluginInstance {
    /// 保持动态库存活, 函数指针依赖它.
    _library: Arc<Library>,
    name: String,
    version: String,
    type_indexes: HashMap<String, u32>,
    functions: HashMap<String, PluginFunction>,
    globals: HashMap<String, PluginGlobal>,
    /// 卸载入口; 动态库卸载前调用一次.
    unload: Option<NativeUnloadEntry>,
    /// 事件泵注册表; 释放时注销.
    pumps: Arc<PumpRegistry>,
    /// 已注册事件泵的编号.
    pump: Option<u64>,
}

impl fmt::Debug for PluginInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PluginInstance")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("types", &self.type_indexes.len())
            .field("functions", &self.functions.len())
            .field("globals", &self.globals.len())
            .finish_non_exhaustive()
    }
}

fn load_library(path: &Path, span: Span) -> Result<Arc<Library>, Error> {
    let library = unsafe { Library::new(path) }
        .map_err(|error| plugin_load(format!("{}: {error}", path.display()), span))?;
    Ok(Arc::new(library))
}

/// 读取并校验插件描述符.
fn load_descriptor(
    library: &Library,
    name: &str,
    span: Span,
) -> Result<(&'static NativePlugin, String), Error> {
    let entry = unsafe {
        library.get::<extern "C" fn() -> *const NativePlugin>(b"ksl_native_plugin\0")
    }
    .map_err(|error| plugin_symbol("ksl_native_plugin", &error.to_string(), span))?;
    let descriptor = (*entry)();
    if descriptor.is_null() {
        return Err(plugin_load("原生插件描述符为空".to_string(), span));
    }
    // 描述符由插件静态持有, 生命周期不短于动态库.
    let descriptor = unsafe { &*descriptor };
    if descriptor.abi != NATIVE_ABI_VERSION {
        return Err(Error::runtime(
            ErrorKind::PluginAbiMismatch {
                expected: NATIVE_ABI_VERSION,
                found: descriptor.abi,
            },
            Some(span),
        ));
    }
    let found_name = read_str(descriptor.name, span)?;
    if found_name != name {
        return Err(Error::runtime(
            ErrorKind::PluginNameMismatch {
                expected: name.to_string(),
                found: found_name,
            },
            Some(span),
        ));
    }
    let version = read_str(descriptor.version, span)?;
    Ok((descriptor, version))
}

fn read_types(descriptor: &NativePlugin, span: Span) -> Result<HashMap<String, u32>, Error> {
    let types = table(descriptor.types, descriptor.type_count, "类型表", span)?;
    let mut type_indexes = HashMap::new();
    for (index, ty) in types.iter().enumerate() {
        let type_name = read_str(ty.name, span)?;
        let index = u32::try_from(index)
            .map_err(|_| plugin_load("类型数量超出 u32 上限".to_string(), span))?;
        if type_indexes.insert(type_name.clone(), index).is_some() {
            return Err(Error::runtime(
                ErrorKind::PluginDuplicateType { name: type_name },
                Some(span),
            ));
        }
    }
    Ok(type_indexes)
}

fn read_functions(
    descriptor: &NativePlugin,
    span: Span,
) -> Result<HashMap<String, PluginFunction>, Error> {
    let functions = table(descriptor.functions, descriptor.function_count, "函数表", span)?;
    let mut loaded = HashMap::new();
    for function in functions {
        let name = read_str(function.name, span)?;
        let source_name = read_str(function.source_name, span)?;
        let execution = Execution::from_abi(function.execution).ok_or_else(|| {
            plugin_load(format!("未知的插件执行类别: {}", function.execution), span)
        })?;
        let function = PluginFunction {
            source_name,
            entry: function.entry,
            execution,
        };
        if loaded.insert(name.clone(), function).is_some() {
            return Err(Error::runtime(
                ErrorKind::PluginDuplicateExport { name },
                Some(span),
            ));
        }
    }
    Ok(loaded)
}

fn read_globals(
    descriptor: &NativePlugin,
    functions: &HashMap<String, PluginFunction>,
    span: Span,
) -> Result<HashMap<String, PluginGlobal>, Error> {
    let globals = table(descriptor.globals, descriptor.global_count, "全局值表", span)?;
    let mut loaded = HashMap::new();
    for global in globals {
        let name = read_str(global.name, span)?;
        let _source_name = read_str(global.source_name, span)?;
        if functions.contains_key(&name) || loaded.contains_key(&name) {
            return Err(Error::runtime(
                ErrorKind::PluginDuplicateExport { name },
                Some(span),
            ));
        }
        loaded.insert(name, PluginGlobal { get: global.get });
    }
    Ok(loaded)
}

/// 描述符中的静态表; 计数为 0 时允许空指针.
fn table<T>(ptr: *const T, count: u32, what: &str, span: Span) -> Result<&'static [T], Error> {
    if count == 0 {
        return Ok(&[]);
    }
    if ptr.is_null() {
        return Err(plugin_load(format!("原生插件{what}为空指针"), span));
    }
    Ok(unsafe { std::slice::from_raw_parts(ptr, count as usize) })
}

impl PluginInstance {
    /// 加载动态库并校验描述符.
    ///
    /// # Errors
    ///
    /// 动态库缺失, 入口缺失, ABI 不符或描述符非法时返回运行时错误.
    fn load(
        name: &str,
        path: &Path,
        span: Span,
        pumps: &Arc<PumpRegistry>,
        libraries: &mut LibraryKeeper,
    ) -> Result<Self, Error> {
        let library = load_library(path, span)?;
        let (descriptor, version) = load_descriptor(&library, name, span)?;
        let type_indexes = read_types(descriptor, span)?;
        let functions = read_functions(descriptor, span)?;
        let globals = read_globals(descriptor, &functions, span)?;

        if let Some(init) = descriptor.init {
            init().map_err(|message| {
                Error::runtime(ErrorKind::PluginInitFailed { message }, Some(span))
            })?;
        }

        let pump = descriptor.pump.and_then(|entry| {
            pumps.register_native(Arc::clone(&library) as Arc<dyn Any + Send + Sync>, entry)
        });
        libraries.keep(path, &library);
        Ok(Self {
            _library: library,
            name: name.to_string(),
            version,
            type_indexes,
            functions,
            globals,
            unload: descriptor.unload,
            pumps: Arc::clone(pumps),
            pump,
        })
    }

    /// 按名字查找不透明类型下标.
    #[must_use]
    pub(crate) fn type_index(&self, name: &str) -> Option<u32> {
        self.type_indexes.get(name).copied()
    }

    /// 由 `@Link` 声明构造函数值或全局值.
    ///
    /// # Errors
    ///
    /// 描述符没有该名字, 或声明的类型不是函数/全局值.
    pub(crate) fn make_value(
        self: &Arc<Self>,
        name: &str,
        declared: &Type,
        span: Span,
    ) -> Result<Value, Error> {
        if let Some(function) = self.functions.get(name) {
            if !matches!(declared, Type::Fn(..)) {
                return Err(signature_mismatch(name, span));
            }
            return Ok(Value::ForeignFn(Arc::new(ForeignFunction::new(
                name,
                &function.source_name,
                Arc::clone(self) as Arc<dyn Any + Send + Sync>,
                declared,
                function.entry,
                function.execution,
            ))));
        }
        if let Some(global) = self.globals.get(name) {
            if matches!(declared, Type::Fn(..)) {
                return Err(signature_mismatch(name, span));
            }
            return Ok((global.get)());
        }
        Err(Error::runtime(
            ErrorKind::PluginMissingExport {
                name: name.to_string(),
            },
            Some(span),
        ))
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        // 事件泵先注销, 再调用卸载入口.
        if let Some(id) = self.pump {
            self.pumps.unregister(id);
        }
        if let Some(unload) = self.unload {
            unload();
        }
    }
}

/// 过程内共享的动态库缓存.
#[derive(Clone, Default)]
pub(crate) struct LinkCache {
    plugins: HashMap<PathBuf, Arc<PluginInstance>>,
}

impl fmt::Debug for LinkCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkCache")
            .field("plugins", &self.plugins.len())
            .finish()
    }
}

impl LinkCache {
    /// 按插件名加载动态库; 同一规范路径只加载一次.
    ///
    /// 插件位于模块目录的 `plugin` 子目录, 文件名为平台约定.
    ///
    /// # Errors
    ///
    /// 插件名非法, 文件缺失, 入口缺失或描述符非法时返回运行时错误.
    pub(crate) fn load(
        &mut self,
        source: &PluginSource,
        name: &str,
        span: Span,
        pumps: &Arc<PumpRegistry>,
        libraries: &mut LibraryKeeper,
    ) -> Result<Arc<PluginInstance>, Error> {
        if let Some(plugin) = self.plugins.get(&source.path) {
            // 同一路径可能以另一个名字声明; 描述符名字必须与声明一致.
            if plugin.name != name {
                return Err(Error::runtime(
                    ErrorKind::PluginNameMismatch {
                        expected: name.to_string(),
                        found: plugin.name.clone(),
                    },
                    Some(span),
                ));
            }
            return Ok(Arc::clone(plugin));
        }
        let plugin = Arc::new(PluginInstance::load(
            name,
            &source.path,
            span,
            pumps,
            libraries,
        )?);
        self.plugins.insert(source.path.clone(), Arc::clone(&plugin));
        Ok(plugin)
    }
}

fn read_str(value: KslStr, span: Span) -> Result<String, Error> {
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| plugin_load("插件描述符包含非法 UTF-8".to_string(), span))
}

fn signature_mismatch(name: &str, span: Span) -> Error {
    Error::runtime(
        ErrorKind::PluginSignatureMismatch {
            name: name.to_string(),
        },
        Some(span),
    )
}

fn plugin_load(message: String, span: Span) -> Error {
    Error::runtime(ErrorKind::PluginLoadFailed { message }, Some(span))
}

fn plugin_symbol(name: &str, message: &str, span: Span) -> Error {
    Error::runtime(
        ErrorKind::PluginSymbolMissing {
            name: format!("{name}: {message}"),
        },
        Some(span),
    )
}
