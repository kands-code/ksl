//! ksl 宿主: 把源码加载, 检查与执行串成一条路径.
//!
//! 批处理与交互共用同一套状态; 模块执行与插件加载也在本层.
//! 命令行只负责参数解析与诊断渲染.

mod host;
mod module;
mod plugin;

pub use host::{Host, InputOutcome, RunOutcome};
