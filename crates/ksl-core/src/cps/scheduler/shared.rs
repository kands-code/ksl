//! 调度器共享状态.
//!
//! 命名注册, 监视, 定时器, Actor 启动与 I/O 提交都在共享状态上完成.
//! 事件队列由 I/O 完成与 Actor 原语投递, 调度循环统一消费.
//! 本模块只处理跨任务可见的数据, 不推进抽象机.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Instant;

use crossbeam_channel::Sender;
use parking_lot::Mutex;

use super::timer_heap::TimerHeap;
use crate::check::Type;
use crate::cps::gc::{GcRegistry, Marker};
use crate::error::RuntimeError as Error;
use crate::value::{
    ActorHandle, ActorResult, ExitReason, Value, exit_reason_value, join_result_value,
};

#[derive(Clone)]
pub(crate) struct SchedulerHandle {
    shared: Arc<Mutex<Shared>>,
    gc: Arc<GcRegistry>,
    /// 调度器唤醒通道; 容量为 1, 多次事件合并为一次唤醒.
    wake: Sender<()>,
    /// 事件队列非空标记; 调度器空转时免锁判空, 与事件入队配对使用.
    events_pending: Arc<AtomicBool>,
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

/// 一个等待 Join 的 Actor.
pub(super) struct JoinWaiter {
    pub(super) actor: u64,
}

/// 调度器事件; 由 I/O 完成与 Actor 原语投递到统一事件队列.
pub(super) enum ShardEvent {
    Spawn(SpawnRequest),
    Send {
        target: u64,
        index: usize,
    },
    Kill {
        target: u64,
    },
    Io {
        id: u64,
        result: Result<Value, Error>,
    },
    Join {
        actor: u64,
        value: Value,
    },
}

pub(super) struct Shared {
    next_id: u64,
    next_io: u64,
    next_monitor: u64,
    next_timer: u64,
    /// 调度器待处理的事件.
    pub(super) events: Vec<ShardEvent>,
    /// Prelude 任务的 I/O 完成; Prelude 不是 Actor.
    pub(super) prelude_io: Vec<(u64, Result<Value, Error>)>,
    /// 全部任务共享的命令行参数; 启动 Actor 时读取.
    pub(super) args: Vec<String>,
    io_cancellations: HashMap<u64, Arc<AtomicBool>>,
    /// 命名注册表以运行时字符串为键, 不进入全局符号驻留表.
    names: HashMap<Arc<str>, RegisteredPid>,
    names_by_pid: HashMap<u64, HashSet<Arc<str>>>,
    pub(super) monitors: HashMap<u64, MonitorEntry>,
    pub(super) monitors_by_target: HashMap<u64, HashSet<u64>>,
    pub(super) monitors_by_watcher: HashMap<u64, HashSet<u64>>,
    /// 延迟投递定时器; 取消或目标终止后条目惰性失效.
    pub(super) send_timers: TimerHeap<u64, SendTimer>,
    /// 目标终止时等待 Join 的 Actor; 键是目标编号.
    join_waiters: HashMap<u64, Vec<JoinWaiter>>,
    /// Join 等待的反向索引: 等待者 -> 目标; 一个 Actor 同时只等待一个目标.
    join_targets: HashMap<u64, u64>,
    /// 未观察的 Actor 运行时错误; 由宿主统一取走并渲染.
    pub(super) actor_errors: Vec<(Arc<AtomicBool>, Error)>,
}

impl Shared {
    /// 遍历共享状态中的运行时值; 环回收根标记用.
    pub(super) fn trace(&self, marker: &mut Marker) {
        for event in &self.events {
            match event {
                ShardEvent::Spawn(request) => {
                    marker.mark_actor(&request.handle);
                    marker.mark_value(&request.handler);
                    for arg in &request.args {
                        marker.mark_value(arg);
                    }
                },
                ShardEvent::Send { .. } | ShardEvent::Kill { .. } => {},
                ShardEvent::Io { result, .. } => {
                    if let Ok(value) = result {
                        marker.mark_value(value);
                    }
                },
                ShardEvent::Join { value, .. } => marker.mark_value(value),
            }
        }
        for (_, result) in &self.prelude_io {
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

    /// 分配下一个 Actor 编号.
    pub(super) fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
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
        self.send_timers
            .take_due(now)
            .into_iter()
            .map(|(_, timer)| timer)
            .collect()
    }

    pub(super) fn next_send_deadline(&mut self) -> Option<Instant> {
        self.send_timers.next_deadline()
    }

    fn cleanup_send_timers(&mut self, target_id: u64) {
        self.send_timers.retain(|_, timer| timer.target_id != target_id);
    }

    fn clear_send_timers(&mut self) {
        self.send_timers.clear();
    }

    fn take_join_waiters(&mut self, target_id: u64) -> Vec<JoinWaiter> {
        let Some(waiters) = self.join_waiters.remove(&target_id) else {
            return Vec::new();
        };
        for waiter in &waiters {
            self.join_targets.remove(&waiter.actor);
        }
        waiters
    }

    fn remove_join_waiter(&mut self, actor: u64) {
        let Some(target) = self.join_targets.remove(&actor) else {
            return;
        };
        if let Some(waiters) = self.join_waiters.get_mut(&target) {
            waiters.retain(|waiter| waiter.actor != actor);
            if waiters.is_empty() {
                self.join_waiters.remove(&target);
            }
        }
    }
}

pub(super) struct SpawnRequest {
    pub(super) handle: Arc<ActorHandle>,
    pub(super) handler: Value,
    pub(super) args: Vec<Value>,
}

impl SchedulerHandle {
    pub(super) fn new(gc: Arc<GcRegistry>, wake: Sender<()>) -> (Self, Arc<Mutex<Shared>>) {
        let shared = Arc::new(Mutex::new(Shared {
            next_id: 1,
            next_io: 1,
            next_monitor: 1,
            next_timer: 1,
            events: Vec::new(),
            prelude_io: Vec::new(),
            args: Vec::new(),
            io_cancellations: HashMap::new(),
            names: HashMap::new(),
            names_by_pid: HashMap::new(),
            monitors: HashMap::new(),
            monitors_by_target: HashMap::new(),
            monitors_by_watcher: HashMap::new(),
            send_timers: TimerHeap::new(),
            join_waiters: HashMap::new(),
            join_targets: HashMap::new(),
            actor_errors: Vec::new(),
        }));
        (
            Self {
                shared: Arc::clone(&shared),
                gc,
                wake,
                events_pending: Arc::new(AtomicBool::new(false)),
            },
            shared,
        )
    }

