//! 协调器的 Join 等待表.
//!
//! 一个 Actor 同时只等待一个目标.

use super::{ActorHandle, Arc, Coordinator, Value, join_result_value};

/// 一个等待 Join 的 Actor.
pub(super) struct JoinWaiter {
    pub(super) actor: u64,
}

impl Coordinator {
    pub(super) fn take_join_waiters(&mut self, target_id: u64) -> Vec<JoinWaiter> {
        self.join_waiters.remove(&target_id).unwrap_or_default()
    }

    pub(super) fn remove_join_waiter(&mut self, actor: u64) {
        self.join_waiters.retain(|_, waiters| {
            waiters.retain(|waiter| waiter.actor != actor);
            !waiters.is_empty()
        });
    }

    /// 登记 Join 等待; 目标已终止时直接返回结局.
    ///
    /// 检查与登记在同一状态上完成, 避免在检查与登记之间丢失唤醒.
    pub(super) fn wait_join(
        &mut self,
        target: &Arc<ActorHandle>,
        waiter: u64,
    ) -> Option<Value> {
        if let Some(result) = target.result() {
            return Some(join_result_value(&result));
        }
        self.join_waiters
            .entry(target.id)
            .or_default()
            .push(JoinWaiter { actor: waiter });
        None
    }
}
