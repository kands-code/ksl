//! 协调器的监视登记.
//!
//! 监视者用弱引用, 目标只记编号, 避免形成强引用环.
//! 目标终止时投递一次性通知.

use super::{ActorHandle, ActorResult, Arc, Coordinator, ExitReason, Value, Weak};
use crate::value::exit_reason_value;

/// 一次监视登记.
pub(super) struct MonitorEntry {
    watcher: Weak<ActorHandle>,
    watcher_id: u64,
    target_id: u64,
    index: usize,
}

impl Coordinator {
    pub(super) fn insert_monitor(&mut self, id: u64, entry: MonitorEntry) {
        self.monitors_by_target
            .entry(entry.target_id)
            .or_default()
            .insert(id);
        self.monitors_by_watcher
            .entry(entry.watcher_id)
            .or_default()
            .insert(id);
        self.monitors.insert(id, entry);
    }

    pub(super) fn remove_monitor(&mut self, id: u64) -> Option<MonitorEntry> {
        let entry = self.monitors.remove(&id)?;
        if let Some(ids) = self.monitors_by_target.get_mut(&entry.target_id) {
            ids.remove(&id);
            if ids.is_empty() {
                self.monitors_by_target.remove(&entry.target_id);
            }
        }
        if let Some(ids) = self.monitors_by_watcher.get_mut(&entry.watcher_id) {
            ids.remove(&id);
            if ids.is_empty() {
                self.monitors_by_watcher.remove(&entry.watcher_id);
            }
        }
        Some(entry)
    }

    pub(super) fn clear_monitors(&mut self) {
        self.monitors.clear();
        self.monitors_by_target.clear();
        self.monitors_by_watcher.clear();
    }

    /// 监视表是否已清空; 供测试确认清理路径.
    #[cfg(test)]
    #[must_use]
    pub(in crate::runtime::scheduler) fn monitors_empty(&self) -> bool {
        self.monitors.is_empty()
            && self.monitors_by_target.is_empty()
            && self.monitors_by_watcher.is_empty()
    }

    /// 登记一次监视; 监视自身返回 None.
    ///
    /// 目标已终止时立即投递通知并返回编号, 但不登记,
    /// 避免调用方在 Spawn 与 Monitor 之间丢失终止事件.
    pub(super) fn monitor(
        &mut self,
        watcher: &Arc<ActorHandle>,
        target: &Arc<ActorHandle>,
    ) -> Option<u64> {
        if watcher.id == target.id {
            return None;
        }
        // 控制信箱固定为最后一个; 检查器已保证其类型为监视通知.
        let index = watcher.last_mailbox_index();
        let id = self.next_monitor;
        self.next_monitor += 1;
        if let Some(reason) = target.exit_reason() {
            let notice = monitor_notice_value(id, &reason);
            if watcher.send(index, notice) {
                // 通知已实际投递, 该失败才算被观察.
                target.mark_observed();
                if watcher.take_wake(index) {
                    self.wake_send(watcher.id, index);
                }
            }
            return Some(id);
        }
        self.insert_monitor(
            id,
            MonitorEntry {
                watcher: Arc::downgrade(watcher),
                watcher_id: watcher.id,
                target_id: target.id,
                index,
            },
        );
        Some(id)
    }

    /// 注销一次监视; 只有登记它的 Actor 可以注销.
    pub(super) fn unmonitor(&mut self, id: u64, watcher_id: u64) -> bool {
        let owned = self
            .monitors
            .get(&id)
            .is_some_and(|entry| entry.watcher_id == watcher_id);
        if !owned {
            return false;
        }
        self.remove_monitor(id).is_some()
    }

    /// 目标终止时投递全部监视通知; 监视者已终止的条目直接丢弃.
    pub(super) fn deliver_monitors(&mut self, target: &Arc<ActorHandle>, result: &ActorResult) {
        let target_id = target.id;
        let Some(ids) = self.monitors_by_target.remove(&target_id) else {
            return;
        };
        let reason = notice_reason(result);
        let actions = ids
            .into_iter()
            .filter_map(|id| {
                let entry = self.remove_monitor(id)?;
                let watcher = entry.watcher.upgrade()?;
                if watcher.is_finished() {
                    return None;
                }
                Some((watcher, entry.index, monitor_notice_value(id, &reason)))
            })
            .collect::<Vec<_>>();
        for (watcher, index, notice) in actions {
            if watcher.send(index, notice) {
                // 至少一个监视者实际收到通知, 该失败才算被观察.
                target.mark_observed();
                if watcher.take_wake(index) {
                    self.wake_send(watcher.id, index);
                }
            }
        }
    }

    /// 监视者终止时删除它登记的全部监视.
    pub(super) fn cleanup_monitors(&mut self, watcher_id: u64) {
        let Some(ids) = self.monitors_by_watcher.remove(&watcher_id) else {
            return;
        };
        for id in ids {
            self.remove_monitor(id);
        }
    }
}

/// 监视通知中的退出原因: 正常返回与 `@Halt` 都记为 `Halted`.
fn notice_reason(result: &ActorResult) -> ExitReason {
    match result {
        Ok(_) => ExitReason::Halted,
        Err(reason) => reason.clone(),
    }
}

/// 监视通知的载荷; 信箱类型约定为 `Pair[u64, ExitReason]`.
fn monitor_notice_value(id: u64, reason: &ExitReason) -> Value {
    Value::Pair(Arc::new((Value::U64(id), exit_reason_value(reason))))
}
