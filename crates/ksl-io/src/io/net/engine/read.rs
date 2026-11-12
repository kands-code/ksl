//! 读操作与共享读缓冲.

use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use arcstr::ArcStr;
use ksl_value::value::{List, Value, option_none, option_some, result_err, result_ok};

use super::super::error::SocketError;
use super::support::{closed_value, error_value, interrupted, timeout_value};
use super::{Done, Engine, Location, Pending, PendingOp, ReadMode, Timeout};

impl Engine {
    pub(super) fn start_read(
        &mut self,
        op_id: u64,
        socket: u64,
        mode: ReadMode,
        max: usize,
        timeout: Duration,
        done: Done,
    ) {
        let Some(entry) = self.sockets.get(&socket) else {
            done(Ok(closed_value()));
            return;
        };
        if entry.closed.load(Ordering::Acquire) {
            done(Ok(closed_value()));
            return;
        }
        if matches!(mode, ReadMode::Exact) && max == 0 {
            done(Ok(result_ok(bytes_value(Vec::new()))));
            return;
        }
        let queued = !entry.read_queue.is_empty();
        let timeout = if mode.idle_timeout() {
            Timeout::Idle(timeout)
        } else {
            Timeout::Total(timeout)
        };
        let mut pending = Pending::new(op_id, PendingOp::Read { mode, max }, done, timeout);
        if pending.zero_timeout() && queued {
            self.finish(pending, timeout_value());
            return;
        }
        if queued && !timeout.is_idle() {
            pending.ensure_timer(&mut self.timers);
        }
        self.sockets
            .get_mut(&socket)
            .expect("套接字在取用后消失")
            .read_queue
            .push_back(pending);
        self.locations.insert(op_id, Location::Read { socket });
        self.service_read(socket);
    }

    pub(super) fn service_read(&mut self, socket: u64) {
        let mut completions = Vec::new();
        {
            let Some(entry) = self.sockets.get_mut(&socket) else {
                return;
            };
            loop {
                let Some(mut pending) = entry.read_queue.pop_front() else {
                    break;
                };
                if pending.expired() {
                    completions.push((pending, timeout_value()));
                    continue;
                }
                let PendingOp::Read { mode, max } = &pending.operation else {
                    unreachable!("读队列包含非读操作");
                };
                let (mode, max) = (*mode, *max);
                if let Some(value) = satisfy_read(&mut entry.read_buffer, mode, max) {
                    completions.push((pending, value));
                    continue;
                }
                if entry.read_buffer.eof {
                    let value = eof_read(&mut entry.read_buffer, mode, max);
                    completions.push((pending, value));
                    continue;
                }
                entry.read_buffer.compact();
                let want = mode.read_len(max, entry.read_buffer.buffered()).max(1);
                match entry.stream.read(&mut self.read_chunk[..want]) {
                    Ok(0) => {
                        entry.read_buffer.eof = true;
                        entry.read_queue.push_front(pending);
                    },
                    Ok(count) => {
                        entry
                            .read_buffer
                            .bytes
                            .extend_from_slice(&self.read_chunk[..count]);
                        pending.restart_idle(&mut self.timers);
                        entry.read_queue.push_front(pending);
                    },
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        if pending.zero_timeout() {
                            completions.push((pending, timeout_value()));
                            continue;
                        }
                        pending.ensure_timer(&mut self.timers);
                        entry.read_queue.push_front(pending);
                        break;
                    },
                    Err(error) if interrupted(&error) => {
                        pending.ensure_timer(&mut self.timers);
                        entry.read_queue.push_front(pending);
                    },
                    Err(error) => {
                        let value = error_value(&error);
                        completions.push((pending, value.clone()));
                        while let Some(pending) = entry.read_queue.pop_front() {
                            completions.push((pending, value.clone()));
                        }
                        break;
                    },
                }
            }
        }
        for (pending, value) in completions {
            self.finish(pending, value);
        }
    }
}

/// 按套接字上的读形态取出已有数据.
fn satisfy_read(state: &mut ReadBuffer, mode: ReadMode, max: usize) -> Option<Value> {
    match mode {
        ReadMode::Read => {
            if state.buffered() == 0 {
                None
            } else {
                let count = max.min(state.buffered());
                Some(result_ok(option_some(bytes_value(state.take_bytes(count)))))
            }
        },
        ReadMode::Exact => {
            if state.buffered() >= max {
                Some(result_ok(bytes_value(state.take_bytes(max))))
            } else {
                None
            }
        },
        ReadMode::All => {
            if state.buffered() > max {
                Some(result_err(SocketError::TooLarge.to_value()))
            } else {
                None
            }
        },
        ReadMode::Line => {
            if let Some(position) = state.remaining().iter().position(|byte| *byte == b'\n') {
                if position > max {
                    return Some(result_err(SocketError::LineTooLong.to_value()));
                }
                Some(line_value(state.take_bytes(position + 1)))
            } else if state.buffered() > max {
                Some(result_err(SocketError::LineTooLong.to_value()))
            } else {
                None
            }
        },
    }
}

/// EOF 到达后的读结果.
fn eof_read(state: &mut ReadBuffer, mode: ReadMode, max: usize) -> Value {
    match mode {
        ReadMode::Read => result_ok(option_none()),
        ReadMode::Exact => {
            if state.buffered() >= max {
                result_ok(bytes_value(state.take_bytes(max)))
            } else {
                result_err(SocketError::UnexpectedEof.to_value())
            }
        },
        ReadMode::All => {
            if state.buffered() > max {
                result_err(SocketError::TooLarge.to_value())
            } else {
                result_ok(bytes_value(state.take_remaining()))
            }
        },
        ReadMode::Line => {
            if state.buffered() == 0 {
                result_ok(option_none())
            } else {
                line_value(state.take_remaining())
            }
        },
    }
}

fn bytes_value(bytes: Vec<u8>) -> Value {
    Value::List(Arc::new(List::u8s(bytes)))
}

fn line_value(mut bytes: Vec<u8>) -> Value {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    match String::from_utf8(bytes) {
        Ok(text) => result_ok(option_some(Value::Str(ArcStr::from(text)))),
        Err(_) => result_err(SocketError::InvalidUtf8.to_value()),
    }
}
/// 共享读缓冲.
#[derive(Default)]
pub(super) struct ReadBuffer {
    bytes: Vec<u8>,
    start: usize,
    eof: bool,
}

impl ReadBuffer {
    fn buffered(&self) -> usize {
        self.bytes.len() - self.start
    }

    fn remaining(&self) -> &[u8] {
        &self.bytes[self.start..]
    }

    fn consume(&mut self, count: usize) {
        self.start += count;
    }

    /// 读入新数据前压缩已消费前缀; 全部消费时直接清空.
    fn compact(&mut self) {
        const COMPACT_THRESHOLD: usize = 8192;
        if self.start == self.bytes.len() {
            self.bytes.clear();
            self.start = 0;
        } else if self.start >= COMPACT_THRESHOLD {
            self.bytes.drain(..self.start);
            self.start = 0;
        }
    }

    fn take_bytes(&mut self, count: usize) -> Vec<u8> {
        let bytes = self.remaining()[..count].to_vec();
        self.consume(count);
        bytes
    }

    /// 取走全部未消费缓冲并清空读缓冲.
    fn take_remaining(&mut self) -> Vec<u8> {
        let bytes = self.remaining().to_vec();
        self.bytes.clear();
        self.start = 0;
        bytes
    }
}
