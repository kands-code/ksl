//! 协调器的延迟投递.
//!
//! 定时器按截止时刻排序; 取消或目标终止后条目惰性失效.

use super::{ActorHandle, Arc, Coordinator, Instant, Value, Weak};

/// 一次延迟投递; 目标用弱引用, 消息在触发前是环回收根.
pub(crate) struct SendTimer {
    pub(super) target: Weak<ActorHandle>,
    pub(super) target_id: u64,
    owner: Option<u64>,
    pub(super) index: usize,
    pub(super) message: Value,
}

impl Coordinator {
    pub(super) fn take_due_send_timers(&mut self, now: Instant) -> Vec<SendTimer> {
        self.send_timers
            .take_due(now)
            .into_iter()
            .map(|(_, timer)| timer)
            .collect()
    }

    pub(in crate::runtime::scheduler) fn next_send_deadline(&mut self) -> Option<Instant> {
        self.send_timers.next_deadline()
    }

    pub(super) fn cleanup_send_timers(&mut self, target_id: u64) {
        self.send_timers.retain(|_, timer| timer.target_id != target_id);
    }

    /// 登记一次延迟投递; 目标已终止时返回 None.
    pub(in crate::runtime::scheduler) fn set_timer(
        &mut self,
        target: &Arc<ActorHandle>,
        index: usize,
        message: Value,
        deadline: Instant,
        owner: Option<u64>,
    ) -> Option<u64> {
        if target.is_finished() {
            return None;
        }
        let id = self.next_timer;
        self.next_timer += 1;
        self.send_timers.insert(
            deadline,
            id,
            SendTimer {
                target: Arc::downgrade(target),
                target_id: target.id,
                owner,
                index,
                message,
            },
        );
        Some(id)
    }

    /// 取消尚未触发的定时消息; 只有创建者可以取消.
    pub(in crate::runtime::scheduler) fn cancel_timer(
        &mut self,
        id: u64,
        owner: Option<u64>,
    ) -> bool {
        let can_cancel = self
            .send_timers
            .get(&id)
            .is_some_and(|timer| timer.owner == owner);
        if !can_cancel {
            return false;
        }
        self.send_timers.remove(&id).is_some()
    }

    #[must_use]
    pub(in crate::runtime::scheduler) fn has_send_timers(&self) -> bool {
        !self.send_timers.is_empty()
    }

    /// 触发到期的延迟投递.
    pub(in crate::runtime::scheduler) fn fire_send_timers(&mut self) {
        // 没有延迟消息时不取当前时间, 避免热路径上的系统调用.
        if self.send_timers.is_empty() {
            return;
        }
        let due = self.take_due_send_timers(Instant::now());
        for timer in due {
            let Some(target) = timer.target.upgrade() else {
                continue;
            };
            if target.is_finished() {
                continue;
            }
            if target.send(timer.index, timer.message) && target.take_wake(timer.index) {
                self.wake_send(timer.target_id, timer.index);
            }
        }
    }
}
