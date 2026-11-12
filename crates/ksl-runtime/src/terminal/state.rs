//! 终端原始模式的持有状态.
//!
//! 输入线程负责修改持有者与标记; 标记由同一运行时的临时守卫与宿主读取.
//! 宿主在退出前恢复终端时也会清除该标记.

use std::sync::atomic::{AtomicBool, Ordering};

/// 原始模式持有者.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawModeHolder {
    /// 顶层语句或根 Actor.
    Entry,
    /// 普通 Actor.
    Actor(u64),
    /// 交互编辑器会话.
    Session,
}

impl RawModeHolder {
    /// 把 Actor 编号归一为持有者; 根 Actor 与顶层语句同为入口.
    pub(crate) fn from_owner(owner: Option<u64>, root: Option<u64>) -> Self {
        match owner {
            None => Self::Entry,
            Some(id) if Some(id) == root => Self::Entry,
            Some(id) => Self::Actor(id),
        }
    }
}

/// 输入线程持有的逻辑状态.
#[derive(Default)]
pub(crate) struct TerminalState {
    holder: Option<RawModeHolder>,
}

impl TerminalState {
    pub(crate) fn holder(&self) -> Option<RawModeHolder> {
        self.holder
    }

    pub(crate) fn set_holder(&mut self, holder: RawModeHolder) {
        self.holder = Some(holder);
    }

    /// 清空持有者并返回被清除的持有者.
    pub(crate) fn clear(&mut self) -> Option<RawModeHolder> {
        self.holder.take()
    }
}

/// 终端模式标记; 由输入线程维护, 供临时守卫与宿主读取.
#[derive(Default)]
pub(crate) struct TerminalFlags {
    /// 终端是否可能仍处于原始模式.
    raw: AtomicBool,
    /// 是否存在原始模式持有者.
    held: AtomicBool,
}

impl TerminalFlags {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub(crate) fn raw_active(&self) -> bool {
        self.raw.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_raw(&self) {
        self.raw.store(true, Ordering::Relaxed);
    }

    pub(crate) fn clear_raw(&self) {
        self.raw.store(false, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn held(&self) -> bool {
        self.held.load(Ordering::Relaxed)
    }

    pub(crate) fn set_held(&self, value: bool) {
        self.held.store(value, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::{RawModeHolder, TerminalState};

    #[test]
    fn holder_normalization_and_updates() {
        assert_eq!(RawModeHolder::from_owner(None, None), RawModeHolder::Entry);
        assert_eq!(
            RawModeHolder::from_owner(Some(7), Some(7)),
            RawModeHolder::Entry
        );
        assert_eq!(
            RawModeHolder::from_owner(Some(8), Some(7)),
            RawModeHolder::Actor(8)
        );

        let mut state = TerminalState::default();
        assert_eq!(state.holder(), None);
        state.set_holder(RawModeHolder::Actor(3));
        assert_eq!(state.holder(), Some(RawModeHolder::Actor(3)));
        assert_eq!(state.clear(), Some(RawModeHolder::Actor(3)));
        assert_eq!(state.clear(), None);
    }
}
