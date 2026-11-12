//! ksl 插件 ABI.
//!
//! 插件编译为动态库, 导出唯一的入口 `ksl_plugin_init`.
//! 入口返回静态描述符, 列出插件提供的类型, 函数与全局值.
//! 宿主按描述符校验 `@Link` 声明, 并按统一的 C ABI 调用函数.
//!
//! 值边界只包含标量, 字符串, `nil` 与不透明句柄.
//! 字符串是 UTF-8 字节加长度, 允许内嵌 NUL.
//! 不透明句柄由插件定义, 默认是装箱值的指针; 拥有句柄在最后一个引用消失时调用类型的释放函数.
//! 初始化与卸载入口可选. 卸载入口声明线程亲和性, 初始化入口不接受类别声明.

#![deny(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;

pub use ksl_macros::ksl_plugin;

/// 当前 ABI 版本.
pub const ABI_VERSION: u32 = 1;

/// 调用成功.
pub const STATUS_OK: u32 = 0;
/// 业务失败; `out` 中放 UTF-8 错误信息, 可映射为 `Result/Err[string]`.
pub const STATUS_ERROR: u32 = 1;
/// 插件内部错误或 panic; 宿主报运行时错误.
pub const STATUS_INTERNAL: u32 = 2;

/// 函数声明为可恢复失败, 即返回 `Result[T, string]`.
pub const FUNCTION_FALLIBLE: u32 = 1;

/// 在调度线程内联调用.
pub const EXECUTION_CURRENT: u32 = 0;
/// 在 I/O 阻塞线程池中调用, 当前 Actor 挂起等待.
pub const EXECUTION_IO: u32 = 1;
/// 在 CPU 线程池中调用, 当前 Actor 挂起等待.
pub const EXECUTION_CPU: u32 = 2;

/// 生命周期入口没有线程亲和性.
pub const THREAD_ANY: u32 = 0;
/// 生命周期入口必须在运行时线程执行; CLI 中该线程即进程主线程.
pub const THREAD_RUNTIME: u32 = 1;

// 类型标签.
pub const TYPE_I64: u32 = 1;
pub const TYPE_U64: u32 = 2;
pub const TYPE_U8: u32 = 3;
pub const TYPE_F64: u32 = 4;
pub const TYPE_BOOL: u32 = 5;
pub const TYPE_STRING: u32 = 6;
pub const TYPE_NIL: u32 = 7;
pub const TYPE_OPAQUE: u32 = 8;
pub const TYPE_CHAR: u32 = 9;

// 值标签.
pub const VALUE_NIL: u32 = 0;
pub const VALUE_I64: u32 = 1;
pub const VALUE_U64: u32 = 2;
pub const VALUE_U8: u32 = 3;
pub const VALUE_F64: u32 = 4;
pub const VALUE_BOOL: u32 = 5;
pub const VALUE_STRING: u32 = 6;
pub const VALUE_OPAQUE: u32 = 7;
pub const VALUE_CHAR: u32 = 8;

/// UTF-8 字节切片; 不要求 NUL 结尾.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KslStr {
    pub ptr: *const u8,
    pub len: usize,
}

impl KslStr {
    /// 由静态字符串构造; 字节在进程生命周期内有效.
    #[must_use]
    pub const fn from_static(value: &'static str) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }

    /// 读取为字符串切片.
    ///
    /// # Safety
    ///
    /// `ptr` 必须指向至少 `len` 字节的有效内存, 且在返回切片的生命周期内保持有效.
    #[must_use]
    #[allow(clippy::needless_lifetimes)]
    pub unsafe fn as_str<'a>(&self) -> Option<&'a str> {
        if self.len == 0 {
            return Some("");
        }
        if self.ptr.is_null() {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes).ok()
    }
}

impl std::fmt::Debug for KslStr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match unsafe { self.as_str() } {
            Some(value) => f.debug_tuple("KslStr").field(&value).finish(),
            None => f.debug_tuple("KslStr").field(&self.len).finish(),
        }
    }
}

// 描述符由插件静态分配, 只读, 可跨线程共享.
unsafe impl Send for KslStr {}
unsafe impl Sync for KslStr {}

