use ksl_plugin::prelude::*;
use ksl_plugin::{KslFunctionDef, KslGlobalDef, KslOpaque, KslOpaqueType, KslPlugin, KslValue};

pub struct Widget {
    count: i64,
}

impl KslOpaque for Widget {
    const NAME: &'static str = "Widget";
    const RELEASE_THREAD: u32 = ksl_plugin::THREAD_RUNTIME;
}

#[ksl_plugin(name = "demo", version = "0.1.0")]
mod demo {
    use super::*;

    #[ksl_export]
    fn add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_export("Greet")]
    fn greet(name: &str) -> String {
        format!("hello {name}")
    }

    #[ksl_export]
    fn make_widget() -> Widget {
        Widget { count: 7 }
    }

    #[ksl_export]
    fn widget_value(widget: &Widget) -> i64 {
        widget.count
    }

    #[ksl_export]
    fn boom() -> i64 {
        panic!("boom")
    }

    #[ksl_export]
    #[ksl_exec(io)]
    fn io_add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_export]
    #[ksl_exec(cpu)]
    fn cpu_add(a: i64, b: i64) -> i64 {
        a + b
    }

    #[ksl_init]
    #[allow(clippy::unnecessary_wraps)]
    fn setup() -> Result<(), String> {
        Ok(())
    }

    #[ksl_unload]
    fn teardown() {}

    #[ksl_global]
    const VERSION: i64 = 1;

    #[ksl_global("TITLE")]
    static TITLE_TEXT: &str = "demo";

    #[ksl_global]
    const MAIN_ONLY: i64 = 2;
}

fn plugin() -> &'static KslPlugin {
    let pointer = demo::ksl_plugin_init(ksl_plugin::ABI_VERSION);
    assert!(!pointer.is_null());
    unsafe { &*pointer }
}

fn name(value: ksl_plugin::KslStr) -> &'static str {
    unsafe { value.as_str() }.expect("名字应是 UTF-8")
}

fn functions() -> &'static [KslFunctionDef] {
    let plugin = plugin();
    unsafe { std::slice::from_raw_parts(plugin.functions, plugin.function_count as usize) }
}

fn function(name: &str) -> &'static KslFunctionDef {
    functions()
        .iter()
        .find(|function| self::name(function.name) == name)
        .expect("函数应存在")
}

fn global(name: &str) -> &'static KslGlobalDef {
    let plugin = plugin();
    let globals =
        unsafe { std::slice::from_raw_parts(plugin.globals, plugin.global_count as usize) };
    globals
        .iter()
        .find(|global| self::name(global.name) == name)
        .expect("全局值应存在")
}

fn types() -> &'static [KslOpaqueType] {
    let plugin = plugin();
    unsafe { std::slice::from_raw_parts(plugin.types, plugin.type_count as usize) }
}

#[test]
fn owns_and_releases_opaque_handles() {
    assert_eq!(types().len(), 1);
    assert_eq!(name(types()[0].name), "Widget");

    let make = function("make_widget");
    let mut out = KslValue::nil();
    let status = unsafe { (make.entry)(std::ptr::null(), 0, std::ptr::from_mut(&mut out)) };
    assert_eq!(status, 0);
    let widget = ksl_plugin::value::as_opaque(&out).unwrap();
    assert_eq!(widget.type_index, 0);
    assert_eq!(widget.owned, 1);

    let value = function("widget_value");
    let args = [out];
    let mut result = KslValue::nil();
    let status = unsafe {
        (value.entry)(
            args.as_ptr(),
            u32::try_from(args.len()).unwrap(),
            std::ptr::from_mut(&mut result),
        )
    };
    assert_eq!(status, 0);
    assert_eq!(ksl_plugin::value::as_i64(&result).unwrap(), 7);

    let release = types()[0].release.expect("类型应有释放函数");
    unsafe { release(widget.handle) };
}

#[test]
fn panicking_export_returns_error() {
    let boom = function("boom");
    let mut out = KslValue::nil();
    let status = unsafe { (boom.entry)(std::ptr::null(), 0, std::ptr::from_mut(&mut out)) };
    assert_ne!(status, 0);
    assert_eq!(ksl_plugin::value::as_str(&out).unwrap(), "插件函数 panic");
}

#[test]
fn descriptor_declares_execution_and_thread_affinity() {
    assert_eq!(plugin().unload_thread, ksl_plugin::THREAD_ANY);
    assert!(plugin().init.is_some());
    assert!(plugin().unload.is_some());
    assert_eq!(function("io_add").execution, ksl_plugin::EXECUTION_IO);
    assert_eq!(function("cpu_add").execution, ksl_plugin::EXECUTION_CPU);
    assert_eq!(function("add").execution, ksl_plugin::EXECUTION_CURRENT);
    assert_eq!(global("MAIN_ONLY").execution, ksl_plugin::EXECUTION_CURRENT);
    assert_eq!(global("VERSION").execution, ksl_plugin::EXECUTION_CURRENT);
    assert_eq!(types()[0].release_thread, ksl_plugin::THREAD_RUNTIME);
    assert_eq!(name(function("add").source_name), "add");
    assert_eq!(name(function("Greet").source_name), "greet");
    assert_eq!(name(global("TITLE").source_name), "TITLE_TEXT");
    assert_eq!(name(global("MAIN_ONLY").source_name), "MAIN_ONLY");
}
