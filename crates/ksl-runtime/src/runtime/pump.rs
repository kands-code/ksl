//! 平台事件泵注册表.
//!
//! 事件泵由插件提供, 必须在运行时线程上调用. 运行时占住该线程,
//! 由调度循环在片段之间轮询, 空闲时按固定间隔唤醒.

use std::any::Any;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use ksl_value::native::NativeEventPumpEntry;
use parking_lot::Mutex;

/// 事件泵轮询间隔.
pub(crate) const INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);

struct Pump {
    id: u64,
    /// 保持插件动态库存活; 事件泵入口依赖它.
    keepalive: Arc<dyn Any + Send + Sync>,
    entry: NativeEventPumpEntry,
}

/// 事件泵注册表.
///
/// 交互会话不注册事件泵: 它会把控制权交给插件, 与终端输入冲突.
#[derive(Default)]
pub struct PumpRegistry {
    pumps: Mutex<Vec<Pump>>,
    next_id: AtomicU64,
    enabled: AtomicBool,
}

impl PumpRegistry {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            pumps: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            enabled: AtomicBool::new(enabled),
        }
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// 注册原生事件泵; 未启用时返回 None.
    pub fn register_native(
        &self,
        keepalive: Arc<dyn Any + Send + Sync>,
        entry: NativeEventPumpEntry,
    ) -> Option<u64> {
        if !self.enabled.load(Ordering::Relaxed) {
            return None;
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pumps.lock().push(Pump {
            id,
            keepalive,
            entry,
        });
        Some(id)
    }

    pub fn unregister(&self, id: u64) {
        self.pumps.lock().retain(|pump| pump.id != id);
    }

    /// 禁用期间注册表保留已有泵, 重新启用后继续轮询.
    #[must_use]
    pub(crate) fn is_active(&self) -> bool {
        self.enabled.load(Ordering::Relaxed) && !self.pumps.lock().is_empty()
    }

    /// 调用一轮事件泵; panic 的泵移除.
    pub(crate) fn tick(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        // 外部入口可能注销自身或卸载插件, 不能在持锁时调用.
        // 快照持有动态库引用, 调用期间库不会被卸载.
        let pumps: Vec<(u64, Arc<dyn Any + Send + Sync>, NativeEventPumpEntry)> = {
            let pumps = self.pumps.lock();
            pumps
                .iter()
                .map(|pump| (pump.id, Arc::clone(&pump.keepalive), pump.entry))
                .collect()
        };
        let mut failed = Vec::new();
        for (id, _keepalive, entry) in &pumps {
            let entry = *entry;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(entry));
            if result.is_err() {
                failed.push(*id);
            }
        }
        if !failed.is_empty() {
            self.pumps.lock().retain(|pump| !failed.contains(&pump.id));
        }
    }
}
