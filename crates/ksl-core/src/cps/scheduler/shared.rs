//! 调度器共享状态.
//!
//! 命名注册, 监视, 定时器与 I/O 提交都在共享状态上完成; 调度器循环通过
//! 同一个共享句柄唤醒任务. 本模块只处理跨任务可见的数据, 不推进抽象机.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Instant;

use parking_lot::Mutex;

use crate::check::Type;
use crate::cps::gc::{GcRegistry, Marker};
use crate::error::RuntimeError as Error;
use crate::value::{ActorHandle, ActorResult, ExitReason, Value, exit_reason_value};

#[derive(Clone)]
pub(crate) struct SchedulerHandle {
    shared: Arc<Mutex<Shared>>,
    gc: Arc<GcRegistry>,
}

struct RegisteredPid {
    pid: Arc<ActorHandle>,
    /// 注册时 Pid 的具体类型; `WhereIs` 按它校验期望类型.
    ty: Type,
}

/// 一次监视登记; 监视者用弱引用, 目标只记 id, 避免形成强引用环.
pub(super) struct MonitorEntry {
    watcher: Weak<ActorHandle>,
    watcher_id: u64,
    pub(super) target_id: u64,
    pub(super) index: usize,
}

/// 一次延迟投递; 目标用弱引用, 消息在触发前是环回收根.
pub(super) struct SendTimer {
    pub(super) target: Weak<ActorHandle>,
    pub(super) target_id: u64,
    owner: Option<u64>,
    pub(super) index: usize,
    pub(super) message: Value,
}

pub(super) struct Shared {
    next_id: u64,
    next_io: u64,
    next_monitor: u64,
    next_timer: u64,
    pub(super) spawns: Vec<SpawnRequest>,
    pub(super) wakes: Vec<Wake>,
    pub(super) io_results: Vec<(u64, Result<Value, Error>)>,
    io_cancellations: HashMap<u64, Arc<AtomicBool>>,
    /// 命名注册表以运行时字符串为键, 不进入全局符号驻留表.
    names: HashMap<Arc<str>, RegisteredPid>,
    names_by_pid: HashMap<u64, HashSet<Arc<str>>>,
    pub(super) monitors: HashMap<u64, MonitorEntry>,
    pub(super) monitors_by_target: HashMap<u64, HashSet<u64>>,
    pub(super) monitors_by_watcher: HashMap<u64, HashSet<u64>>,
    pub(super) send_timers: HashMap<u64, SendTimer>,
    /// 延迟投递的最小堆; 取消或目标终止后条目留在堆中, 到堆顶时跳过.
    send_timer_heap: BinaryHeap<Reverse<(Instant, u64)>>,
}

impl Shared {
    /// 遍历待启动的 Actor 与 I/O 结果; 环回收根标记用.
    pub(super) fn trace(&self, marker: &mut Marker) {
        for request in &self.spawns {
            marker.mark_actor(&request.handle);
            marker.mark_value(&request.handler);
            for arg in &request.args {
                marker.mark_value(arg);
            }
        }
        for (_, result) in &self.io_results {
            if let Ok(value) = result {
                marker.mark_value(value);
            }
        }
        for entry in self.names.values() {
            marker.mark_actor(&entry.pid);
        }
        for timer in self.send_timers.values() {
            marker.mark_value(&timer.message);
        }
    }

    fn insert_name(&mut self, name: Arc<str>, entry: RegisteredPid) {
        let id = entry.pid.id;
        let previous = self.names.insert(Arc::clone(&name), entry);
        if let Some(previous) = previous
            && let Some(names) = self.names_by_pid.get_mut(&previous.pid.id)
        {
            names.remove(&name);
            if names.is_empty() {
                self.names_by_pid.remove(&previous.pid.id);
            }
        }
        self.names_by_pid.entry(id).or_default().insert(name);
    }