/// 一个参数或返回值的类型.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KslType {
    /// 类型标签, 取 `TYPE_*`.
    pub tag: u32,
    /// 不透明类型在类型表中的下标; 其他标签忽略.
    pub opaque: u32,
}

impl KslType {
    #[must_use]
    pub const fn i64() -> Self {
        Self {
            tag: TYPE_I64,
            opaque: 0,
        }
    }

    #[must_use]
    pub const fn u64() -> Self {
        Self {
            tag: TYPE_U64,
            opaque: 0,
        }
    }

    #[must_use]
    pub const fn u8() -> Self {
        Self {
            tag: TYPE_U8,
            opaque: 0,
        }
    }

    #[must_use]
    pub const fn f64() -> Self {
        Self {
            tag: TYPE_F64,
            opaque: 0,
        }
    }

    #[must_use]
    pub const fn bool() -> Self {
        Self {
            tag: TYPE_BOOL,
            opaque: 0,
        }
    }

    #[must_use]
    pub const fn string() -> Self {
        Self {
            tag: TYPE_STRING,
            opaque: 0,
        }
    }

    #[must_use]
    pub const fn nil() -> Self {
        Self {
            tag: TYPE_NIL,
            opaque: 0,
        }
    }

    #[must_use]
    pub const fn opaque(index: u32) -> Self {
        Self {
            tag: TYPE_OPAQUE,
            opaque: index,
        }
    }

    #[must_use]
    pub const fn char() -> Self {
        Self {
            tag: TYPE_CHAR,
            opaque: 0,
        }
    }
}

/// 字符串值; 字节由写入方持有.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KslString {
    pub ptr: *const u8,
    pub len: usize,
}

/// 不透明句柄值.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KslOpaqueValue {
    /// 不透明类型在类型表中的下标.
    pub type_index: u32,
    /// 是否为宿主拥有的句柄; 0 表示借用, 1 表示拥有.
    pub owned: u32,
    pub handle: u64,
}

/// 值的负载.
#[repr(C)]
#[derive(Clone, Copy)]
pub union KslValueData {
    pub as_i64: i64,
    pub as_u64: u64,
    pub as_u8: u8,
    pub as_f64: f64,
    pub as_bool: u8,
    pub as_char: u32,
    pub as_string: KslString,
    pub as_opaque: KslOpaqueValue,
}

/// 一个插件值.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KslValue {
    /// 值标签, 取 `VALUE_*`.
    pub tag: u32,
    pub data: KslValueData,
}

impl KslValue {
    #[must_use]
    pub const fn nil() -> Self {
        Self {
            tag: VALUE_NIL,
            data: KslValueData { as_u64: 0 },
        }
    }

    #[must_use]
    pub const fn i64(value: i64) -> Self {
        Self {
            tag: VALUE_I64,
            data: KslValueData { as_i64: value },
        }
    }

    #[must_use]
    pub const fn u64(value: u64) -> Self {
        Self {
            tag: VALUE_U64,
            data: KslValueData { as_u64: value },
        }
    }

    #[must_use]
    pub const fn u8(value: u8) -> Self {
        Self {
            tag: VALUE_U8,
            data: KslValueData { as_u8: value },
        }
    }

    #[must_use]
    pub const fn f64(value: f64) -> Self {
        Self {
            tag: VALUE_F64,
            data: KslValueData { as_f64: value },
        }
    }

    #[must_use]
    pub const fn bool(value: bool) -> Self {
        Self {
            tag: VALUE_BOOL,
            data: KslValueData {
                as_bool: if value { 1 } else { 0 },
            },
        }
    }

    /// 构造返回值字符串; 字节复制到线程暂存区, 下一次插件入口开始时失效.
    #[must_use]
    pub fn string(value: &str) -> Self {
        let string = copy_to_scratch(value);
        Self {
            tag: VALUE_STRING,
            data: KslValueData { as_string: string },
        }
    }

