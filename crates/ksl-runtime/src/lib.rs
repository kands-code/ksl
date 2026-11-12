//! ksl 运行时: 线性 IR 执行与调度.
//!
//! 值表示与宿主资源位于 `ksl-value`, I/O 服务位于 `ksl-io`.
//! 本层只保留抽象机, 调度器, 环回收与内置函数; 宿主编排位于 `ksl-host`.

pub mod builtin;
mod runtime;
pub use runtime::{GlobalsCheckpoint, PumpRegistry, RootGuard, Scheduler};
#[cfg(test)]
pub(crate) mod testutil;
