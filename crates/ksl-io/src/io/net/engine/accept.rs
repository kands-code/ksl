//! 监听与接受连接.

use std::collections::VecDeque;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ksl_value::value::{result_err, result_ok};
use mio::net::{TcpListener, TcpStream};
use mio::{Interest, Token};
use socket2::{Domain, Protocol, Socket, Type};

use super::support::{closed_value, error_value, timeout_value, token};
use super::{Done, Engine, ListenerEntry, Location, Pending, PendingOp, Timeout};
use crate::host;
use crate::io::TcpListenerHandle;

impl Engine {
    pub(super) fn listen(&mut self, addr: SocketAddr, done: Done) {
        let listener = match bind_listener(addr) {
            Ok(listener) => listener,
            Err(error) => {
                done(Ok(error_value(&error)));
                return;
            },
        };
        let port = match listener.local_addr() {
            Ok(addr) => addr.port(),
            Err(error) => {
                done(Ok(error_value(&error)));
                return;
            },
        };
        let id = self.alloc_id();
        let mut listener = listener;
        if let Err(error) =
            self.poll
                .registry()
                .register(&mut listener, Token(token(id)), Interest::READABLE)
        {
            done(Ok(error_value(&error)));
            return;
        }
        let closed = Arc::new(AtomicBool::new(false));
        let handle = TcpListenerHandle::new(self.handle.clone(), id, port, Arc::clone(&closed));
        self.listeners.insert(
            id,
            ListenerEntry {
                listener,
                accepts: VecDeque::new(),
                closed,
            },
        );
        done(Ok(result_ok(host::tcp_listener(Arc::new(handle)))));
    }

    pub(super) fn start_accept(
        &mut self,
        op_id: u64,
        listener: u64,
        timeout: Duration,
        done: Done,
    ) {
        let Some(entry) = self.listeners.get(&listener) else {
            done(Ok(closed_value()));
            return;
        };
        if entry.closed.load(Ordering::Acquire) {
            done(Ok(closed_value()));
            return;
        }
        let queued = !entry.accepts.is_empty();
        let mut pending = Pending::new(op_id, PendingOp::Accept, done, Timeout::Total(timeout));
        if pending.zero_timeout() && queued {
            self.finish(pending, timeout_value());
            return;
        }
        if queued {
            pending.ensure_timer(&mut self.timers);
        }
        self.listeners
            .get_mut(&listener)
            .expect("监听器在取用后消失")
            .accepts
            .push_back(pending);
        self.locations.insert(op_id, Location::Accept { listener });
        self.service_accept(listener);
    }

    pub(super) fn service_accept(&mut self, listener: u64) {
        let mut accepted = Vec::new();
        let mut failures = Vec::new();
        {
            let Some(entry) = self.listeners.get_mut(&listener) else {
                return;
            };
            while !entry.accepts.is_empty() {
                if entry.accepts.front().is_some_and(Pending::expired) {
                    let pending = entry.accepts.pop_front().expect("接受队列非空");
                    failures.push((pending, timeout_value()));
                    continue;
                }
                match entry.listener.accept() {
                    Ok((stream, peer)) => {
                        let pending = entry.accepts.pop_front().expect("接受队列非空");
                        accepted.push((pending, stream, peer));
                    },
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        let zero = entry.accepts.front().is_some_and(Pending::zero_timeout);
                        if zero {
                            let pending = entry.accepts.pop_front().expect("接受队列非空");
                            failures.push((pending, timeout_value()));
                        } else {
                            if let Some(head) = entry.accepts.front_mut() {
                                head.ensure_timer(&mut self.timers);
                            }
                            break;
                        }
                    },
                    Err(error) if transient_accept(&error) => {},
                    Err(error) => {
                        let value = error_value(&error);
                        while let Some(pending) = entry.accepts.pop_front() {
                            failures.push((pending, value.clone()));
                        }
                        break;
                    },
                }
            }
        }
        for (pending, stream, peer) in accepted {
            self.finish_accepted(pending, stream, peer);
        }
        for (pending, value) in failures {
            self.finish(pending, value);
        }
    }

    fn finish_accepted(&mut self, pending: Pending, stream: TcpStream, peer: SocketAddr) {
        match self.insert_socket(stream, peer.to_string()) {
            Ok(handle) => self.finish(pending, result_ok(host::socket(handle))),
            Err(error) => self.finish(pending, result_err(error.to_value())),
        }
    }
}

/// 创建监听器并设置监听选项.
fn bind_listener(addr: SocketAddr) -> io::Result<TcpListener> {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, Some(Protocol::TCP))?;
    // Windows 的 SO_REUSEADDR 允许绑定已被监听的端口, 不随 std 开启.
    #[cfg(not(windows))]
    socket.set_reuse_address(true)?;
    let address = addr.into();
    socket.bind(&address)?;
    socket.listen(128)?;
    socket.set_nonblocking(true)?;
    Ok(TcpListener::from_std(std::net::TcpListener::from(socket)))
}

/// accept 上可重试的错误; 被放弃的连接不代表监听器故障.
fn transient_accept(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::ConnectionReset
    )
}