    /// 构造借用字符串; 字节由调用方持有, 必须在接收方读取前保持有效.
    ///
    /// 宿主传入实参时使用, 避免多个字符串共用暂存区互相覆盖.
    #[must_use]
    pub fn string_borrowed(value: &str) -> Self {
        Self {
            tag: VALUE_STRING,
            data: KslValueData {
                as_string: KslString {
                    ptr: value.as_ptr(),
                    len: value.len(),
                },
            },
        }
    }

    #[must_use]
    pub const fn char(value: char) -> Self {
        Self {
            tag: VALUE_CHAR,
            data: KslValueData {
                as_char: value as u32,
            },
        }
    }

    #[must_use]
    pub const fn opaque(type_index: u32, handle: u64, owned: bool) -> Self {
        Self {
            tag: VALUE_OPAQUE,
            data: KslValueData {
                as_opaque: KslOpaqueValue {
                    type_index,
                    owned: if owned { 1 } else { 0 },
                    handle,
                },
            },
        }
    }
}

impl std::fmt::Debug for KslValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.tag {
            VALUE_NIL => f.write_str("nil"),
            VALUE_I64 => write!(f, "{}", unsafe { self.data.as_i64 }),
            VALUE_U64 => write!(f, "{}", unsafe { self.data.as_u64 }),
            VALUE_U8 => write!(f, "{}", unsafe { self.data.as_u8 }),
            VALUE_F64 => write!(f, "{}", unsafe { self.data.as_f64 }),
            VALUE_BOOL => write!(f, "{}", unsafe { self.data.as_bool } != 0),
            VALUE_CHAR => match char::from_u32(unsafe { self.data.as_char }) {
                Some(value) => write!(f, "{value:?}"),
                None => f.write_str("<invalid char>"),
            },
            VALUE_STRING => match unsafe { self.data.as_string.as_str() } {
                Some(value) => write!(f, "{value:?}"),
                None => f.write_str("<invalid string>"),
            },
            VALUE_OPAQUE => {
                let opaque = unsafe { self.data.as_opaque };
                write!(
                    f,
                    "<opaque {}{}:{}>",
                    opaque.type_index,
                    if opaque.owned != 0 { "+" } else { "-" },
                    opaque.handle
                )
            },
            other => write!(f, "<unknown value tag {other}>"),
        }
    }
}

impl KslString {
    /// 读取为字符串切片.
    ///
    /// # Safety
    ///
    /// `ptr` 必须指向至少 `len` 字节的有效 UTF-8 内存.
    #[must_use]
    #[allow(clippy::needless_lifetimes)]
    pub unsafe fn as_str<'a>(&self) -> Option<&'a str> {
        if self.len == 0 {
            return Some("");
        }
        if self.ptr.is_null() {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr, self.len) };
        std::str::from_utf8(bytes).ok()
    }
}

/// 插件中的一个不透明类型.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KslOpaqueType {
    pub name: KslStr,
    /// 释放句柄; `None` 表示借用句柄, 宿主不负责释放.
    pub release: Option<unsafe extern "C" fn(u64)>,
    /// 释放入口的线程亲和性, 取 `THREAD_*`.
    pub release_thread: u32,
}

unsafe impl Send for KslOpaqueType {}
unsafe impl Sync for KslOpaqueType {}

/// 插件函数入口.
///
/// 返回 [`STATUS_OK`], [`STATUS_ERROR`] 或 [`STATUS_INTERNAL`].
/// 非成功状态时 `out` 中放 UTF-8 错误信息字符串.
pub type KslFunctionEntry =
    unsafe extern "C" fn(args: *const KslValue, argc: u32, out: *mut KslValue) -> u32;

/// 插件全局值入口; 返回约定与函数入口相同.
pub type KslGlobalEntry = unsafe extern "C" fn(out: *mut KslValue) -> u32;

/// 插件初始化入口; 失败时把错误信息写入 `out`.
pub type KslInitEntry = unsafe extern "C" fn(out: *mut KslValue) -> u32;

/// 插件卸载入口; 不接受参数, 不返回状态.
pub type KslUnloadEntry = unsafe extern "C" fn();

/// 事件泵; 每次调用处理一批平台事件后立即返回.
pub type KslEventPumpEntry = unsafe extern "C" fn();

