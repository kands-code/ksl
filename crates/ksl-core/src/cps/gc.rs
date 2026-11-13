//! 环回收.
//!
//! 常规释放由 `Arc` 引用计数完成; 本模块只回收引用计数无法释放的环.
//! 不可变的容器值与标量无法独立成环, 因此任何环都至少经过一个 `Closure`
//! 的捕获向量或 `ActorHandle` 的信箱与结果. 标记从调度器状态, 模块缓存导出值
//! 与宿主额外根出发, 清扫时只清空不可达闭包的捕获与不可达 Actor 句柄的存储,
//! 环被截断后由引用计数释放.
//!
//! 收集器只能在调度器安全点调用: 除传入的根与调度器内部状态外,
//! Rust 栈上不能还有额外的 `Value`, `Closure` 或 `ActorHandle`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::value::{ActorHandle, Capture, Closure, Env, FixCell, Value};

/// 弱引用表达到该长度时清理已失效项, 避免长期会话中无限增长.
const PRUNE_THRESHOLD: usize = 1024;

/// 参与环回收的对象注册表.
///
/// 注册表只保存弱引用, 不延长对象生命周期; 未注册的对象不会被清扫,
/// 但仍会被标记遍历, 因此注册遗漏只会降低回收率, 不会误清活对象.
#[derive(Default)]
pub(crate) struct GcRegistry {
    closures: Mutex<Vec<Weak<Closure>>>,
    actors: Mutex<Vec<Weak<ActorHandle>>>,
    /// 上次收集后新登记的候选对象数, 供自动触发判断.
    allocations: AtomicUsize,
    /// 已执行的收集次数; 供测试确认自动触发确实发生.
    collections: AtomicUsize,
}

impl GcRegistry {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_closure(&self, closure: &Arc<Closure>) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
        let mut closures = self.closures.lock();
        closures.push(Arc::downgrade(closure));
        if closures.len().is_multiple_of(PRUNE_THRESHOLD) {
            prune(&mut closures);
        }
    }

    pub(crate) fn register_actor(&self, actor: &Arc<ActorHandle>) {
        self.allocations.fetch_add(1, Ordering::Relaxed);
        let mut actors = self.actors.lock();
        actors.push(Arc::downgrade(actor));
        if actors.len().is_multiple_of(PRUNE_THRESHOLD) {
            prune(&mut actors);
        }
    }

    /// 断开未标记对象上的强引用; 调用前必须完成标记.
    pub(crate) fn sweep(&self, marker: &Marker) {
        let closures = std::mem::take(&mut *self.closures.lock());
        let mut kept = Vec::with_capacity(closures.len());
        for weak in closures {
            let Some(closure) = weak.upgrade() else {
                continue;
            };
            if marker.closure_marked(&closure) {
                kept.push(weak);
            } else {
                closure.clear_captures();
            }
        }
        *self.closures.lock() = kept;

        let actors = std::mem::take(&mut *self.actors.lock());
        let mut kept = Vec::with_capacity(actors.len());
        for weak in actors {
            let Some(actor) = weak.upgrade() else {
                continue;
            };
            if marker.actor_marked(&actor) {
                kept.push(weak);
            } else {
                actor.clear_for_gc();
            }
        }
        *self.actors.lock() = kept;
        self.allocations.store(0, Ordering::Relaxed);
        self.collections.fetch_add(1, Ordering::Relaxed);
    }

    /// 上次收集后新登记的候选对象数.
    #[must_use]
    pub(crate) fn allocations_since_collection(&self) -> usize {
        self.allocations.load(Ordering::Relaxed)
    }

    /// 已执行的收集次数; 供测试确认自动触发.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn collection_count(&self) -> usize {
        self.collections.load(Ordering::Relaxed)
    }
}

fn prune<T>(items: &mut Vec<Weak<T>>) {
    items.retain(|item| item.strong_count() > 0);
}

/// 标记器: 从根出发记录可达闭包与 Actor 句柄.
pub(crate) struct Marker {
    closures: HashSet<usize>,
    actors: HashSet<usize>,
    /// 已经展开过的 `Data`, `Pair` 与绑定单元.
    objects: HashSet<usize>,
    work: Vec<Work>,
}

