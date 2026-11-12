//! 协调器的效果执行.
//!
//! 抽象机把 Actor 原语交回协调器; 每个效果结束当前片段, 协调器执行后重新排队.
//! 当前调用内联执行; I/O 与 CPU 调用提交到各自的线程池.

use super::{
    ActorHandle, Arc, Coordinator, Effect, EffectResult, Error, ErrorKind, IoTask, Value, Wait,
    option_none, option_some,
};
use crate::root::RootSet;

impl Coordinator {
    /// 执行抽象机交回的效果; 只由协调器线程调用.
    pub(super) fn apply_effect(
        &mut self,
        current: Option<&Arc<ActorHandle>>,
        task: crate::runtime::event::TaskId,
        effect: Effect,
    ) -> EffectResult {
        match effect {
            Effect::Spawn { .. }
            | Effect::Kill { .. }
            | Effect::Monitor { .. }
            | Effect::Unmonitor { .. } => self.apply_actor_effect(current, effect),
            Effect::Register { .. } | Effect::WhereIs { .. } | Effect::Unregister { .. } => {
                self.apply_registry_effect(effect)
            },
            Effect::SetTimer { .. } | Effect::CancelTimer { .. } => {
                self.apply_timer_effect(current, effect)
            },
            Effect::SubmitIo { .. } => self.apply_io_effect(current, task, effect),
            Effect::ForeignCall { .. } => self.apply_foreign_effect(current, task, effect),
        }
    }

    /// 执行 Actor 生命周期与通信效果.
    pub(super) fn apply_actor_effect(
        &mut self,
        current: Option<&Arc<ActorHandle>>,
        effect: Effect,
    ) -> EffectResult {
        match effect {
            Effect::Spawn {
                handler,
                mailboxes,
                detached,
                args,
            } => EffectResult::Resume(self.spawn(current, mailboxes, detached, handler, args)),
            Effect::Kill { target } => {
                let alive = !target.is_finished();
                if alive {
                    target.request_kill();
                    let current_id = current.map(|handle| handle.id);
                    if current_id == Some(target.id) {
                        // 根 Actor 终止自身是程序级错误; 普通 Actor 在下一个让出点终止.
                        if self.is_root(target.id) {
                            return EffectResult::Fail(Error::runtime(
                                ErrorKind::KillRoot,
                                None,
                            ));
                        }
                    } else {
                        self.kill_actors([target.id]);
                    }
                }
                EffectResult::Resume(Value::Bool(alive))
            },
            Effect::Monitor { target } => {
                let Some(watcher) = current else {
                    return EffectResult::Fail(Error::internal("@Monitor 出现在 Handler 外"));
                };
                match self.monitor(watcher, &target) {
                    Some(id) => EffectResult::Resume(option_some(Value::U64(id))),
                    None => EffectResult::Resume(option_none()),
                }
            },
            Effect::Unmonitor { id } => {
                let Some(watcher) = current else {
                    return EffectResult::Fail(Error::internal("@Unmonitor 出现在 Handler 外"));
                };
                EffectResult::Resume(Value::Bool(self.unmonitor(id, watcher.id)))
            },
            _ => unreachable!("apply_actor_effect 只处理 Actor 效果"),
        }
    }

    /// 执行命名注册表效果.
    pub(super) fn apply_registry_effect(&mut self, effect: Effect) -> EffectResult {
        match effect {
            Effect::Register { name, pid, ty } => {
                EffectResult::Resume(Value::Bool(self.register_name(name, pid, ty)))
            },
            Effect::WhereIs { name, ty } => match self.where_is_name(&name, &ty) {
                Some(pid) => EffectResult::Resume(option_some(Value::Pid(pid))),
                None => EffectResult::Resume(option_none()),
            },
            Effect::Unregister { name } => {
                EffectResult::Resume(Value::Bool(self.unregister_name(&name)))
            },
            _ => unreachable!("apply_registry_effect 只处理注册表效果"),
        }
    }

    /// 执行延迟消息效果.
    pub(super) fn apply_timer_effect(
        &mut self,
        current: Option<&Arc<ActorHandle>>,
        effect: Effect,
    ) -> EffectResult {
        match effect {
            Effect::SetTimer {
                target,
                index,
                message,
                deadline,
            } => {
                let owner = current.map(|handle| handle.id);
                let value = self
                    .set_timer(&target, index, message, deadline, owner)
                    .map_or_else(option_none, |id| option_some(Value::U64(id)));
                EffectResult::Resume(value)
            },
            Effect::CancelTimer { id } => {
                let owner = current.map(|handle| handle.id);
                EffectResult::Resume(Value::Bool(self.cancel_timer(id, owner)))
            },
            _ => unreachable!("apply_timer_effect 只处理定时器效果"),
        }
    }

    /// 执行 I/O 效果.
    pub(super) fn apply_io_effect(
        &mut self,
        current: Option<&Arc<ActorHandle>>,
        task_id: crate::runtime::event::TaskId,
        effect: Effect,
    ) -> EffectResult {
        match effect {
            Effect::SubmitIo { mut task } => {
                let owner = current.map(|handle| handle.id);
                let roots = task.take_roots();
                let op_id = self.handle.submit_io(task, task_id, owner, self.root);
                EffectResult::Park(Wait::Io { op_id, roots })
            },
            _ => unreachable!("apply_io_effect 只处理 I/O 效果"),
        }
    }

    /// 执行外来调用效果.
    ///
    /// I/O 与 CPU 调用提交到线程池, 当前任务挂起, 完成后经 I/O 完成事件恢复.
    fn apply_foreign_effect(
        &mut self,
        current: Option<&Arc<ActorHandle>>,
        task_id: crate::runtime::event::TaskId,
        effect: Effect,
    ) -> EffectResult {
        let Effect::ForeignCall {
            function,
            args,
            span,
        } = effect
        else {
            unreachable!("apply_foreign_effect 只处理外来调用");
        };
        // 工作线程只持弱引用, 不延长投递端在 worker 侧的生命周期.
        let releases = Arc::downgrade(self.releases());
        match function.execution() {
            crate::ffi::Execution::Current => {
                // 抽象机已内联当前调用.
                match function.call(&args, span, &releases) {
                    Ok(value) => EffectResult::Resume(value),
                    Err(error) => EffectResult::Fail(error),
                }
            },
            execution @ (crate::ffi::Execution::Io | crate::ffi::Execution::Cpu) => {
                let owner = current.map(|handle| handle.id);
                // 实参交给线程池后仍作为根保留到操作完成.
                let roots = RootSet::from(args.clone());
                let job = Box::new(move || function.call(&args, span, &releases));
                let task = match execution {
                    crate::ffi::Execution::Cpu => IoTask::Compute(job),
                    _ => IoTask::Blocking(job),
                };
                let op_id = self.handle.submit_io(task, task_id, owner, self.root);
                EffectResult::Park(Wait::Io { op_id, roots })
            },
        }
    }
}
