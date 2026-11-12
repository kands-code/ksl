//! Actor 句柄与终止原因.

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use arcstr::ArcStr;
use ksl_base::name::keywords;
use parking_lot::Mutex;

use super::Value;
use super::data::{data_value, data_value0, result_err, result_ok};

/// Actor 终止原因.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitReason {
    Halted,
    Killed,
    Failed { code: i64, message: String },
}

/// Actor 的最终结局.
pub type ActorResult = Result<Value, ExitReason>;

/// Actor 状态; 终止后保留结局供 Join 读取.
#[derive(Debug, Clone)]
enum ActorStatus {
    Running,
    Finished(ActorResult),
}

/// Actor 句柄: 信箱, 状态与父子关系.
#[derive(Debug)]
pub struct ActorHandle {
    pub id: u64,
    /// 父 Actor 句柄; Detached Actor 为 None.
    pub parent: Option<Arc<ActorHandle>>,
    mailboxes: Vec<Mutex<VecDeque<Value>>>,
    status: Mutex<ActorStatus>,
    /// 仍存活或尚未启动的子 Actor; 按编号排序, 使终止时的清理顺序可复现.
    children: Mutex<BTreeSet<u64>>,
    kill: AtomicBool,
    /// 每个信箱的唤醒位; 置位表示正在等待该信箱且没有待处理的唤醒事件.
    ///
    /// 信箱数不超过 16, 每个信箱占一位.
    wake_armed: AtomicU32,
    /// 是否已通过 `@Join` 观察; 已观察的失败不再重复诊断.
    ///
    /// 用 `Arc` 共享给错误记录, 句柄释放后仍能判断该失败是否已被观察.
    observed: Arc<AtomicBool>,
}

impl ActorHandle {
    #[must_use]
    pub fn new(id: u64, mailboxes: usize, parent: Option<Arc<ActorHandle>>) -> Self {
        Self {
            id,
            parent,
            mailboxes: (0..mailboxes).map(|_| Mutex::new(VecDeque::new())).collect(),
            status: Mutex::new(ActorStatus::Running),
            children: Mutex::new(BTreeSet::new()),
            kill: AtomicBool::new(false),
            wake_armed: AtomicU32::new(0),
            observed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 标记该 Actor 的失败已被 `@Join` 观察.
    pub fn mark_observed(&self) {
        self.observed.store(true, Ordering::Relaxed);
    }

    /// 与错误记录共享的观察标记.
    #[must_use]
    pub fn observation_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.observed)
    }

    /// 投递消息; 索引越界返回 false.
    pub fn send(&self, index: usize, message: Value) -> bool {
        let Some(mailbox) = self.mailboxes.get(index) else {
            return false;
        };
        mailbox.lock().push_back(message);
        true
    }

    /// 尝试从指定信箱取一条消息.
    #[must_use]
    pub fn try_recv(&self, index: usize) -> Option<Value> {
        self.mailboxes.get(index)?.lock().pop_front()
    }

    /// 指定信箱是否非空; 协调器唤醒时只检查, 不取走消息.
    #[must_use]
    pub fn has_message(&self, index: usize) -> bool {
        self.mailboxes
            .get(index)
            .is_some_and(|mailbox| !mailbox.lock().is_empty())
    }

    /// 置位信箱唤醒位; 进入等待时调用.
    ///
    /// 先置位再检查信箱, 此后到达的消息能看到该位.
    pub fn arm_wake(&self, index: usize) {
        if let Some(bit) = wake_bit(index) {
            self.wake_armed.fetch_or(bit, Ordering::Relaxed);
        }
    }

    /// 取走信箱唤醒位; 返回 true 表示需要投递唤醒事件.
    ///
    /// 等待期间只有第一条消息需要唤醒, 其余消息留在信箱中由下一次读取取走.
    pub fn take_wake(&self, index: usize) -> bool {
        let Some(bit) = wake_bit(index) else {
            return false;
        };
        self.wake_armed.fetch_and(!bit, Ordering::Relaxed) & bit != 0
    }

