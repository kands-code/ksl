//! 平台事件泵注册表.
//!
//! 事件泵由插件提供, 必须在运行时线程上调用. 运行时占住该线程,
//! 因此由调度循环在片段之间轮询, 空闲时按固定间隔唤醒.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use ksl_plugin::KslEventPumpEntry;
use libloading::Library;
use parking_lot::Mutex;

/// 事件泵轮询间隔.
pub(crate) const INTERVAL: std::time::Duration = std::time::Duration::from_millis(8);

struct Pump {
    id: u64,
    /// 保持动态库存活; 事件泵入口依赖它.
    library: Arc<Library>,
    entry: KslEventPumpEntry,
}

/// 事件泵注册表.
///
/// 交互会话不注册事件泵: 它会把控制权交给插件, 与终端输入冲突.
#[derive(Default)]
pub(crate) struct PumpRegistry {
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

    /// 注册一个事件泵; 未启用时返回 None, 插件仍可正常加载.
    pub(crate) fn register(
        &self,
        library: Arc<Library>,
        entry: KslEventPumpEntry,
    ) -> Option<u64> {
        if !self.enabled.load(Ordering::Relaxed) {
            return None;
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.pumps.lock().push(Pump { id, library, entry });
        Some(id)
    }

    pub(crate) fn unregister(&self, id: u64) {
        self.pumps.lock().retain(|pump| pump.id != id);
    }

    /// 是否有需要轮询的事件泵.
    ///
    /// 禁用期间注册表保留已有泵, 重新启用后继续轮询.
    #[must_use]
    pub(crate) fn is_active(&self) -> bool {
        self.enabled.load(Ordering::Relaxed) && !self.pumps.lock().is_empty()
    }

    /// 调用一轮事件泵; panic 的泵移除, 避免反复打断调度循环.
    pub(crate) fn tick(&self) {
        if !self.enabled.load(Ordering::Relaxed) {
            return;
        }
        // 外部入口可能在调用中注销自身或卸载插件, 因此不能在持锁时调用.
        // 快照持有动态库引用, 调用期间库不会被卸载.
        let pumps: Vec<(u64, Arc<Library>, KslEventPumpEntry)> = {
            let pumps = self.pumps.lock();
            pumps
                .iter()
                .map(|pump| (pump.id, Arc::clone(&pump.library), pump.entry))
                .collect()
        };
        let mut failed = Vec::new();
        for (id, _library, entry) in &pumps {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                entry();
            }))
            .is_err()
            {
                failed.push(*id);
            }
        }
        if !failed.is_empty() {
            self.pumps.lock().retain(|pump| !failed.contains(&pump.id));
        }
    }
}
