//! ksl 原生插件接口的测试插件.
//!
//! 覆盖标量, 列表, 字符串, 字符, `Result`, 不透明资源, 全局值,
//! 执行类别与生命周期; 由 `plugins/demo/runtime.ksl` 驱动.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};

use ksl_value::value::NativeOpaque;

pub struct Widget {
    value: i64,
}

impl NativeOpaque for Widget {
    fn type_name(&self) -> &'static str {
        "Widget"
    }
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static INIT_THREAD_TOKEN: AtomicU64 = AtomicU64::new(0);
static PUMPED: AtomicUsize = AtomicUsize::new(0);
static UNLOAD_MARKER: Mutex<Option<String>> = Mutex::new(None);

fn thread_token() -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::format!("{:?}", std::thread::current().id()).hash(&mut hasher);
    hasher.finish()
}

#[ksl_value::ksl_native_plugin(name = "demo", types = ["Widget"])]
mod native {
    use std::sync::atomic::Ordering;

    use ksl_value::value::Value;

    use super::{INIT_THREAD_TOKEN, INITIALIZED, PUMPED, UNLOAD_MARKER, Widget};

    #[ksl_native_init]
    // 初始化函数由插件宏要求统一返回 Result.
    #[expect(clippy::unnecessary_wraps)]
    fn setup() -> Result<(), String> {
        INITIALIZED.store(true, Ordering::SeqCst);
        INIT_THREAD_TOKEN.store(super::thread_token(), Ordering::SeqCst);
        Ok(())
    }

    #[ksl_native_unload]
    fn teardown() {
        INITIALIZED.store(false, Ordering::SeqCst);
        if let Some(path) = UNLOAD_MARKER.lock().unwrap().as_deref() {
            std::fs::write(path, "unloaded").expect("卸载标记应可写入");
        }
    }

    #[ksl_native_event_pump]
    fn pump() {
        PUMPED.fetch_add(1, Ordering::SeqCst);
    }

    #[ksl_native_global]
    const VERSION: i64 = 1;

    #[ksl_native_global]
    const TITLE: &str = "demo";

    #[ksl_native_global]
    const MAIN_ONLY: i64 = 2;

    #[ksl_native_export]
    fn pump_count() -> u64 {
        u64::try_from(PUMPED.load(Ordering::SeqCst)).expect("计数应可转换")
    }

    #[ksl_native_export]
    fn was_initialized() -> bool {
        INITIALIZED.load(Ordering::SeqCst)
    }

    #[ksl_native_export]
    fn init_thread_token() -> u64 {
        INIT_THREAD_TOKEN.load(Ordering::SeqCst)
    }

    #[ksl_native_export]
    fn set_unload_marker(path: &str) {
        *UNLOAD_MARKER.lock().unwrap() = Some(path.to_string());
    }

    #[ksl_native_export]
    fn thread_token() -> u64 {
        super::thread_token()
    }

    #[ksl_native_export]
    fn add(left: i64, right: i64) -> i64 {
        left + right
    }

    #[ksl_native_export]
    fn sum(args: &[Value]) -> Result<Value, String> {
        let [value] = args else {
            return Err("参数应为列表".to_string());
        };
        let len = value.list_len().ok_or_else(|| "参数应为列表".to_string())?;
        let mut total = 0;
        for index in 0..len {
            total += value
                .list_get(index)
                .and_then(|item| item.as_i64())
                .ok_or_else(|| "元素应为 i64".to_string())?;
        }
        Ok(Value::I64(total))
    }

    #[ksl_native_export]
    fn count_true(args: &[Value]) -> Result<Value, String> {
        let [value] = args else {
            return Err("参数应为列表".to_string());
        };
        let len = value.list_len().ok_or_else(|| "参数应为列表".to_string())?;
        let mut total = 0;
        for index in 0..len {
            if value
                .list_get(index)
                .and_then(|item| item.as_bool())
                .ok_or_else(|| "元素应为 bool".to_string())?
            {
                total += 1;
            }
        }
        Ok(Value::U64(total))
    }

    #[ksl_native_export]
    fn add_i32(left: i32, right: i32) -> i32 {
        left + right
    }

    #[ksl_native_export]
    fn half(value: f64) -> f64 {
        value / 2.0
    }

    #[ksl_native_export]
    fn half_f32(value: f32) -> f32 {
        value / 2.0
    }

    #[ksl_native_export]
    fn is_positive(value: i64) -> bool {
        value > 0
    }

    #[ksl_native_export]
    fn greet(name: &str) -> String {
        format!("hello {name}")
    }

    #[ksl_native_export]
    fn join(left: &str, right: &str) -> String {
        format!("{left}+{right}")
    }

    #[ksl_native_export]
    fn shout(value: char) -> char {
        value.to_ascii_uppercase()
    }

    #[ksl_native_export]
    fn parse(value: &str) -> Result<i64, String> {
        value.parse().map_err(|_| "bad".to_string())
    }

    #[ksl_native_export]
    #[ksl_native_exec(io)]
    fn io_add(left: i64, right: i64) -> i64 {
        left + right
    }

    #[ksl_native_export]
    #[ksl_native_exec(cpu)]
    fn cpu_add(left: i64, right: i64) -> i64 {
        left + right
    }

    #[ksl_native_export]
    fn main_add(left: i64, right: i64) -> i64 {
        left + right
    }

    #[ksl_native_export("MakeWidget")]
    fn make_widget() -> Widget {
        Widget { value: 7 }
    }

    #[ksl_native_export]
    fn widget_value(widget: &Widget) -> i64 {
        widget.value * 6
    }

    /// 返回列表, 验证宿主与插件之间的非标量返回值.
    #[ksl_native_export]
    fn list_of(value: i64) -> Value {
        Value::list(vec![Value::I64(value)])
    }
}