enum Work {
    Value(Value),
    Actor(Arc<ActorHandle>),
}

impl Marker {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            closures: HashSet::new(),
            actors: HashSet::new(),
            objects: HashSet::new(),
            work: Vec::new(),
        }
    }

    pub(crate) fn mark_value(&mut self, value: &Value) {
        self.work.push(Work::Value(value.clone()));
    }

    pub(crate) fn mark_actor(&mut self, actor: &Arc<ActorHandle>) {
        self.work.push(Work::Actor(Arc::clone(actor)));
    }

    pub(crate) fn mark_env(&mut self, env: &Env) {
        for value in env.values() {
            self.mark_value(value);
        }
    }

    pub(crate) fn mark_captures(&mut self, captures: &[Capture]) {
        for capture in captures {
            match capture {
                Capture::Value(value) => self.mark_value(value),
                Capture::Fix(cell) => self.mark_fix(cell),
                Capture::Missing(_) => {},
            }
        }
    }

    /// 记录只展开一次的对象; 返回是否首次访问.
    pub(crate) fn mark_object(&mut self, address: usize) -> bool {
        self.objects.insert(address)
    }

    /// 展开工作队列, 直到所有可达对象都已标记.
    pub(crate) fn run(&mut self) {
        while let Some(work) = self.work.pop() {
            match work {
                Work::Value(value) => self.visit_value(value),
                Work::Actor(actor) => self.visit_actor(&actor),
            }
        }
    }

    #[must_use]
    pub(crate) fn closure_marked(&self, closure: &Arc<Closure>) -> bool {
        self.closures.contains(&address(closure))
    }

    #[must_use]
    pub(crate) fn actor_marked(&self, actor: &Arc<ActorHandle>) -> bool {
        self.actors.contains(&address(actor))
    }

    pub(crate) fn mark_fix(&mut self, cell: &FixCell) {
        if !self.mark_object(address(cell)) {
            return;
        }
        if let Some(value) = cell.get() {
            self.mark_value(value);
        }
    }

    fn visit_closure(&mut self, closure: &Arc<Closure>) {
        if !self.closures.insert(address(closure)) {
            return;
        }
        let captures = closure.captures();
        self.mark_captures(&captures);
    }

    fn visit_actor(&mut self, actor: &Arc<ActorHandle>) {
        if !self.actors.insert(address(actor)) {
            return;
        }
        if let Some(parent) = actor.parent_handle() {
            self.mark_actor(parent);
        }
        actor.for_each_stored(&mut |value| self.mark_value(value));
    }

    fn visit_value(&mut self, value: Value) {
        match value {
            Value::Closure(closure) => self.visit_closure(&closure),
            Value::Pid(actor) => self.visit_actor(&actor),
            Value::Data(data) => {
                if !self.mark_object(address(&data)) {
                    return;
                }
                for field in &data.fields {
                    self.mark_value(field);
                }
            },
            Value::Pair(pair) => {
                if !self.mark_object(address(&pair)) {
                    return;
                }
                self.mark_value(&pair.0);
                self.mark_value(&pair.1);
            },
            Value::List(list) => {
                if let crate::value::List::Values(items) = &*list {
                    for item in items {
                        self.mark_value(item);
                    }
                }
            },
            Value::Dict(dict) => {
                for value in dict.values() {
                    self.mark_value(value);
                }
            },
            Value::I64(_)
            | Value::U8(_)
            | Value::U64(_)
            | Value::F64(_)
            | Value::Char(_)
            | Value::Bool(_)
            | Value::Nil
            | Value::Str(_)
            | Value::Builtin(_)
            | Value::Set(_)
            | Value::File(_)
            | Value::Socket(_)
            | Value::TcpListener(_)
            | Value::DateTime(_) => {},
        }
    }
}

fn address<T>(value: &Arc<T>) -> usize {
    Arc::as_ptr(value) as usize
}

#[cfg(test)]
mod tests;