/// 插件函数描述.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KslFunctionDef {
    /// ksl 链接名; 对应 `@Link` 中的名字.
    pub name: KslStr,
    /// 插件源名; 对应 Rust 函数名, 供诊断与工具使用.
    pub source_name: KslStr,
    pub params: *const KslType,
    pub param_count: u32,
    /// 函数标志, 取 `FUNCTION_*`.
    pub flags: u32,
    pub ret: KslType,
    pub entry: KslFunctionEntry,
    /// 执行类别, 取 `EXECUTION_*`.
    pub execution: u32,
}

unsafe impl Send for KslFunctionDef {}
unsafe impl Sync for KslFunctionDef {}

/// 插件全局值描述.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KslGlobalDef {
    /// ksl 链接名; 对应 `@Link` 中的名字.
    pub name: KslStr,
    /// 插件源名; 对应 Rust 常量或静态项名.
    pub source_name: KslStr,
    pub ty: KslType,
    pub get: KslGlobalEntry,
    /// 读取类别, 当前只支持 `EXECUTION_CURRENT`.
    pub execution: u32,
}

unsafe impl Send for KslGlobalDef {}
unsafe impl Sync for KslGlobalDef {}

/// 插件描述符; 由插件静态分配, 生命周期与动态库一致.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KslPlugin {
    pub abi_version: u32,
    pub name: KslStr,
    pub version: KslStr,
    pub types: *const KslOpaqueType,
    pub type_count: u32,
    pub functions: *const KslFunctionDef,
    pub function_count: u32,
    pub globals: *const KslGlobalDef,
    pub global_count: u32,
    /// 初始化入口; 描述符校验通过后调用一次.
    ///
    /// 初始化在模块加载时执行, 加载发生在调度线程上; 宿主负责在线程亲和性满足的位置驱动运行时.
    pub init: Option<KslInitEntry>,
    /// 卸载入口; 动态库卸载前调用一次.
    pub unload: Option<KslUnloadEntry>,
    /// 卸载入口的线程亲和性, 取 `THREAD_*`.
    pub unload_thread: u32,
    /// 事件泵; 由调度循环在片段之间调用.
    pub pump: Option<KslEventPumpEntry>,
}

unsafe impl Send for KslPlugin {}
unsafe impl Sync for KslPlugin {}

/// 不透明值类型.
///
/// 默认句柄是 `Box<Self>` 的指针, 释放时装箱回收.
/// 句柄必须始终指向有效的 `Self`, 借用参数按此约定直接解引用.
/// 需要自定义所有权时覆写 `into_handle` 与 `release`, 并保持同一指针约定.
pub trait KslOpaque: Sized + 'static {
    /// 暴露给 ksl 的类型名.
    const NAME: &'static str;

    /// 释放入口的线程亲和性, 取 `THREAD_ANY` 或 `THREAD_RUNTIME`.
    const RELEASE_THREAD: u32 = THREAD_ANY;

    /// 把值转为句柄.
    fn into_handle(self) -> u64 {
        Box::into_raw(Box::new(self)) as u64
    }

    /// 释放句柄.
    ///
    /// # Safety
    ///
    /// `handle` 必须来自同一类型的 `into_handle`, 且只能释放一次.
    unsafe fn release(handle: u64) {
        unsafe {
            drop(Box::from_raw(handle as *mut Self));
        }
    }
}

