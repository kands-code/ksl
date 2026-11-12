//! 事件分派, 取消, 超时与关闭.

use std::net::Shutdown;
use std::sync::atomic::Ordering;
use std::time::Instant;

use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::Value;
use mio::event::Event;

use super::support::{WAKER, closed_value, timeout_value};
use super::{Engine, Location, Pending};

impl Engine {
    pub(super) fn handle_event(&mut self, event: &Event) {
        let token = event.token();
        if token == WAKER {
            return;
        }
        let id = token.0 as u64;
        if self.listeners.contains_key(&id) {
            if event.is_readable() || event.is_read_closed() || event.is_error() {
                self.service_accept(id);
            }
            return;
        }
        if !self.sockets.contains_key(&id) {
            return;
        }
        if self
            .sockets
            .get(&id)
            .is_some_and(|entry| entry.connect.is_some())
            && (event.is_writable()
                || event.is_read_closed()
                || event.is_write_closed()
                || event.is_error())
        {
            self.service_connect(id);
        }
        if event.is_readable() || event.is_read_closed() || event.is_error() {
            self.service_read(id);
        }
        if event.is_writable() || event.is_write_closed() || event.is_error() {
            self.service_write(id);
        }
    }

    pub(super) fn cancel_op(&mut self, op_id: u64) {
        let Some(location) = self.locations.get(&op_id).copied() else {
            return;
        };
        match location {
            Location::Accept { listener } => {
                if self.remove_accept(listener, op_id).is_some() {
                    self.service_accept(listener);
                }
            },
            Location::Connect { socket } => {
                if let Some(entry) = self.sockets.get_mut(&socket)
                    && entry
                        .connect
                        .as_ref()
                        .is_some_and(|pending| pending.id == op_id)
                {
                    entry.connect = None;
                }
                self.close_socket(socket);
            },
            Location::Read { socket } => {
                if self.remove_read(socket, op_id).is_some() {
                    self.service_read(socket);
                }
            },
            Location::Write { socket } => {
                if self.remove_write(socket, op_id).is_some() {
                    self.service_write(socket);
                }
            },
        }
        self.locations.remove(&op_id);
        self.timers.remove(&op_id);
    }

    pub(super) fn timeout(&mut self, op_id: u64) {
        let Some(location) = self.locations.get(&op_id).copied() else {
            return;
        };
        match location {
            Location::Accept { listener } => {
                if let Some(pending) = self.remove_accept(listener, op_id) {
                    self.finish(pending, timeout_value());
                    self.service_accept(listener);
                }
            },
            Location::Connect { socket } => {
                let pending = self.sockets.get_mut(&socket).and_then(|entry| {
                    if entry
                        .connect
                        .as_ref()
                        .is_some_and(|pending| pending.id == op_id)
                    {
                        entry.connect.take()
                    } else {
                        None
                    }
                });
                if let Some(pending) = pending {
                    self.finish(pending, timeout_value());
                }
                self.close_socket(socket);
            },
            Location::Read { socket } => {
                if let Some(pending) = self.remove_read(socket, op_id) {
                    self.finish(pending, timeout_value());
                    self.service_read(socket);
                }
            },
            Location::Write { socket } => {
                if let Some(pending) = self.remove_write(socket, op_id) {
                    self.finish(pending, timeout_value());
                    self.service_write(socket);
                }
            },
        }
        self.locations.remove(&op_id);
    }

    fn remove_accept(&mut self, listener: u64, op_id: u64) -> Option<Pending> {
        let entry = self.listeners.get_mut(&listener)?;
        let index = entry.accepts.iter().position(|pending| pending.id == op_id)?;
        entry.accepts.remove(index)
    }

    fn remove_read(&mut self, socket: u64, op_id: u64) -> Option<Pending> {
        let entry = self.sockets.get_mut(&socket)?;
        let index = entry
            .read_queue
            .iter()
            .position(|pending| pending.id == op_id)?;
        entry.read_queue.remove(index)
    }

    fn remove_write(&mut self, socket: u64, op_id: u64) -> Option<Pending> {
        let entry = self.sockets.get_mut(&socket)?;
        let index = entry
            .write_queue
            .iter()
            .position(|pending| pending.id == op_id)?;
        entry.write_queue.remove(index)
    }

    pub(super) fn close_socket(&mut self, id: u64) {
        let Some(mut entry) = self.sockets.remove(&id) else {
            return;
        };
        entry.closed.store(true, Ordering::Release);
        let mut pending = Vec::new();
        while let Some(item) = entry.read_queue.pop_front() {
            pending.push(item);
        }
        while let Some(item) = entry.write_queue.pop_front() {
            pending.push(item);
        }
        if let Some(item) = entry.connect.take() {
            pending.push(item);
        }
        let _ = self.poll.registry().deregister(&mut entry.stream);
        let _ = entry.stream.shutdown(Shutdown::Both);
        for item in pending {
            self.finish(item, closed_value());
        }
    }

    pub(super) fn close_listener(&mut self, id: u64) {
        let Some(mut entry) = self.listeners.remove(&id) else {
            return;
        };
        entry.closed.store(true, Ordering::Release);
        let mut pending = Vec::new();
        while let Some(item) = entry.accepts.pop_front() {
            pending.push(item);
        }
        let _ = self.poll.registry().deregister(&mut entry.listener);
        for item in pending {
            self.finish(item, closed_value());
        }
    }

    pub(super) fn fire_timers(&mut self) {
        let due = self.timers.take_due(Instant::now());
        for (op_id, ()) in due {
            self.timeout(op_id);
        }
    }

    pub(super) fn finish(&mut self, pending: Pending, value: Value) {
        self.locations.remove(&pending.id);
        self.timers.remove(&pending.id);
        (pending.done)(Ok(value));
    }

    pub(super) fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub(super) fn fail_all(&mut self) {
        let error = Error::internal("网络引擎已停止");
        let mut pending = Vec::new();
        for (_, mut entry) in self.sockets.drain() {
            while let Some(item) = entry.read_queue.pop_front() {
                pending.push(item);
            }
            while let Some(item) = entry.write_queue.pop_front() {
                pending.push(item);
            }
            if let Some(item) = entry.connect.take() {
                pending.push(item);
            }
        }
        for (_, mut entry) in self.listeners.drain() {
            while let Some(item) = entry.accepts.pop_front() {
                pending.push(item);
            }
        }
        self.locations.clear();
        self.timers.clear();
        for item in pending {
            (item.done)(Err(error.clone()));
        }
    }
}
