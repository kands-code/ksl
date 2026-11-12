//! 管道模式的预读缓冲.
//!
//! 标准输入不是终端时由专用线程持续读取, 消费端从缓冲取行.
//! 缓冲达到上限后读取线程暂停, 为下游施加背压.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use parking_lot::{Condvar, Mutex, MutexGuard};

use super::super::strip_line_ending;
use super::canceled;

/// 预读缓冲上限, 按未消费字节计; 单行自身可以超过上限.
const BUFFER_LIMIT: usize = 4 * 1024;

/// 预读缓冲.
///
/// 单行超过上限时整行仍进入缓冲.
#[derive(Default)]
struct Buffer {
    lines: VecDeque<(String, usize)>,
    bytes: usize,
    closed: bool,
    error: Option<String>,
}

/// 共享预读缓冲.
#[derive(Clone)]
pub(super) struct PipeShared(Arc<(Mutex<Buffer>, Condvar)>);

impl PipeShared {
    fn lock(&self) -> MutexGuard<'_, Buffer> {
        self.0.0.lock()
    }

    fn notify_all(&self) {
        self.0.1.notify_all();
    }

    fn wait(&self, buffer: &mut MutexGuard<'_, Buffer>) {
        self.0.1.wait(buffer);
    }

    fn wait_for(&self, buffer: &mut MutexGuard<'_, Buffer>, timeout: Duration) {
        self.0.1.wait_for(buffer, timeout);
    }
}

/// 新的空预读缓冲.
pub(super) fn shared() -> PipeShared {
    PipeShared(Arc::new((Mutex::new(Buffer::default()), Condvar::new())))
}

/// 预读线程异常退出时的兜底.
///
/// 线程 panic 不会执行循环末尾的收尾; 守卫把缓冲标记为已关闭,
/// 避免消费端在永远不会到来的行上等待.
struct ReaderGuard {
    pipe: PipeShared,
}

impl Drop for ReaderGuard {
    fn drop(&mut self) {
        let mut buffer = self.pipe.lock();
        if !buffer.closed {
            buffer.closed = true;
            buffer.error = Some("管道输入线程异常".to_string());
            self.pipe.notify_all();
        }
    }
}

/// 启动管道预读线程; 只在标准输入不是终端时调用.
pub(super) fn start_reader(pipe: PipeShared) {
    let failure = pipe.clone();
    let started = std::thread::Builder::new()
        .name("ksl-pipe-input".to_owned())
        .spawn(move || {
            let _guard = ReaderGuard { pipe: pipe.clone() };
            loop {
                // 缓冲满时暂停读取; 消费端取行后会唤醒.
                {
                    let mut buffer = pipe.lock();
                    while buffer.bytes >= BUFFER_LIMIT && !buffer.closed {
                        pipe.wait(&mut buffer);
                    }
                    if buffer.closed {
                        break;
                    }
                }
                let mut raw = String::new();
                match std::io::stdin().read_line(&mut raw) {
                    Ok(0) => {
                        let mut buffer = pipe.lock();
                        buffer.closed = true;
                        pipe.notify_all();
                        break;
                    },
                    Ok(_) => {
                        let line = strip_line_ending(raw);
                        let bytes = line.len() + 1;
                        let mut buffer = pipe.lock();
                        buffer.bytes += bytes;
                        buffer.lines.push_back((line, bytes));
                        pipe.notify_all();
                    },
                    Err(error) => {
                        let mut buffer = pipe.lock();
                        buffer.error = Some(error.to_string());
                        buffer.closed = true;
                        pipe.notify_all();
                        break;
                    },
                }
            }
        });
    if started.is_err() {
        let mut buffer = failure.lock();
        buffer.error = Some("无法启动管道输入线程".to_string());
        buffer.closed = true;
        failure.notify_all();
    }
}

/// 管道读取的失败类别; 取消与宿主读取错误走不同的语言层映射.
pub(super) enum PipeError {
    Cancelled,
    Io(String),
}

/// 从管道缓冲取一行; 缓冲为空时等待预读线程.
pub(super) fn take_line(
    cancel: Option<&AtomicBool>,
    pipe: &PipeShared,
) -> Result<Option<String>, PipeError> {
    loop {
        if cancel.is_some_and(canceled) {
            return Err(PipeError::Cancelled);
        }
        let mut buffer = pipe.lock();
        if let Some((line, bytes)) = buffer.lines.pop_front() {
            // 取出后才发现取消: 行退回队列, 不调整字节计数.
            if cancel.is_some_and(canceled) {
                buffer.lines.push_front((line, bytes));
                return Err(PipeError::Cancelled);
            }
            buffer.bytes = buffer.bytes.saturating_sub(bytes);
            pipe.notify_all();
            return Ok(Some(line));
        }
        if buffer.closed {
            return match &buffer.error {
                Some(error) => Err(PipeError::Io(error.clone())),
                None => Ok(None),
            };
        }
        pipe.wait_for(&mut buffer, Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::{PipeError, ReaderGuard, shared, take_line};

    #[test]
    fn reader_guard_closes_waiting_buffer() {
        let pipe = shared();
        drop(ReaderGuard { pipe: pipe.clone() });
        assert!(matches!(take_line(None, &pipe), Err(PipeError::Io(_))));
    }
}