thread_local! {
    /// 返回值字符串的暂存区.
    ///
    /// 宿主在调用返回后立即复制, 下一次插件入口开始时清空.
    static RETURN_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn copy_to_scratch(value: &str) -> KslString {
    RETURN_BUFFER.with(|buffer| {
        let mut buffer = buffer.borrow_mut();
        buffer.clear();
        buffer.extend_from_slice(value.as_bytes());
        KslString {
            ptr: buffer.as_ptr(),
            len: buffer.len(),
        }
    })
}

/// 值的读取与构造辅助.
///
/// 读取函数在标签不符时返回说明文本; 字符串与字符另行校验 UTF-8 与标量值.
pub mod value {
    use super::{
        KslOpaqueValue, KslString, KslValue, VALUE_BOOL, VALUE_CHAR, VALUE_F64, VALUE_I64,
        VALUE_NIL, VALUE_OPAQUE, VALUE_STRING, VALUE_U8, VALUE_U64,
    };

    /// 读取 i64.
    ///
    /// # Errors
    ///
    /// 标签不符时返回说明文本.
    pub fn as_i64(value: &KslValue) -> Result<i64, String> {
        if value.tag != VALUE_I64 {
            return Err("期望 i64".to_string());
        }
        Ok(unsafe { value.data.as_i64 })
    }

    /// 读取 u64.
    ///
    /// # Errors
    ///
    /// 标签不符时返回说明文本.
    pub fn as_u64(value: &KslValue) -> Result<u64, String> {
        if value.tag != VALUE_U64 {
            return Err("期望 u64".to_string());
        }
        Ok(unsafe { value.data.as_u64 })
    }

    /// 读取 u8.
    ///
    /// # Errors
    ///
    /// 标签不符时返回说明文本.
    pub fn as_u8(value: &KslValue) -> Result<u8, String> {
        if value.tag != VALUE_U8 {
            return Err("期望 u8".to_string());
        }
        Ok(unsafe { value.data.as_u8 })
    }

    /// 读取 f64.
    ///
    /// # Errors
    ///
    /// 标签不符时返回说明文本.
    pub fn as_f64(value: &KslValue) -> Result<f64, String> {
        if value.tag != VALUE_F64 {
            return Err("期望 f64".to_string());
        }
        Ok(unsafe { value.data.as_f64 })
    }

    /// 读取 bool.
    ///
    /// # Errors
    ///
    /// 标签不符时返回说明文本.
    pub fn as_bool(value: &KslValue) -> Result<bool, String> {
        if value.tag != VALUE_BOOL {
            return Err("期望 bool".to_string());
        }
        Ok(unsafe { value.data.as_bool } != 0)
    }

    /// 读取 char.
    ///
    /// # Errors
    ///
    /// 标签不符或标量值非法时返回说明文本.
    pub fn as_char(value: &KslValue) -> Result<char, String> {
        if value.tag != VALUE_CHAR {
            return Err("期望 char".to_string());
        }
        char::from_u32(unsafe { value.data.as_char })
            .ok_or_else(|| "非法 Unicode 标量值".to_string())
    }

    /// 读取字符串借用.
    ///
    /// # Errors
    ///
    /// 标签不符, 指针为空或字节不是 UTF-8 时返回说明文本.
    pub fn as_str(value: &KslValue) -> Result<&str, String> {
        if value.tag != VALUE_STRING {
            return Err("期望 string".to_string());
        }
        let string: KslString = unsafe { value.data.as_string };
        unsafe { string.as_str() }.ok_or_else(|| "字符串不是合法 UTF-8".to_string())
    }

    /// 读取字符串副本.
    ///
    /// # Errors
    ///
    /// 同 [`as_str`].
    pub fn as_string(value: &KslValue) -> Result<String, String> {
        as_str(value).map(str::to_owned)
    }

    /// 读取不透明句柄.
    ///
    /// # Errors
    ///
    /// 标签不符时返回说明文本.
    pub fn as_opaque(value: &KslValue) -> Result<KslOpaqueValue, String> {
        if value.tag != VALUE_OPAQUE {
            return Err("期望不透明句柄".to_string());
        }
        Ok(unsafe { value.data.as_opaque })
    }

    /// 构造 `nil`.
    #[must_use]
    pub const fn nil() -> KslValue {
        KslValue::nil()
    }

    /// 构造字符串值; 字节在下一次插件入口前有效.
    #[must_use]
    pub fn string(value: &str) -> KslValue {
        KslValue::string(value)
    }

    /// 构造错误信息; 与字符串值共用暂存区.
    #[must_use]
    pub fn error(message: &str) -> KslValue {
        KslValue::string(message)
    }

    /// 值标签是否为 `nil`.
    #[must_use]
    pub fn is_nil(value: &KslValue) -> bool {
        value.tag == VALUE_NIL
    }
}

/// 插件作者常用项的汇总.
pub mod prelude {
    pub use crate::{KslOpaque, ksl_plugin, value};
}
