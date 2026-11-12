//! 原生插件接口.
//!
//! 插件与宿主由同一工作区与同一工具链构建, 函数直接接收与返回运行时值.
//! 动态库只导出 `ksl_native_plugin` 与静态描述符, 不经过 C 值编解码.
//! 插件未引入的运行时状态各持一份; 名字自包含, 值可直接跨副本比较.

/// 在调度线程内联执行.
pub const EXECUTION_CURRENT: u32 = 0;
/// 在 I/O 线程池执行.
pub const EXECUTION_IO: u32 = 1;
/// 在 CPU 线程池执行.
pub const EXECUTION_CPU: u32 = 2;

/// 静态 UTF-8 字节切片; 不要求 NUL 结尾.
///
/// 只由 [`Self::from_static`] 构造, 指向插件二进制中的静态内存.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KslStr {
    ptr: *const u8,
    len: usize,
}

impl KslStr {
    #[must_use]
    pub const fn from_static(value: &'static str) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }

    /// 读取静态字符串; 非法 UTF-8 或空指针时返回 None.
    #[must_use]
    pub fn as_str(&self) -> Option<&'static str> {
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

// 字段指向插件二进制中的静态只读字节, 不拥有内存.
unsafe impl Send for KslStr {}
unsafe impl Sync for KslStr {}

use crate::value::Value;

/// 原生插件 ABI 版本; 插件与宿主必须一致.
pub const NATIVE_ABI_VERSION: u32 = 1;

/// 原生插件函数入口.
///
/// 参数由调用方按 `@Link` 声明传入, 函数自行校验类型.
/// `Err` 表示插件内部失败, 宿主报运行时错误.
pub type NativeEntry = extern "Rust" fn(args: &[Value]) -> Result<Value, String>;

/// 插件函数描述.
#[repr(C)]
pub struct NativeFunction {
    /// ksl 链接名, 对应 `@Link` 中的名字.
    pub name: KslStr,
    /// 插件源名, 供诊断使用.
    pub source_name: KslStr,
    pub entry: NativeEntry,
    /// 执行类别, 取 `EXECUTION_*`.
    pub execution: u32,
}

/// 插件不透明类型.
#[repr(C)]
pub struct NativeType {
    pub name: KslStr,
}

/// 初始化入口; 返回 `Err` 表示加载失败.
pub type NativeInitEntry = extern "Rust" fn() -> Result<(), String>;

/// 卸载入口; 在动态库卸载前调用一次.
pub type NativeUnloadEntry = extern "Rust" fn();

/// 事件泵入口; 在调度线程上, 于执行片段之间调用.
pub type NativeEventPumpEntry = extern "Rust" fn();

/// 原生全局值入口.
pub type NativeGlobalEntry = extern "Rust" fn() -> Value;

/// 插件全局值.
#[repr(C)]
pub struct NativeGlobal {
    pub name: KslStr,
    pub source_name: KslStr,
    pub get: NativeGlobalEntry,
}

/// 插件描述符; 由插件静态持有.
#[repr(C)]
pub struct NativePlugin {
    pub abi: u32,
    pub name: KslStr,
    pub version: KslStr,
    pub functions: *const NativeFunction,
    pub function_count: u32,
    pub types: *const NativeType,
    pub type_count: u32,
    pub globals: *const NativeGlobal,
    pub global_count: u32,
    pub init: Option<NativeInitEntry>,
    pub unload: Option<NativeUnloadEntry>,
    pub pump: Option<NativeEventPumpEntry>,
}

// 描述符由插件静态持有, 只读, 可跨线程共享.
unsafe impl Sync for NativePlugin {}
