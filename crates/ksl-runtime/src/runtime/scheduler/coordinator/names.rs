//! 协调器的名字注册表.
//!
//! 名字以运行时字符串为键, 不进入全局符号驻留表.
//! 目标终止时按 Pid 反向清理.

use super::{ActorHandle, Arc, ArcStr, Coordinator, Type};

/// 一个名字注册项.
pub(super) struct RegisteredPid {
    pid: Arc<ActorHandle>,
    /// 注册时 Pid 的具体类型; `WhereIs` 按它校验期望类型.
    ty: Type,
}

impl Coordinator {
    pub(super) fn insert_name(&mut self, name: ArcStr, entry: RegisteredPid) {
        let id = entry.pid.id;
        let previous = self.names.insert(name.clone(), entry);
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

    pub(super) fn remove_name(&mut self, name: &str) -> Option<RegisteredPid> {
        let entry = self.names.remove(name)?;
        if let Some(names) = self.names_by_pid.get_mut(&entry.pid.id) {
            names.remove(name);
            if names.is_empty() {
                self.names_by_pid.remove(&entry.pid.id);
            }
        }
        Some(entry)
    }

    pub(super) fn cleanup_names(&mut self, id: u64) {
        if let Some(names) = self.names_by_pid.remove(&id) {
            for name in names {
                self.names.remove(&name);
            }
        }
    }

    pub(super) fn clear_names(&mut self) {
        self.names.clear();
        self.names_by_pid.clear();
    }

    /// 注册名字; 同名同 Pid 同类型视为幂等, 其余冲突返回 false.
    pub(super) fn register_name(
        &mut self,
        name: ArcStr,
        pid: Arc<ActorHandle>,
        ty: Type,
    ) -> bool {
        if pid.is_finished() {
            return false;
        }
        let existing = self
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
            self.remove_name(&name);
        }
        self.insert_name(name, RegisteredPid { pid, ty });
        true
    }

    /// 按名字查找 Pid; 类型标签不匹配或目标已终止时返回 None.
    pub(super) fn where_is_name(
        &mut self,
        name: &str,
        expected: &Type,
    ) -> Option<Arc<ActorHandle>> {
        let entry = self.names.get(name)?;
        if entry.pid.is_finished() {
            self.remove_name(name);
            return None;
        }
        if &entry.ty == expected {
            Some(Arc::clone(&entry.pid))
        } else {
            None
        }
    }

    pub(super) fn unregister_name(&mut self, name: &str) -> bool {
        self.remove_name(name).is_some()
    }
}