    fn remove_name(&mut self, name: &str) -> Option<RegisteredPid> {
        let entry = self.names.remove(name)?;
        if let Some(names) = self.names_by_pid.get_mut(&entry.pid.id) {
            names.remove(name);
            if names.is_empty() {
                self.names_by_pid.remove(&entry.pid.id);
            }
        }
        Some(entry)
    }

    fn cleanup_names(&mut self, id: u64) {
        if let Some(names) = self.names_by_pid.remove(&id) {
            for name in names {
                self.names.remove(&name);
            }
        }
    }

    fn clear_names(&mut self) {
        self.names.clear();
        self.names_by_pid.clear();
    }

    fn insert_monitor(&mut self, id: u64, entry: MonitorEntry) {
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

    fn remove_monitor(&mut self, id: u64) -> Option<MonitorEntry> {
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

    fn clear_monitors(&mut self) {
        self.monitors.clear();
        self.monitors_by_target.clear();
        self.monitors_by_watcher.clear();
    }

    pub(super) fn take_due_send_timers(&mut self, now: Instant) -> Vec<SendTimer> {
        let mut due = Vec::new();
        while let Some(Reverse((deadline, id))) = self.send_timer_heap.peek().copied() {
            if deadline > now {
                break;
            }
            self.send_timer_heap.pop();
            if let Some(timer) = self.send_timers.remove(&id) {
                due.push(timer);
            }
        }
        due
    }

    pub(super) fn next_send_deadline(&mut self) -> Option<Instant> {
        while let Some(Reverse((deadline, id))) = self.send_timer_heap.peek().copied() {
            if self.send_timers.contains_key(&id) {
                return Some(deadline);
            }
            self.send_timer_heap.pop();
        }
        None
    }

    fn cleanup_send_timers(&mut self, target_id: u64) {
        self.send_timers.retain(|_, timer| timer.target_id != target_id);
    }

    fn clear_send_timers(&mut self) {
        self.send_timers.clear();
        self.send_timer_heap.clear();
    }
}

pub(super) struct SpawnRequest {
    pub(super) handle: Arc<ActorHandle>,
    pub(super) handler: Value,
    pub(super) args: Vec<Value>,
    /// 发起 Spawn 的输入轮次; 同一轮次内由主脚本及其 Actor 启动的 Actor 共享.
    pub(super) epoch: u64,
}

pub(super) enum Wake {
    Send {
        source: Option<u64>,
        target: u64,
        index: usize,
    },
    Kill {
        target: u64,
    },
}

impl SchedulerHandle {
    pub(super) fn new(gc: Arc<GcRegistry>) -> (Self, Arc<Mutex<Shared>>) {
        let shared = Arc::new(Mutex::new(Shared {
            next_id: 1,
            next_io: 1,
            next_monitor: 1,
            next_timer: 1,
            spawns: Vec::new(),
            wakes: Vec::new(),
            io_results: Vec::new(),
            io_cancellations: HashMap::new(),
            names: HashMap::new(),
            names_by_pid: HashMap::new(),
            monitors: HashMap::new(),
            monitors_by_target: HashMap::new(),
            monitors_by_watcher: HashMap::new(),
            send_timers: HashMap::new(),
            send_timer_heap: BinaryHeap::new(),
        }));
        (
            Self {
                shared: Arc::clone(&shared),
                gc,
            },
            shared,
        )
    }

    /// 调度器共享的环回收注册表.
    #[must_use]
    pub(crate) fn registry(&self) -> &Arc<GcRegistry> {
        &self.gc
    }

    /// 注册名字; 同名同 Pid 同类型视为幂等, 其余冲突返回 false.
    pub(crate) fn register_name(
        &self,
        name: Arc<str>,
        pid: Arc<ActorHandle>,
        ty: Type,
    ) -> bool {
        if pid.is_finished() {
            return false;
        }
        let mut shared = self.shared.lock();
        let existing = shared
            .names
            .get(&*name)
            .map(|entry| (entry.pid.id, entry.pid.is_finished(), entry.ty.clone()));
        if let Some((id, terminated, existing_ty)) = existing {
            if id == pid.id {
                return existing_ty == ty;
            }
            if !terminated {
                return false;
            }
            shared.remove_name(&name);
        }
        shared.insert_name(name, RegisteredPid { pid, ty });
        true
    }

    /// 按名字查找 Pid; 类型标签不匹配或目标已终止时返回 None.
    pub(crate) fn where_is_name(
        &self,
        name: &str,
        expected: &Type,
    ) -> Option<Arc<ActorHandle>> {
        let mut shared = self.shared.lock();
        let entry = shared.names.get(name)?;
        if entry.pid.is_finished() {
            shared.remove_name(name);
            return None;
        }
        if &entry.ty == expected {
            Some(Arc::clone(&entry.pid))
        } else {
            None
        }
    }

    pub(crate) fn unregister_name(&self, name: &str) -> bool {
        self.shared.lock().remove_name(name).is_some()
    }

    pub(crate) fn cleanup_names(&self, id: u64) {
        self.shared.lock().cleanup_names(id);
    }

    pub(crate) fn clear_names(&self) {
        self.shared.lock().clear_names();
    }

    /// 登记一次监视; 目标已终止时立即投递通知.
    ///
    /// 监视自身没有意义, 返回 None. 目标已终止时仍返回编号, 但不再登记,
    /// 避免调用方在 Spawn 与 Monitor 之间丢失终止事件.
    pub(crate) fn monitor(
        &self,
        watcher: &Arc<ActorHandle>,
        target: &Arc<ActorHandle>,
    ) -> Option<u64> {
        if watcher.id == target.id {
            return None;
        }
        // 登记监视即视为观察该 Actor 的失败; 同一次失败不再重复诊断.
        target.mark_observed();
        // 控制信箱固定为最后一个; 检查器已保证其类型为监视通知.
        let index = watcher.last_mailbox_index();
        let mut shared = self.shared.lock();
        let id = shared.next_monitor;
        shared.next_monitor += 1;
        let notice_id = id;
        if let Some(reason) = target.exit_reason() {
            drop(shared);
            let notice = monitor_notice_value(notice_id, &reason);
            if watcher.send(index, notice) {
                self.wake_send(None, watcher.id, index);
            }
            return Some(notice_id);
        }
        shared.insert_monitor(
            id,
            MonitorEntry {
                watcher: Arc::downgrade(watcher),
                watcher_id: watcher.id,
                target_id: target.id,
                index,
            },
        );
        Some(notice_id)
    }

    /// 注销一次监视; 只有登记它的 Actor 可以注销.
    pub(crate) fn unmonitor(&self, id: u64, watcher_id: u64) -> bool {
        let mut shared = self.shared.lock();
        let owned = shared
            .monitors
            .get(&id)
            .is_some_and(|entry| entry.watcher_id == watcher_id);
        if !owned {
            return false;
        }
        shared.remove_monitor(id).is_some()
    }

    /// 目标终止时投递全部监视通知; 监视者已终止的条目直接丢弃.
    pub(crate) fn deliver_monitors(&self, target_id: u64, result: &ActorResult) {
        let actions = {
            let mut shared = self.shared.lock();
            let Some(ids) = shared.monitors_by_target.remove(&target_id) else {
                return;
            };
            let reason = notice_reason(result);
            ids.into_iter()
                .filter_map(|id| {
                    let entry = shared.remove_monitor(id)?;
                    let watcher = entry.watcher.upgrade()?;
                    if watcher.is_finished() {
                        return None;
                    }
                    let notice_id = id;
                    Some((watcher, entry.index, monitor_notice_value(notice_id, &reason)))
                })
                .collect::<Vec<_>>()
        };
        for (watcher, index, notice) in actions {
            if watcher.send(index, notice) {
                self.wake_send(None, watcher.id, index);
            }
        }
    }

    /// 监视者终止时删除它登记的全部监视.
    pub(crate) fn cleanup_monitors(&self, watcher_id: u64) {
        let mut shared = self.shared.lock();
        let Some(ids) = shared.monitors_by_watcher.remove(&watcher_id) else {
            return;
        };
        for id in ids {
            shared.remove_monitor(id);
        }
    }

    pub(crate) fn clear_monitors(&self) {
        self.shared.lock().clear_monitors();
    }

    /// 登记一次延迟投递; 目标已终止时返回 None.
    pub(crate) fn set_timer(
        &self,
        target: &Arc<ActorHandle>,
        index: usize,
        message: Value,
        deadline: Instant,
        owner: Option<u64>,
    ) -> Option<u64> {
        if target.is_finished() {
            return None;
        }
        let mut shared = self.shared.lock();
        let id = shared.next_timer;
        shared.next_timer += 1;
        shared.send_timers.insert(
            id,
            SendTimer {
                target: Arc::downgrade(target),
                target_id: target.id,
                owner,
                index,
                message,
            },
        );
        shared.send_timer_heap.push(Reverse((deadline, id)));
        Some(id)
    }

    /// 取消尚未触发的定时消息; 只有创建者可以取消.
    pub(crate) fn cancel_timer(&self, id: u64, owner: Option<u64>) -> bool {
        let mut shared = self.shared.lock();
        let can_cancel = shared
            .send_timers
            .get(&id)
            .is_some_and(|timer| timer.owner == owner);
        if !can_cancel {
            return false;
        }
        shared.send_timers.remove(&id).is_some()
    }

    pub(crate) fn cleanup_send_timers(&self, target_id: u64) {
        self.shared.lock().cleanup_send_timers(target_id);
    }

    pub(crate) fn clear_send_timers(&self) {
        self.shared.lock().clear_send_timers();
    }

    /// 创建 Actor 句柄并登记启动请求; 返回 Pid 值.
    pub(crate) fn spawn(
        &self,
        parent: Option<&Arc<ActorHandle>>,
        epoch: u64,
        mailboxes: usize,
        detached: bool,
        handler: Value,
        args: Vec<Value>,
    ) -> Value {
        let mut shared = self.shared.lock();
        let id = shared.next_id;
        shared.next_id += 1;
        let parent_handle = if detached { None } else { parent.cloned() };
        let handle = Arc::new(ActorHandle::new(id, mailboxes, parent_handle.clone()));
        self.gc.register_actor(&handle);
        if let Some(parent) = &parent_handle {
            parent.add_child(id);
        }
        shared.spawns.push(SpawnRequest {
            handle: Arc::clone(&handle),
            handler,
            args,
            epoch,
        });
        Value::Pid(handle)
    }

    pub(crate) fn wake_send(&self, source: Option<u64>, target: u64, index: usize) {
        self.shared.lock().wakes.push(Wake::Send {
            source,
            target,
            index,
        });
    }

    pub(crate) fn wake_kill(&self, target: u64) {
        self.shared.lock().wakes.push(Wake::Kill { target });
    }

    /// 把 I/O 任务提交到线程池或专用输入线程; 完成后放入完成队列.
    pub(crate) fn submit_io(&self, job: crate::io::Task) -> (u64, crate::io::IoKind) {
        let kind = job.kind();
        let cancel = Arc::new(AtomicBool::new(false));
        let id = {
            let mut shared = self.shared.lock();
            let id = shared.next_io;
            shared.next_io += 1;
            shared.io_cancellations.insert(id, Arc::clone(&cancel));
            id
        };
        let shared = Arc::clone(&self.shared);
        crate::io::submit_task(job, cancel, move |result| {
            let mut shared = shared.lock();
            shared.io_cancellations.remove(&id);
            shared.io_results.push((id, result));
        });
        (id, kind)
    }

    /// 请求取消一个 I/O 操作; 只有终端读取会协作检查取消标记.
    pub(crate) fn cancel_io(&self, id: u64) {
        if let Some(cancel) = self.shared.lock().io_cancellations.get(&id) {
            cancel.store(true, Ordering::Relaxed);
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
