//! 主动连接与套接字登记.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ksl_value::value::result_ok;
use mio::net::TcpStream;
use mio::{Interest, Token};

use super::super::error::{SocketError, from_io};
use super::read::ReadBuffer;
use super::support::{error_value, interrupted, timeout_value, token};
use super::{Done, Engine, Location, Pending, PendingOp, SocketEntry, Timeout};
use crate::host;
use crate::io::SocketHandle;

impl Engine {
    pub(super) fn start_connect(
        &mut self,
        op_id: u64,
        addr: SocketAddr,
        timeout: Duration,
        done: Done,
    ) {
        let stream = match TcpStream::connect(addr) {
            Ok(stream) => stream,
            Err(error) => {
                done(Ok(error_value(&error)));
                return;
            },
        };
        let id = self.alloc_id();
        let peer = stream
            .peer_addr()
            .map_or_else(|_| addr.to_string(), |peer| peer.to_string());
        let closed = Arc::new(AtomicBool::new(false));
        let mut stream = stream;
        if let Err(error) = self.poll.registry().register(
            &mut stream,
            Token(token(id)),
            Interest::READABLE | Interest::WRITABLE,
        ) {
            done(Ok(error_value(&error)));
            return;
        }
        let mut pending =
            Pending::new(op_id, PendingOp::Connect, done, Timeout::Total(timeout));
        pending.ensure_timer(&mut self.timers);
        self.locations.insert(op_id, Location::Connect { socket: id });
        self.sockets.insert(
            id,
            SocketEntry {
                stream,
                read_buffer: ReadBuffer::default(),
                read_queue: VecDeque::new(),
                write_queue: VecDeque::new(),
                connect: Some(pending),
                closed,
                peer,
            },
        );
        if timeout.is_zero() {
            self.service_connect(id);
            if self
                .sockets
                .get(&id)
                .is_some_and(|entry| entry.connect.is_some())
            {
                if let Some(pending) =
                    self.sockets.get_mut(&id).and_then(|entry| entry.connect.take())
                {
                    self.finish(pending, timeout_value());
                }
                self.close_socket(id);
            }
        } else {
            self.service_connect(id);
        }
    }

    pub(super) fn service_connect(&mut self, socket: u64) {
        let outcome = {
            let Some(entry) = self.sockets.get_mut(&socket) else {
                return;
            };
            let Some(pending) = entry.connect.take() else {
                return;
            };
            let taken = entry.stream.take_error();
            if let Err(error) = &taken
                && interrupted(error)
            {
                entry.connect = Some(pending);
                return;
            }
            match taken {
                Ok(Some(error)) | Err(error) => Some((pending, Err(error))),
                Ok(None) => {
                    if entry.stream.peer_addr().is_ok() {
                        let _ = entry.stream.set_nodelay(true);
                        let handle = Arc::new(SocketHandle::new(
                            self.handle.clone(),
                            socket,
                            entry.peer.clone(),
                            Arc::clone(&entry.closed),
                        ));
                        Some((pending, Ok(handle)))
                    } else {
                        entry.connect = Some(pending);
                        None
                    }
                },
            }
        };
        match outcome {
            Some((pending, Ok(handle))) => {
                self.finish(pending, result_ok(host::socket(handle)));
            },
            Some((pending, Err(error))) => {
                self.finish(pending, error_value(&error));
                self.close_socket(socket);
            },
            None => {},
        }
    }

    pub(super) fn insert_socket(
        &mut self,
        mut stream: TcpStream,
        peer: String,
    ) -> Result<Arc<SocketHandle>, SocketError> {
        let _ = stream.set_nodelay(true);
        let id = self.alloc_id();
        self.poll
            .registry()
            .register(
                &mut stream,
                Token(token(id)),
                Interest::READABLE | Interest::WRITABLE,
            )
            .map_err(|error| from_io(&error))?;
        let closed = Arc::new(AtomicBool::new(false));
        let handle = Arc::new(SocketHandle::new(
            self.handle.clone(),
            id,
            peer.clone(),
            Arc::clone(&closed),
        ));
        self.sockets.insert(
            id,
            SocketEntry {
                stream,
                read_buffer: ReadBuffer::default(),
                read_queue: VecDeque::new(),
                write_queue: VecDeque::new(),
                connect: None,
                closed,
                peer,
            },
        );
        Ok(handle)
    }
}
