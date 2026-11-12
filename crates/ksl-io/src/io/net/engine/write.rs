//! 写操作与关闭写方向.

use std::io::{self, Write};
use std::net::Shutdown;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ksl_value::value::{Value, result_err, result_ok};

use super::super::error::SocketError;
use super::support::{closed_value, error_value, interrupted, timeout_value};
use super::{Done, Engine, Location, Pending, PendingOp, Timeout};

impl Engine {
    pub(super) fn start_write(
        &mut self,
        op_id: u64,
        socket: u64,
        bytes: Vec<u8>,
        all: bool,
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
        if bytes.is_empty() {
            let value = if all {
                result_ok(Value::Nil)
            } else {
                result_ok(Value::U64(0))
            };
            done(Ok(value));
            return;
        }
        let queued = !entry.write_queue.is_empty();
        let mut pending = Pending::new(
            op_id,
            PendingOp::Write {
                bytes,
                all,
                offset: 0,
            },
            done,
            Timeout::Total(timeout),
        );
        if pending.zero_timeout() && queued {
            self.finish(pending, timeout_value());
            return;
        }
        if queued {
            pending.ensure_timer(&mut self.timers);
        }
        self.sockets
            .get_mut(&socket)
            .expect("套接字在取用后消失")
            .write_queue
            .push_back(pending);
        self.locations.insert(op_id, Location::Write { socket });
        self.service_write(socket);
    }

    pub(super) fn start_shutdown_write(
        &mut self,
        op_id: u64,
        socket: u64,
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
        let queued = !entry.write_queue.is_empty();
        let mut pending =
            Pending::new(op_id, PendingOp::ShutdownWrite, done, Timeout::Total(timeout));
        if pending.zero_timeout() && queued {
            self.finish(pending, timeout_value());
            return;
        }
        if queued {
            pending.ensure_timer(&mut self.timers);
        }
        self.sockets
            .get_mut(&socket)
            .expect("套接字在取用后消失")
            .write_queue
            .push_back(pending);
        self.locations.insert(op_id, Location::Write { socket });
        self.service_write(socket);
    }

    pub(super) fn service_write(&mut self, socket: u64) {
        let mut completions = Vec::new();
        {
            let Some(entry) = self.sockets.get_mut(&socket) else {
                return;
            };
            'queue: loop {
                let Some(mut pending) = entry.write_queue.pop_front() else {
                    break;
                };
                if pending.expired() {
                    completions.push((pending, timeout_value()));
                    continue;
                }
                let operation = std::mem::replace(&mut pending.operation, PendingOp::Accept);
                match operation {
                    PendingOp::Write {
                        bytes,
                        all,
                        mut offset,
                    } => loop {
                        if offset == bytes.len() {
                            let value = if all {
                                result_ok(Value::Nil)
                            } else {
                                result_ok(Value::U64(offset as u64))
                            };
                            completions.push((pending, value));
                            break;
                        }
                        if pending.expired() {
                            completions.push((pending, timeout_value()));
                            break;
                        }
                        match entry.stream.write(&bytes[offset..]) {
                            Ok(0) => {
                                let value =
                                    result_err(SocketError::ConnectionClosed.to_value());
                                completions.push((pending, value.clone()));
                                while let Some(pending) = entry.write_queue.pop_front() {
                                    completions.push((pending, value.clone()));
                                }
                                break 'queue;
                            },
                            Ok(count) => {
                                offset += count;
                                if !all {
                                    let value = result_ok(Value::U64(offset as u64));
                                    completions.push((pending, value));
                                    break;
                                }
                            },
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                if pending.zero_timeout() {
                                    completions.push((pending, timeout_value()));
                                    continue 'queue;
                                }
                                pending.ensure_timer(&mut self.timers);
                                pending.operation = PendingOp::Write { bytes, all, offset };
                                entry.write_queue.push_front(pending);
                                break 'queue;
                            },
                            Err(error) if interrupted(&error) => {
                                pending.ensure_timer(&mut self.timers);
                            },
                            Err(error) => {
                                let value = error_value(&error);
                                completions.push((pending, value.clone()));
                                while let Some(pending) = entry.write_queue.pop_front() {
                                    completions.push((pending, value.clone()));
                                }
                                break 'queue;
                            },
                        }
                    },
                    PendingOp::ShutdownWrite => {
                        let value = match entry.stream.shutdown(Shutdown::Write) {
                            Ok(()) => result_ok(Value::Nil),
                            Err(error) => error_value(&error),
                        };
                        completions.push((pending, value));
                    },
                    PendingOp::Accept | PendingOp::Connect | PendingOp::Read { .. } => {
                        unreachable!("写队列包含非写操作");
                    },
                }
            }
        }
        for (pending, value) in completions {
            self.finish(pending, value);
        }
    }
}
