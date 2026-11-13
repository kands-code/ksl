//! 通用定时器最小堆.
//!
//! 只负责截止时刻排序, 惰性删除与到期取出; 条目的语义由持有者决定.
//! 任务超时与共享延迟投递各自持有实例, 不共享生命周期或取消规则.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;
use std::time::Instant;

/// 以键区分活动条目的定时器堆.
///
/// 同一键再次登记时旧条目留在堆中, 通过序号比对惰性失效. 键必须可排序,
/// 用于同一截止时刻的稳定顺序.
pub(super) struct TimerHeap<K, V> {
    heap: BinaryHeap<Reverse<(Instant, u64, K)>>,
    active: HashMap<K, (u64, V)>,
    next_sequence: u64,
}

impl<K, V> TimerHeap<K, V>
where
    K: Copy + Eq + Hash + Ord,
{
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            active: HashMap::new(),
            next_sequence: 1,
        }
    }

    /// 登记一个定时器; 同一键的旧条目失效.
    pub(super) fn insert(&mut self, deadline: Instant, key: K, value: V) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.active.insert(key, (sequence, value));
        self.heap.push(Reverse((deadline, sequence, key)));
    }

    /// 取消一个键的活动定时器.
    pub(super) fn remove(&mut self, key: &K) -> Option<V> {
        self.active.remove(key).map(|(_, value)| value)
    }

    #[must_use]
    pub(super) fn get(&self, key: &K) -> Option<&V> {
        self.active.get(key).map(|(_, value)| value)
    }

    /// 保留满足条件的活动条目; 堆内旧条目延迟清理.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        self.active.retain(|key, (_, value)| keep(key, value));
    }

    pub(super) fn clear(&mut self) {
        self.heap.clear();
        self.active.clear();
    }

    #[must_use]
    pub(super) fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub(super) fn values(&self) -> impl Iterator<Item = &V> {
        self.active.values().map(|(_, value)| value)
    }

    /// 清理堆顶失效条目; 更深的失效条目留到成为堆顶时处理.
    pub(super) fn prune(&mut self) {
        while let Some(Reverse((_, sequence, key))) = self.heap.peek().copied() {
            if self.is_active(key, sequence) {
                break;
            }
            self.heap.pop();
        }
    }

    /// 最近的活动截止时刻.
    pub(super) fn next_deadline(&mut self) -> Option<Instant> {
        self.prune();
        self.heap.peek().map(|Reverse((deadline, ..))| *deadline)
    }

    /// 取出全部已到期条目; 同一键只取最新登记.
    pub(super) fn take_due(&mut self, now: Instant) -> Vec<(K, V)> {
        self.prune();
        let mut due = Vec::new();
        while let Some(Reverse((deadline, sequence, key))) = self.heap.peek().copied() {
            if deadline > now {
                break;
            }
            self.heap.pop();
            if !self.is_active(key, sequence) {
                continue;
            }
            if let Some((_, value)) = self.active.remove(&key) {
                due.push((key, value));
            }
        }
        due
    }

    fn is_active(&self, key: K, sequence: u64) -> bool {
        self.active
            .get(&key)
            .is_some_and(|(active, _)| *active == sequence)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::TimerHeap;

    #[test]
    fn cancelled_and_replaced_entries_are_ignored() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut timers = TimerHeap::<u64, u8>::new();
        timers.insert(deadline, 1, 10);
        timers.insert(deadline, 2, 20);
        timers.remove(&1);
        timers.insert(deadline, 2, 21);

        assert!(!timers.active.contains_key(&1));
        assert!(timers.active.get(&2).is_some_and(|(_, value)| *value == 21));
        timers.prune();
        assert_eq!(timers.heap.len(), 1, "失效条目应被清理");
        assert_eq!(timers.take_due(deadline), vec![(2, 21)]);
        assert_eq!(
            timers.heap.as_slice(),
            [] as [std::cmp::Reverse<(std::time::Instant, u64, u64)>; 0]
        );
        assert!(timers.active.is_empty());
    }

    #[test]
    fn removed_entry_has_no_deadline() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut timers = TimerHeap::<u64, u8>::new();
        timers.insert(deadline, 1, 10);
        timers.remove(&1);
        assert!(timers.next_deadline().is_none());
    }
}
