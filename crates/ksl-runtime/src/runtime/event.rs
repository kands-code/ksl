//! 协调器收件箱中的事件.
//!
//! 跨线程状态变化只经此队列回到协调器.
//! 协调器本地产生的状态变化直接执行, 不入队.

use ksl_frontend::error::RuntimeError as Error;

use crate::value::Value;

/// 任务编号; Prelude 固定为 0, Actor 从 1 开始.
pub(crate) type TaskId = u64;

/// 协调器事件.
pub(crate) enum Event {
    /// I/O 完成; `task` 是等待该操作的任务.
    ///
    /// 结果只包含标量, 字符串, 容器与不透明句柄, 不形成环;
    /// 环回收不需要把未处理的 I/O 事件登记为根.
    Io {
        task: TaskId,
        op_id: u64,
        result: Result<Value, Error>,
    },
    /// 发送方投递消息后发现目标在等待该信箱.
    SendWake { target: TaskId, index: usize },
    /// 其他线程投递了调度线程释放任务; 只用于唤醒调度循环.
    Wake,
}
