//! 运行时执行层.
//!
//! 抽象机运行执行计划, 调度线程按预算推进任务, 挂起时保存执行状态.
//! 运行时占住驱动它的线程; 网络引擎与工作线程池在其他线程执行 I/O 与线程池调用.
//! 环回收只在调度循环不推进片段时执行.
//! 实现见 `docs/topics/implementation/scheduler.md`.

pub(crate) mod event;
pub(crate) mod gc;
pub(crate) mod machine;
pub(crate) mod pump;
pub use pump::PumpRegistry;
pub mod scheduler;
pub use scheduler::{GlobalsCheckpoint, RootGuard, Scheduler};