    /// 更新全部任务共享的命令行参数.
    pub(crate) fn set_args(&self, args: Vec<String>) {
        self.shared.lock().args = args;
    }

    /// 调度器共享的环回收注册表.
    #[must_use]
    pub(crate) fn registry(&self) -> &Arc<GcRegistry> {
        &self.gc
    }

    /// 把事件放入事件队列并唤醒调度器.
    pub(super) fn push_event(&self, event: ShardEvent) {
        self.shared.lock().events.push(event);
        // 标记在入队之后发布: 见到标记为真时, 其前置入队必然可见;
        // 入队晚于标记读取的事件由下一轮的标记重新覆盖.
        self.events_pending.store(true, Ordering::Release);
        self.wake();
    }

    /// 取走事件非空标记; 返回 false 表示事件队列必为空, 无需加锁.
    pub(super) fn take_events_pending(&self) -> bool {
        self.events_pending.swap(false, Ordering::Acquire)
    }

    /// 只置事件非空标记, 不唤醒; 事件入队已由调用方在同一临界区完成.
    pub(super) fn mark_events_pending(&self) {
        self.events_pending.store(true, Ordering::Release);
    }

    fn wake(&self) {
        // 容量为 1, 合并重复通知; 运行时被丢弃后的发送失败无需处理.
        let _ = self.wake.try_send(());
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
                self.wake_send(watcher.id, index);
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
                self.wake_send(watcher.id, index);
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

    /// 创建 Actor 句柄并登记启动请求; 返回 Pid 值.
    pub(crate) fn spawn(
        &self,
        parent: Option<&Arc<ActorHandle>>,
        mailboxes: usize,
        detached: bool,
        handler: Value,
        args: Vec<Value>,
    ) -> Value {
        let handle = self.enqueue_spawn(parent, mailboxes, detached, handler, args);
        self.wake();
        Value::Pid(handle)
    }

    fn enqueue_spawn(
        &self,
        parent: Option<&Arc<ActorHandle>>,
        mailboxes: usize,
        detached: bool,
        handler: Value,
        args: Vec<Value>,
    ) -> Arc<ActorHandle> {
        let mut state = self.shared.lock();
        let id = state.alloc_id();
        let parent_handle = if detached { None } else { parent.cloned() };
        let handle = Arc::new(ActorHandle::new(id, mailboxes, parent_handle.clone()));
        self.gc.register_actor(&handle);
        if let Some(parent) = &parent_handle {
            parent.add_child(id);
        }
        state.events.push(ShardEvent::Spawn(SpawnRequest {
            handle: Arc::clone(&handle),
            handler,
            args,
        }));
        // 标记在入队之后发布; 与 drain 端的对齐方式见 push_event.
        self.events_pending.store(true, Ordering::Release);
        handle
    }

    /// 登记 Join 等待; 目标已终止时直接返回结局.
    ///
    /// 检查与登记在同一把共享锁内, 与终止路径的"设置结局后再取等待者"
    /// 配合, 避免在检查与登记之间丢失唤醒.
    pub(crate) fn wait_join(&self, target: &Arc<ActorHandle>, waiter: u64) -> Option<Value> {
        let mut shared = self.shared.lock();
        if let Some(result) = target.result() {
            return Some(join_result_value(&result));
        }
        shared
            .join_waiters
            .entry(target.id)
            .or_default()
            .push(JoinWaiter { actor: waiter });
        shared.join_targets.insert(waiter, target.id);
        None
    }

    /// 取走目标终止时需唤醒的全部 Join 等待者.
    pub(super) fn take_join_waiters(&self, target_id: u64) -> Vec<JoinWaiter> {
        self.shared.lock().take_join_waiters(target_id)
    }

    /// 等待者终止或不再等待时删除登记.
    pub(crate) fn remove_join_waiter(&self, actor: u64) {
        self.shared.lock().remove_join_waiter(actor);
    }

    pub(crate) fn wake_send(&self, target: u64, index: usize) {
        self.push_event(ShardEvent::Send { target, index });
    }

    pub(crate) fn wake_kill(&self, target: u64) {
        self.push_event(ShardEvent::Kill { target });
    }

    /// 把 I/O 任务提交到线程池或专用输入线程; 完成后放入 Actor 或 Prelude 队列.
    pub(crate) fn submit_io(&self, job: crate::io::Task, owner: Option<u64>) -> u64 {
        let cancel = Arc::new(AtomicBool::new(false));
        let id = {
            let mut shared = self.shared.lock();
            let id = shared.next_io;
            shared.next_io += 1;
            shared.io_cancellations.insert(id, Arc::clone(&cancel));
            id
        };
        let shared = Arc::clone(&self.shared);
        let wake = self.wake.clone();
        let events_pending = Arc::clone(&self.events_pending);
        crate::io::submit_task(job, id, cancel, move |result| {
            let mut shared = shared.lock();
            shared.io_cancellations.remove(&id);
            if owner.is_some() {
                shared.events.push(ShardEvent::Io { id, result });
            } else {
                shared.prelude_io.push((id, result));
            }
            drop(shared);
            // 标记在入队之后发布; 与 drain 端的对齐方式见 push_event.
            events_pending.store(true, Ordering::Release);
            // 容量为 1, 合并重复通知; 运行时被丢弃后的发送失败无需处理.
            let _ = wake.try_send(());
        });
        id
    }

    /// 请求取消一个 I/O 操作; 终端读取与网络操作会响应取消标记与通知.
    pub(crate) fn cancel_io(&self, id: u64) {
        if let Some(cancel) = self.shared.lock().io_cancellations.get(&id) {
            cancel.store(true, Ordering::Relaxed);
        }
        crate::io::cancel_task(id);
    }

    pub(crate) fn take_prelude_io(&self) -> Vec<(u64, Result<Value, Error>)> {
        std::mem::take(&mut self.shared.lock().prelude_io)
    }

    pub(crate) fn has_send_timers(&self) -> bool {
        !self.shared.lock().send_timers.is_empty()
    }

    /// 是否还有尚未分发的事件.
    pub(crate) fn has_pending_events(&self) -> bool {
        let shared = self.shared.lock();
        !shared.prelude_io.is_empty() || !shared.events.is_empty()
    }

    pub(crate) fn take_actor_errors(&self) -> Vec<Error> {
        let errors = std::mem::take(&mut self.shared.lock().actor_errors);
        errors
            .into_iter()
            .filter_map(|(observed, error)| {
                if observed.load(Ordering::Relaxed) {
                    None
                } else {
                    Some(error)
                }
            })
            .collect()
    }

    /// 清空共享状态; 调度器关闭时调用.
    pub(crate) fn clear_runtime_state(&self) {
        let pending = {
            let mut shared = self.shared.lock();
            let mut pending = Vec::new();
            for event in std::mem::take(&mut shared.events) {
                if let ShardEvent::Spawn(request) = event {
                    pending.push(request.handle);
                }
            }
            shared.prelude_io.clear();
            shared.io_cancellations.clear();
            shared.actor_errors.clear();
            shared.join_waiters.clear();
            shared.join_targets.clear();
            shared.clear_names();
            shared.clear_monitors();
            shared.clear_send_timers();
            pending
        };
        for handle in pending {
            // 未启动的句柄必须标记终止, 否则 Join 等待者眼中的目标永远是 Running.
            let _ = handle.finish(Err(ExitReason::Killed));
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
