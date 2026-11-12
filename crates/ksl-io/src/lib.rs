//! ksl 宿主 I/O 服务.
//!
//! 文件, 子进程, 网络, 终端, 时间与正则.
//! 本层依赖值表示, 不依赖抽象机与调度器; 调度器经宿主资源与任务构造器使用本层.

pub mod host;
pub mod io;
pub mod regex;
pub mod terminal;
pub mod time;
pub mod timer_heap;
