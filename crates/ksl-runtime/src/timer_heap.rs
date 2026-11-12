//! 通用定时器最小堆.
//!
//! 只负责截止时刻排序, 惰性删除与到期取出. 条目的语义由持有者决定.
//! 任务超时, 共享延迟投递与网络操作超时各自持有实例, 不共享生命周期或取消规则.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;
use std::time::Instant;

/// 失效条目相对活动条目的允许余量; 超过后重建堆.
const STALE_SLACK: usize = 32;

/// 以键区分活动条目的定时器堆.
///
/// 同一键再次登记时旧条目留在堆中, 通过序号比对惰性失效. 键必须可排序,
/// 用于同一截止时刻的稳定顺序.
/// 取消或替换较多时按活动条目重建堆, 避免失效条目长期占用内存.
pub(crate) struct TimerHeap<K, V> {
    heap: BinaryHeap<Reverse<(Instant, u64, K)>>,
    active: HashMap<K, (Instant, u64, V)>,
    next_sequence: u64,
}

impl<K, V> TimerHeap<K, V>
where
    K: Copy + Eq + Hash + Ord,
{
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            active: HashMap::new(),
            next_sequence: 1,
        }
    }

    /// 登记一个定时器; 同一键的旧条目失效.
    pub(crate) fn insert(&mut self, deadline: Instant, key: K, value: V) {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.active.insert(key, (deadline, sequence, value));
        self.heap.push(Reverse((deadline, sequence, key)));
        self.compact();
    }

    /// 取消一个键的活动定时器.
    pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
        let value = self.active.remove(key).map(|(_, _, value)| value);
        self.compact();
        value
    }

    #[must_use]
    pub(crate) fn get(&self, key: &K) -> Option<&V> {
        self.active.get(key).map(|(_, _, value)| value)
    }

    /// 保留满足条件的活动条目; 堆内旧条目延迟清理.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        self.active.retain(|key, (_, _, value)| keep(key, value));
        self.compact();
    }

    pub(crate) fn clear(&mut self) {
        self.heap.clear();
        self.active.clear();
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &V> {
        self.active.values().map(|(_, _, value)| value)
    }

    /// 清理堆顶失效条目; 更深的失效条目留到成为堆顶时处理.
    pub(crate) fn prune(&mut self) {
        while let Some(Reverse((_, sequence, key))) = self.heap.peek().copied() {
            if self.is_active(key, sequence) {
                break;
            }
            self.heap.pop();
        }
    }

    /// 最近的活动截止时刻.
    pub(crate) fn next_deadline(&mut self) -> Option<Instant> {
        self.prune();
        self.heap.peek().map(|Reverse((deadline, ..))| *deadline)
    }

    /// 取出全部已到期条目; 同一键只取最新登记.
    pub(crate) fn take_due(&mut self, now: Instant) -> Vec<(K, V)> {
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
            if let Some((_, _, value)) = self.active.remove(&key) {
                due.push((key, value));
            }
        }
        due
    }

    fn is_active(&self, key: K, sequence: u64) -> bool {
        self.active
            .get(&key)
            .is_some_and(|(_, active, _)| *active == sequence)
    }

    /// 失效条目过多时按活动条目重建堆.
    fn compact(&mut self) {
        if self.heap.len() <= self.active.len().saturating_mul(2) + STALE_SLACK {
            return;
        }
        self.heap = self
            .active
            .iter()
            .map(|(key, (deadline, sequence, _))| Reverse((*deadline, *sequence, *key)))
            .collect();
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
        assert!(timers.active.get(&2).is_some_and(|(_, _, value)| *value == 21));
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
    fn cancelled_entries_are_compacted() {
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut timers = TimerHeap::<u64, u8>::new();
        for key in 0..1000 {
            timers.insert(deadline, key, 0);
            timers.remove(&key);
        }
        assert!(timers.heap.len() <= super::STALE_SLACK, "失效条目应被压缩");
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
