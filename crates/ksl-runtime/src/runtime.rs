//! 运行时执行层.
//!
//! 抽象机执行线性 IR, 调度线程按预算推进任务, 挂起时保存执行状态.
//! 运行时占住驱动它的线程; 网络引擎与工作线程池在其他线程执行 I/O 与线程池调用.
//! 环回收只在调度循环不推进片段时执行.
//! 实现见 `docs/topics/implementation/architecture.md` 第 5 节.

pub(crate) mod event;
pub(crate) mod gc;
pub(crate) mod machine;
pub(crate) mod panic;
pub(crate) mod pump;
pub mod scheduler;
