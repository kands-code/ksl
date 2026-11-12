//! 抽象机交回协调器的效果请求.
//!
//! 抽象机只决定要执行什么, 不直接修改 Actor 表, 等待表或注册表.
//! 片段遇到效果即结束, 协调器执行效果后把结果交回抽象机, 或让当前任务挂起.

use std::sync::Arc;
use std::time::Instant;

use ksl_base::source::Span;
use ksl_io::io::Task;
use ksl_value::ffi::ForeignFunction;
use ksl_value::value::{ActorHandle, Value};

use super::super::gc::Marker;
use super::Wait;

/// 效果执行后的处置.
pub(crate) enum EffectResult {
    /// 把值交回抽象机并继续执行.
    Resume(Value),
    /// 让当前任务挂起.
    Park(Wait),
    /// 让当前任务以运行时错误结束.
    Fail(ksl_diagnostics::error::RuntimeError),
}

/// 一次需要协调器执行的副作用.
pub(crate) enum Effect {
    Spawn {
        handler: Value,
        mailboxes: usize,
        detached: bool,
        args: Vec<Value>,
    },
    Kill {
        target: Arc<ActorHandle>,
    },
    SetTimer {
        target: Arc<ActorHandle>,
        index: usize,
        message: Value,
        deadline: Instant,
    },
    CancelTimer {
        id: u64,
    },
    SubmitIo {
        task: Task,
    },
    ForeignCall {
        function: Arc<ForeignFunction>,
        args: Vec<Value>,
        span: Span,
    },
}

impl Effect {
    /// 遍历效果中的运行时值; 环回收根标记用.
    pub(crate) fn trace(&self, marker: &mut Marker) {
        match self {
            Self::Spawn { handler, args, .. } => {
                marker.mark_value(handler);
                for arg in args {
                    marker.mark_value(arg);
                }
            },
            Self::SetTimer {
                target, message, ..
            } => {
                marker.mark_actor(target);
                marker.mark_value(message);
            },
            Self::Kill { target } => marker.mark_actor(target),
            Self::SubmitIo { task } => match task {
                Task::Ready(value) => marker.mark_value(value),
                Task::Net { roots, .. } => marker.mark_roots(roots),
                _ => {},
            },
            Self::ForeignCall { args, .. } => {
                for arg in args {
                    marker.mark_value(arg);
                }
            },
            Self::CancelTimer { .. } => {},
        }
    }
}
