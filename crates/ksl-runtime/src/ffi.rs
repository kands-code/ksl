//! 动态库插件加载与调用.
//!
//! 插件导出唯一入口 `ksl_plugin_init`, 返回静态描述符.
//! 宿主按描述符校验 `@Link` 声明, 并按统一 C ABI 调用函数.
//! 字符串在调用边界复制, 不透明句柄由类型释放函数回收.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, resolve};
use ksl_frontend::check::Type;
use ksl_frontend::error::{ErrorKind, RuntimeError as Error};
use ksl_plugin::{
    ABI_VERSION, EXECUTION_CPU, EXECUTION_CURRENT, EXECUTION_IO, FUNCTION_FALLIBLE,
    KslFunctionDef, KslFunctionEntry, KslGlobalDef, KslGlobalEntry, KslPlugin, KslStr, KslType,
    KslValue, THREAD_ANY, THREAD_RUNTIME,
};
use libloading::Library;

use crate::runtime::pump::PumpRegistry;
use crate::value::Value;

mod foreign;

use self::foreign::{FfiType, error_message, value_from_abi};
pub use self::foreign::{ForeignFunction, ForeignHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Execution {
    Current,
    Io,
    Cpu,
}

mod release;

pub(crate) use self::release::{ReleaseDrain, ReleaseQueue};

/// 类型表中的一个条目.
#[derive(Clone, Copy)]
struct PluginType {
    release: Option<unsafe extern "C" fn(u64)>,
    /// 释放入口的线程亲和性, 取 `THREAD_*`.
    release_thread: u32,
}

/// 插件中的一个函数.
#[derive(Clone)]
struct PluginFunction {
    source_name: String,
    params: Vec<FfiType>,
    ret: FfiType,
    fallible: bool,
    entry: KslFunctionEntry,
    execution: Execution,
}

/// 插件中的一个全局值.
#[derive(Clone, Copy)]
struct PluginGlobal {
    ty: FfiType,
    get: KslGlobalEntry,
    execution: Execution,
}

/// 已加载的插件.
pub struct PluginInstance {
    /// 保持动态库存活, 释放函数与函数指针依赖它.
    library: Arc<Library>,
    name: String,
    version: String,
    types: Vec<PluginType>,
    type_indexes: HashMap<String, u32>,
    functions: HashMap<String, PluginFunction>,
    globals: HashMap<String, PluginGlobal>,
    /// 卸载入口; 动态库卸载前调用一次.
    unload: Option<ksl_plugin::KslUnloadEntry>,
    /// 卸载入口的线程亲和性.
    unload_thread: u32,
    /// 调度线程释放队列; 用弱引用避免与队列中的释放任务形成环.
    releases: Weak<ReleaseQueue>,
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
            .field("types", &self.types.len())
            .field("functions", &self.functions.len())
            .field("globals", &self.globals.len())
            .finish_non_exhaustive()
    }
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
        releases: &Arc<ReleaseQueue>,
        pumps: &Arc<PumpRegistry>,
    ) -> Result<Self, Error> {
        let library = unsafe { Library::new(path) }
            .map_err(|error| plugin_load(format!("{}: {error}", path.display()), span))?;
        let library = Arc::new(library);
        let init = unsafe {
            library.get::<unsafe extern "C" fn(u32) -> *const KslPlugin>(b"ksl_plugin_init\0")
        }
        .map_err(|error| plugin_symbol("ksl_plugin_init", &error.to_string(), span))?;
        let init = *init;
        let descriptor = unsafe { init(ABI_VERSION) };
        if descriptor.is_null() {
            return Err(plugin_load("插件拒绝当前 ABI 版本".to_string(), span));
        }
        let descriptor = unsafe { &*descriptor };
        if descriptor.abi_version != ABI_VERSION {
            return Err(Error::runtime(
                ErrorKind::PluginAbiMismatch {
                    expected: ABI_VERSION,
                    found: descriptor.abi_version,
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
        let unload_thread = thread_affinity(descriptor.unload_thread, span)?;

        let (types, type_indexes) = load_types(descriptor, span)?;
        let functions = load_functions(descriptor, span, types.len())?;
        let globals = load_globals(descriptor, span, types.len(), &functions)?;

        if let Some(init) = descriptor.init {
            let run = move || {
                let mut out = KslValue::nil();
                let status = unsafe { init(std::ptr::from_mut(&mut out)) };
                if status != 0 {
                    Err(Error::runtime(
                        ErrorKind::PluginInitFailed {
                            message: error_message(&out),
                        },
                        Some(span),
                    ))
                } else {
                    Ok(Value::Nil)
                }
            };
            run()?;
        }

        let pump = descriptor
            .pump
            .and_then(|entry| pumps.register(Arc::clone(&library), entry));

        Ok(Self {
            library,
            name: found_name,
            version,
            types,
            type_indexes,
            functions,
            globals,
            unload: descriptor.unload,
            unload_thread,
            releases: Arc::downgrade(releases),
            pumps: Arc::clone(pumps),
            pump,
        })
    }

    /// 按名字查找不透明类型下标.
    #[must_use]
    pub fn type_index(&self, name: &str) -> Option<u32> {
        self.type_indexes.get(name).copied()
    }

    /// 由 `@Link` 声明构造函数值或外来全局值.
    ///
    /// # Errors
    ///
    /// 描述符没有该名字, 或声明类型与描述符不符.
    pub(crate) fn make_value(
        self: &Arc<Self>,
        name: &str,
        declared: &Type,
        type_map: &HashMap<Symbol, u32>,
        span: Span,
        releases: &Arc<ReleaseQueue>,
    ) -> Result<Value, Error> {
        if let Some(function) = self.functions.get(name) {
            let Type::Fn(params, ret) = declared else {
                return Err(signature_mismatch(name, span));
            };
            let (ret_declared, fallible) = match result_inner(ret) {
                Some(inner) => (inner, true),
                None => (ret.as_ref(), false),
            };
            if params.len() != function.params.len()
                || fallible != function.fallible
                || params.iter().zip(&function.params).any(|(declared, expected)| {
                    FfiType::from_ksl(declared, type_map) != Some(*expected)
                })
                || FfiType::from_ksl(ret_declared, type_map) != Some(function.ret)
            {
                return Err(signature_mismatch(name, span));
            }
            return Ok(Value::ForeignFn(Arc::new(ForeignFunction {
                name: ArcStr::from(name),
                source_name: ArcStr::from(function.source_name.as_str()),
                plugin: Arc::clone(self),
                declared: declared.clone(),
                params: function.params.clone(),
                ret: function.ret,
                ret_declared: ret_declared.clone(),
                fallible,
                entry: function.entry,
                execution: function.execution,
            })));
        }
        if let Some(global) = self.globals.get(name) {
            if matches!(declared, Type::Fn(..))
                || FfiType::from_ksl(declared, type_map) != Some(global.ty)
            {
                return Err(signature_mismatch(name, span));
            }
            // 全局值只支持当前调用类别, 其余类别在加载时拒绝.
            debug_assert_eq!(global.execution, Execution::Current);
            let mut out = KslValue::nil();
            let status = unsafe { (global.get)(std::ptr::from_mut(&mut out)) };
            if status != 0 {
                return Err(plugin_call(error_message(&out), span));
            }
            let releases = Arc::downgrade(releases);
            return value_from_abi(global.ty, declared, self, &out, span, &releases);
        }
        Err(Error::runtime(
            ErrorKind::PluginMissingExport {
                name: name.to_string(),
            },
            Some(span),
        ))
    }

    /// 类型表声明的释放线程亲和性.
    fn release_thread(&self, type_index: u32) -> u32 {
        self.types
            .get(type_index as usize)
            .map_or(THREAD_ANY, |ty| ty.release_thread)
    }

    /// 释放一个拥有的不透明句柄.
    fn release(&self, type_index: u32, handle: u64) {
        if let Some(ty) = self.types.get(type_index as usize)
            && let Some(release) = ty.release
        {
            unsafe { release(handle) };
        }
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        // 事件泵先注销, 再卸载动态库.
        if let Some(id) = self.pump {
            self.pumps.unregister(id);
        }
        let Some(unload) = self.unload else {
            return;
        };
        // 队列已不存在时运行时正在关闭, 就地执行是唯一选择.
        let Some(queue) = self.releases.upgrade() else {
            unsafe { unload() };
            return;
        };
        if self.unload_thread == THREAD_RUNTIME {
            let library = Arc::clone(&self.library);
            // 卸载入口可能在任意线程触发; 由释放队列交回调度线程.
            queue.push(Box::new(move || {
                unsafe { unload() };
                // 卸载入口执行完才允许动态库真正卸载.
                drop(library);
            }));
        } else {
            unsafe { unload() };
        }
    }
}

/// 读取描述符中的类型表.
fn load_types(
    descriptor: &KslPlugin,
    span: Span,
) -> Result<(Vec<PluginType>, HashMap<String, u32>), Error> {
    let mut types = Vec::with_capacity(descriptor.type_count as usize);
    let mut type_indexes = HashMap::new();
    for (index, ty) in descriptor_types(descriptor, span)?.iter().enumerate() {
        let type_name = read_str(ty.name, span)?;
        let Ok(index) = u32::try_from(index) else {
            return Err(plugin_load("类型数量超出 u32".to_string(), span));
        };
        if type_indexes.insert(type_name.clone(), index).is_some() {
            return Err(Error::runtime(
                ErrorKind::PluginDuplicateType { name: type_name },
                Some(span),
            ));
        }
        let release_thread = thread_affinity(ty.release_thread, span)?;
        types.push(PluginType {
            release: ty.release,
            release_thread,
        });
    }
    Ok((types, type_indexes))
}

/// 读取描述符中的函数表.
fn load_functions(
    descriptor: &KslPlugin,
    span: Span,
    type_count: usize,
) -> Result<HashMap<String, PluginFunction>, Error> {
    let mut functions = HashMap::new();
    for function in descriptor_functions(descriptor, span)? {
        let name = read_str(function.name, span)?;
        let source_name = read_str(function.source_name, span)?;
        let params = read_params(function, type_count, span)?;
        let ret = read_type(function.ret, type_count, span)?;
        let entry = PluginFunction {
            source_name,
            params,
            ret,
            fallible: function.flags & FUNCTION_FALLIBLE != 0,
            entry: function.entry,
            execution: execution_from_abi(function.execution, span)?,
        };
        if functions.insert(name.clone(), entry).is_some() {
            return Err(Error::runtime(
                ErrorKind::PluginDuplicateExport { name },
                Some(span),
            ));
        }
    }
    Ok(functions)
}

/// 读取描述符中的全局值表; 名字与函数表冲突时报错.
fn load_globals(
    descriptor: &KslPlugin,
    span: Span,
    type_count: usize,
    functions: &HashMap<String, PluginFunction>,
) -> Result<HashMap<String, PluginGlobal>, Error> {
    let mut globals = HashMap::new();
    for global in descriptor_globals(descriptor, span)? {
        let name = read_str(global.name, span)?;
        if functions.contains_key(&name) {
            return Err(Error::runtime(
                ErrorKind::PluginDuplicateExport { name },
                Some(span),
            ));
        }
        let _source_name = read_str(global.source_name, span)?;
        let ty = read_type(global.ty, type_count, span)?;
        let execution = execution_from_abi(global.execution, span)?;
        if execution != Execution::Current {
            return Err(plugin_load("全局值只能以当前调用类别读取".to_string(), span));
        }
        if globals
            .insert(
                name.clone(),
                PluginGlobal {
                    ty,
                    get: global.get,
                    execution,
                },
            )
            .is_some()
        {
            return Err(Error::runtime(
                ErrorKind::PluginDuplicateExport { name },
                Some(span),
            ));
        }
    }
    Ok(globals)
}

/// `@Link` 解析后的插件来源.
///
/// `id` 是根相对, 去扩展名的路径, 构成不透明类型的身份;
/// `path` 是规范化绝对路径, 用于加载与缓存.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PluginSource {
    pub(crate) id: String,
    pub(crate) path: PathBuf,
}

/// 过程内共享的动态库缓存.
#[derive(Clone, Default)]
pub struct LinkCache {
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
        releases: &Arc<ReleaseQueue>,
        pumps: &Arc<PumpRegistry>,
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
        let plugin = Arc::new(PluginInstance::load(name, &source.path, span, releases, pumps)?);
        self.plugins.insert(source.path.clone(), Arc::clone(&plugin));
        Ok(plugin)
    }
}

/// 解析执行类别.
fn execution_from_abi(value: u32, span: Span) -> Result<Execution, Error> {
    match value {
        EXECUTION_CURRENT => Ok(Execution::Current),
        EXECUTION_IO => Ok(Execution::Io),
        EXECUTION_CPU => Ok(Execution::Cpu),
        other => Err(plugin_load(format!("未知的插件执行类别: {other}"), span)),
    }
}

/// 解析线程亲和性; 当前只接受 `THREAD_ANY` 与 `THREAD_RUNTIME`.
fn thread_affinity(value: u32, span: Span) -> Result<u32, Error> {
    match value {
        THREAD_ANY | THREAD_RUNTIME => Ok(value),
        other => Err(plugin_load(format!("未知的插件线程亲和性: {other}"), span)),
    }
}

/// 取 `Result[T, string]` 的内层类型 `T`; 不是该形态时返回 None.
fn result_inner(ty: &Type) -> Option<&Type> {
    let Type::App { name, args } = ty else {
        return None;
    };
    if &*resolve(*name) != "Result" || args.len() != 2 || args[1] != Type::Str {
        return None;
    }
    Some(&args[0])
}

fn read_str(value: KslStr, span: Span) -> Result<String, Error> {
    unsafe { value.as_str() }
        .map(str::to_string)
        .ok_or_else(|| plugin_load("插件描述符包含非法 UTF-8".to_string(), span))
}

fn read_type(ty: KslType, count: usize, span: Span) -> Result<FfiType, Error> {
    let ty = FfiType::from_abi(ty)
        .ok_or_else(|| plugin_load(format!("未知类型标签 {}", ty.tag), span))?;
    if let FfiType::Opaque(index) = ty
        && index as usize >= count
    {
        return Err(plugin_load(format!("不透明类型下标 {index} 超出类型表"), span));
    }
    Ok(ty)
}

fn read_params(
    function: &KslFunctionDef,
    count: usize,
    span: Span,
) -> Result<Vec<FfiType>, Error> {
    if function.param_count == 0 {
        return Ok(Vec::new());
    }
    if function.params.is_null() {
        return Err(plugin_load("函数参数指针为空".to_string(), span));
    }
    let params =
        unsafe { std::slice::from_raw_parts(function.params, function.param_count as usize) };
    params.iter().map(|ty| read_type(*ty, count, span)).collect()
}

fn descriptor_types(
    plugin: &KslPlugin,
    span: Span,
) -> Result<&[ksl_plugin::KslOpaqueType], Error> {
    if plugin.type_count == 0 {
        return Ok(&[]);
    }
    if plugin.types.is_null() {
        return Err(plugin_load("类型表指针为空".to_string(), span));
    }
    Ok(unsafe { std::slice::from_raw_parts(plugin.types, plugin.type_count as usize) })
}

fn descriptor_functions(plugin: &KslPlugin, span: Span) -> Result<&[KslFunctionDef], Error> {
    if plugin.function_count == 0 {
        return Ok(&[]);
    }
    if plugin.functions.is_null() {
        return Err(plugin_load("函数表指针为空".to_string(), span));
    }
    Ok(unsafe { std::slice::from_raw_parts(plugin.functions, plugin.function_count as usize) })
}

fn descriptor_globals(plugin: &KslPlugin, span: Span) -> Result<&[KslGlobalDef], Error> {
    if plugin.global_count == 0 {
        return Ok(&[]);
    }
    if plugin.globals.is_null() {
        return Err(plugin_load("全局值表指针为空".to_string(), span));
    }
    Ok(unsafe { std::slice::from_raw_parts(plugin.globals, plugin.global_count as usize) })
}

/// 平台约定的插件文件名; 名字不合法时返回 None.
///
/// 名字是 `plugin/` 下的裸文件名, 不含路径分隔符与扩展名.
pub(crate) fn plugin_file(name: &str) -> Option<String> {
    let invalid = name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(|ch| matches!(ch, '/' | '\\' | '\0'));
    if invalid {
        return None;
    }
    let file = if cfg!(target_os = "windows") {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    };
    Some(file)
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

fn plugin_call(message: impl Into<String>, span: Span) -> Error {
    Error::runtime(
        ErrorKind::PluginCallFailed {
            message: message.into(),
        },
        Some(span),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use crossbeam_channel::bounded;

    use super::ReleaseQueue;
    use crate::runtime::event::Event;

    #[test]
    fn push_without_route_runs_inline() {
        let (queue, _drain) = ReleaseQueue::new();
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        queue.push(Box::new(move || {
            flag.store(true, Ordering::SeqCst);
        }));
        assert!(ran.load(Ordering::SeqCst), "未附着调度线程时就地执行");
    }

    #[test]
    fn push_on_runtime_thread_runs_inline() {
        let (events, _inbox) = crossbeam_channel::unbounded();
        let (queue, _drain) = ReleaseQueue::new();
        queue.attach(events);
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        queue.push(Box::new(move || flag.store(true, Ordering::SeqCst)));
        assert!(ran.load(Ordering::SeqCst), "调度线程上的释放应就地执行");
    }

    #[test]
    fn push_from_other_thread_waits_for_drain() {
        let (events, inbox) = crossbeam_channel::unbounded();
        let (queue, drain) = ReleaseQueue::new();
        queue.attach(events);
        let queue = Arc::new(queue);
        let (reply, response) = bounded(1);
        let queued = Arc::clone(&queue);
        std::thread::spawn(move || {
            queued.push(Box::new(move || {
                let _ = reply.send(std::thread::current().id());
            }));
        })
        .join()
        .expect("投递线程应正常结束");
        assert!(matches!(inbox.try_recv(), Ok(Event::Wake)), "投递应唤醒调度器");
        assert!(response.try_recv().is_err(), "排空前不应执行");
        for job in drain.drain() {
            job();
        }
        assert_eq!(
            response.recv().expect("释放应执行"),
            std::thread::current().id(),
            "排空应由调度线程执行"
        );
    }

    #[test]
    fn dropping_sender_on_worker_keeps_jobs_for_drain() {
        let (events, _inbox) = crossbeam_channel::unbounded();
        let (queue, drain) = ReleaseQueue::new();
        queue.attach(events);
        let queue = Arc::new(queue);
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let queued = Arc::clone(&queue);
        let (pushed, pushed_rx) = bounded(1);
        let (release, release_rx) = bounded(1);
        let worker = std::thread::spawn(move || {
            queued.push(Box::new(move || {
                flag.store(true, Ordering::SeqCst);
            }));
            let _ = pushed.send(());
            let _ = release_rx.recv();
            drop(queued);
        });
        pushed_rx.recv().expect("投递应完成");
        // 调度线程先释放投递端, 让工作线程成为最后一个持有者.
        drop(queue);
        let _ = release.send(());
        worker.join().expect("投递线程应正常结束");
        assert!(!ran.load(Ordering::SeqCst), "工作线程不得执行释放任务");
        for job in drain.drain() {
            job();
        }
        assert!(ran.load(Ordering::SeqCst), "调度线程排空后执行");
    }

    #[test]
    fn dropping_receiver_runs_pending_jobs() {
        let (events, _inbox) = crossbeam_channel::unbounded();
        let (queue, drain) = ReleaseQueue::new();
        queue.attach(events);
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        std::thread::spawn(move || {
            queue.push(Box::new(move || {
                flag.store(true, Ordering::SeqCst);
            }));
        })
        .join()
        .expect("投递线程应正常结束");
        drop(drain);
        assert!(ran.load(Ordering::SeqCst), "接收端析构时应执行剩余任务");
    }
}