    /// 清除全部唤醒位; Actor 恢复运行或终止时调用.
    pub fn clear_wake(&self) {
        self.wake_armed.store(0, Ordering::Relaxed);
    }

    /// 标记终止; 已终止时返回 false, 保留最初结局.
    pub fn finish(&self, result: ActorResult) -> bool {
        let mut status = self.status.lock();
        if matches!(*status, ActorStatus::Finished(_)) {
            return false;
        }
        *status = ActorStatus::Finished(result);
        true
    }

    /// 读取终止结局; 未终止时为 None.
    #[must_use]
    pub fn result(&self) -> Option<ActorResult> {
        match &*self.status.lock() {
            ActorStatus::Running => None,
            ActorStatus::Finished(result) => Some(result.clone()),
        }
    }

    /// 只判断状态, 不复制结局.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        matches!(*self.status.lock(), ActorStatus::Finished(_))
    }

    pub fn request_kill(&self) {
        self.kill.store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub fn kill_requested(&self) -> bool {
        self.kill.load(Ordering::Relaxed)
    }

    /// 父句柄; 环回收遍历用.
    #[must_use]
    pub fn parent_handle(&self) -> Option<&Arc<ActorHandle>> {
        self.parent.as_ref()
    }

    /// 遍历信箱与终止结果中的值; 环回收标记用.
    pub fn for_each_stored(&self, visit: &mut dyn FnMut(&Value)) {
        for mailbox in &self.mailboxes {
            for value in mailbox.lock().iter() {
                visit(value);
            }
        }
        if let ActorStatus::Finished(Ok(value)) = &*self.status.lock() {
            visit(value);
        }
    }

    /// 断开不可达句柄的强引用; 只能由环回收在安全点调用.
    pub fn clear_for_gc(&self) {
        for mailbox in &self.mailboxes {
            mailbox.lock().clear();
        }
        // 终止结果中的值一并丢弃.
        *self.status.lock() = ActorStatus::Running;
    }

    pub fn add_child(&self, id: u64) {
        self.children.lock().insert(id);
    }

    /// 从父句柄的子列表中移除自身; 只由终止路径调用.
    pub fn detach_from_parent(&self) {
        if let Some(parent) = &self.parent {
            parent.children.lock().remove(&self.id);
        }
    }

    pub fn take_children(&self) -> Vec<u64> {
        std::mem::take(&mut *self.children.lock()).into_iter().collect()
    }
}

impl Drop for ActorHandle {
    fn drop(&mut self) {
        // 逐个摘下父链, 按循环析构.
        let mut parent = self.parent.take();
        while let Some(handle) = parent {
            let Ok(mut handle) = Arc::try_unwrap(handle) else {
                break;
            };
            parent = handle.parent.take();
        }
    }
}

/// 信箱下标对应的唤醒位; 超出位宽时返回 None.
fn wake_bit(index: usize) -> Option<u32> {
    1u32.checked_shl(u32::try_from(index).ok()?)
}

/// 把终止原因转为内置 Data 值.
#[must_use]
pub fn exit_reason_value(reason: &ExitReason) -> Value {
    let names = keywords();
    match reason {
        ExitReason::Halted => data_value0(names.exit_reason_.clone(), names.halted_.clone()),
        ExitReason::Killed => data_value0(names.exit_reason_.clone(), names.killed_.clone()),
        ExitReason::Failed { code, message } => data_value(
            names.exit_reason_.clone(),
            names.failed_.clone(),
            [Value::I64(*code), Value::Str(ArcStr::from(message.as_str()))],
        ),
    }
}

/// 把 Actor 结局转为 `Result[R, ExitReason]`.
#[must_use]
pub fn join_result_value(result: &ActorResult) -> Value {
    match result {
        Ok(value) => result_ok(value.clone()),
        Err(reason) => result_err(exit_reason_value(reason)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 父链的释放不依赖调用栈深度.
    #[test]
    fn actor_parent_chain_drops_iteratively() {
        let mut parent = None;
        for id in 0..100_000 {
            parent = Some(Arc::new(ActorHandle::new(id, 1, parent.take())));
        }
        drop(parent);
    }
}
