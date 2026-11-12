//! 引擎共享的标记, 时间与结果辅助.

use std::io;
use std::time::{Duration, Instant};

use ksl_value::value::{Value, result_err};
use mio::Token;

use super::super::error::{SocketError, from_io};

/// 唤醒轮询器的保留标记; 连接编号从 1 开始.
pub(super) const WAKER: Token = Token(usize::MAX);

/// 单次底层读取的块大小.
pub(super) const READ_CHUNK: usize = 64 * 1024;

/// 连接编号转为轮询标记; 编号由引擎单调分配, 不会超出 usize.
pub(super) fn token(id: u64) -> usize {
    usize::try_from(id).expect("网络连接编号超出平台范围")
}

/// 信号中断; 同一操作可以重试.
pub(super) fn interrupted(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Interrupted
}

/// 超时截止时刻; 调用方已校验平台范围.
pub(super) fn deadline(timeout: Duration) -> Instant {
    Instant::now()
        .checked_add(timeout)
        .expect("网络超时超出平台范围")
}

pub(super) fn closed_value() -> Value {
    result_err(SocketError::Closed.to_value())
}

pub(super) fn timeout_value() -> Value {
    result_err(SocketError::TimedOut.to_value())
}

pub(super) fn error_value(error: &io::Error) -> Value {
    result_err(from_io(error).to_value())
}
