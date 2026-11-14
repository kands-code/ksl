//! 非阻塞 TCP 网络服务.
//!
//! 单个事件线程持有全部套接字; 语言层只持有令牌句柄. 挂起中的操作以
//! `Op` 形式登记, 完成时经 `done` 回调回到调度器的 I/O 完成通道.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token, Waker};

use super::error::{SocketError, from_io};
use super::handle::{SocketHandle, TcpListenerHandle};
use super::{NetTask, ReadMode};
use crate::error::RuntimeError as Error;
use crate::value::{List, Value, option_none, option_some, result_err, result_ok};

const WAKER_TOKEN: Token = Token(0);

type Done = super::super::Done;

/// 网络服务句柄: 命令队列与事件线程唤醒器.
pub(super) struct NetService {
    commands: Sender<Command>,
    waker: Arc<Waker>,
}

static SERVICE: OnceLock<NetService> = OnceLock::new();

fn service() -> &'static NetService {
    SERVICE.get_or_init(NetService::start)
}

/// 提交一个网络任务.
pub(super) fn submit(task: NetTask, op_id: u64, cancel: Arc<AtomicBool>, done: Done) {
    let kind = match task {
        NetTask::Listen { host, port } => CommandKind::Listen {
            addr: SocketAddr::new(host, port),
        },
        NetTask::Accept { listener, timeout } => {
            if listener.is_closed() {
                done(Ok(result_err(SocketError::Closed.to_value())));
                return;
            }
            CommandKind::Accept {
                token: listener.token(),
                op_id,
                timeout,
                cancel,
            }
        },
        NetTask::Connect { addr, timeout } => CommandKind::Connect {
            addr,
            op_id,
            timeout,
            cancel,
        },
        NetTask::Read {
            socket,
            mode,
            max,
            timeout,
        } => {
            if socket.is_closed() {
                done(Ok(result_err(SocketError::Closed.to_value())));
                return;
            }
            CommandKind::Read {
                token: socket.token(),
                op_id,
                mode,
                max,
                timeout,
                cancel,
            }
        },
        NetTask::Write {
            socket,
            bytes,
            all,
            timeout,
        } => {
            if socket.is_closed() {
                done(Ok(result_err(SocketError::Closed.to_value())));
                return;
            }
            CommandKind::Write {
                token: socket.token(),
                op_id,
                bytes,
                all,
                timeout,
                cancel,
            }
        },
        NetTask::ShutdownWrite { socket } => {
            if socket.is_closed() {
                done(Ok(result_err(SocketError::Closed.to_value())));
                return;
            }
            CommandKind::ShutdownWrite {
                token: socket.token(),
            }
        },
    };
    send(Command {
        kind,
        done: Some(done),
    });
}

/// 提交关闭命令; 服务未启动或已停止时静默忽略.
pub(super) fn close(token: usize) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let command = Command {
        kind: CommandKind::Close { token },
        done: None,
    };
    if service.commands.send(command).is_ok() {
        let _ = service.waker.wake();
    }
}

/// 取消挂起操作; 调度器已移除等待者, 完成回调直接丢弃.
pub(super) fn cancel(op_id: u64) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    let command = Command {
        kind: CommandKind::Cancel { op_id },
        done: None,
    };
    if service.commands.send(command).is_ok() {
        let _ = service.waker.wake();
    }
}

fn send(command: Command) {
    let service = service();
    if let Err(error) = service.commands.send(command) {
        if let Some(done) = error.into_inner().done {
            done(Err(Error::internal("网络服务已停止")));
        }
        return;
    }
    let _ = service.waker.wake();
}

impl NetService {
    fn start() -> Self {
        let (commands, receiver) = unbounded();
        let poll = Poll::new().expect("无法创建网络事件循环");
        let waker =
            Arc::new(Waker::new(poll.registry(), WAKER_TOKEN).expect("无法创建网络唤醒器"));
        let events = Events::with_capacity(1024);
        thread::Builder::new()
            .name("ksl-net".to_owned())
            .spawn(move || {
                let mut net = NetLoop {
                    poll,
                    events,
                    commands: receiver,
                    sources: HashMap::new(),
                    ops: HashMap::new(),
                    next_token: 1,
                };
                net.run();
            })
            .expect("无法启动网络线程");
        Self { commands, waker }
    }
}

struct Command {
    kind: CommandKind,
    done: Option<Done>,
}

enum CommandKind {
    Listen {
        addr: SocketAddr,
    },
    Accept {
        token: usize,
        op_id: u64,
        timeout: Duration,
        cancel: Arc<AtomicBool>,
    },
    Connect {
        addr: SocketAddr,
        op_id: u64,
        timeout: Duration,
        cancel: Arc<AtomicBool>,
    },
    Read {
        token: usize,
        op_id: u64,
        mode: ReadMode,
        max: usize,
        timeout: Duration,
        cancel: Arc<AtomicBool>,
    },
    Write {
        token: usize,
        op_id: u64,
        bytes: Vec<u8>,
        all: bool,
        timeout: Duration,
        cancel: Arc<AtomicBool>,
    },
    Close {
        token: usize,
    },
    Cancel {
        op_id: u64,
    },
    ShutdownWrite {
        token: usize,
    },
}

struct Request {
    op_id: u64,
    timeout: Duration,
    cancel: Arc<AtomicBool>,
    done: Done,
}

struct Op {
    token: usize,
    kind: OpKind,
    timeout: Duration,
    deadline: Instant,
    done: Option<Done>,
    cancel: Arc<AtomicBool>,
}

enum OpKind {
    Accept,
    Connect,
    Read {
        mode: ReadMode,
        max: usize,
    },
    Write {
        bytes: Vec<u8>,
        offset: usize,
        all: bool,
    },
}

impl Op {
    fn new(
        token: usize,
        kind: OpKind,
        timeout: Duration,
        done: Done,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            token,
            kind,
            timeout,
            deadline: deadline_after(timeout),
            done: Some(done),
            cancel,
        }
    }

    fn is_canceled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    fn expired(&self, now: Instant) -> bool {
        self.deadline <= now
    }

    fn on_progress(&mut self) {
        if !self.timeout.is_zero() {
            self.deadline = deadline_after(self.timeout);
        }
    }
}

fn deadline_after(timeout: Duration) -> Instant {
    let now = Instant::now();
    if timeout.is_zero() {
        now
    } else {
        now.checked_add(timeout).unwrap_or(now)
    }
}

enum Source {
    Listener(ListenerState),
    Stream(StreamState),
    Connecting(ConnectingState),
}

#[derive(Clone, Copy)]
enum SourceKind {
    Listener,
    Stream,
    Connecting,
}

struct ListenerState {
    socket: TcpListener,
    accepts: VecDeque<u64>,
    connections: VecDeque<usize>,
    registered: bool,
}

struct StreamState {
    socket: TcpStream,
    peer: String,
    read_buffer: Vec<u8>,
    /// 已消费前缀长度; 读缓冲的有效内容从该下标开始.
    read_start: usize,
    eof: bool,
    reads: VecDeque<u64>,
    writes: VecDeque<u64>,
    registered: bool,
}

/// 读缓冲压缩阈值: 已消费前缀达到该值时在下次读入前压缩.
const READ_BUFFER_COMPACT: usize = 8192;

impl StreamState {
    fn new(socket: TcpStream, peer: String) -> Self {
        // 交互式协议的小写入对延迟敏感, 接受与连接的流一律禁用 Nagle 算法;
        // 设置失败只损失延迟优化, 不影响正确性.
        let _ = socket.set_nodelay(true);
        Self {
            socket,
            peer,
            read_buffer: Vec::new(),
            read_start: 0,
            eof: false,
            reads: VecDeque::new(),
            writes: VecDeque::new(),
            registered: false,
        }
    }

    /// 已缓冲未消费的字节数.
    fn buffered(&self) -> usize {
        self.read_buffer.len() - self.read_start
    }

    /// 未消费的缓冲内容.
    fn buffer(&self) -> &[u8] {
        &self.read_buffer[self.read_start..]
    }

    /// 消费前 `count` 个已缓冲字节.
    fn consume(&mut self, count: usize) {
        self.read_start += count;
    }

    /// 读入新数据前压缩已消费前缀; 全部消费时直接清空.
    fn compact_read_buffer(&mut self) {
        if self.read_start == self.read_buffer.len() {
            self.read_buffer.clear();
            self.read_start = 0;
        } else if self.read_start >= READ_BUFFER_COMPACT {
            self.read_buffer.drain(..self.read_start);
            self.read_start = 0;
        }
    }
}

struct ConnectingState {
    socket: TcpStream,
    op: u64,
    registered: bool,
}

struct NetLoop {
    poll: Poll,
    events: Events,
    commands: Receiver<Command>,
    sources: HashMap<usize, Source>,
    ops: HashMap<u64, Op>,
    next_token: usize,
}

impl NetLoop {
    fn run(&mut self) {
        loop {
            self.drain_commands();
            self.expire_deadlines();
            let timeout = self.next_timeout();
            match self.poll.poll(&mut self.events, timeout) {
                Ok(()) => {},
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    self.fail_all(&Error::internal(format!("网络事件循环失败: {error}")));
                    return;
                },
            }
            self.handle_events();
            self.expire_deadlines();
        }
    }

    fn drain_commands(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            self.handle_command(command);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_command(&mut self, command: Command) {
        let Command { kind, mut done } = command;
        match kind {
            CommandKind::Listen { addr } => {
                let Some(done) = done.take() else {
                    return;
                };
                match TcpListener::bind(addr) {
                    Ok(socket) => {
                        let token = self.new_token();
                        let local_port = socket.local_addr().map_or(0, |addr| addr.port());
                        self.sources.insert(
                            token,
                            Source::Listener(ListenerState {
                                socket,
                                accepts: VecDeque::new(),
                                connections: VecDeque::new(),
                                registered: false,
                            }),
                        );
                        let handle = Arc::new(TcpListenerHandle::new(token, local_port));
                        done(Ok(result_ok(Value::TcpListener(handle))));
                    },
                    Err(error) => {
                        done(Ok(result_err(from_io(&error).to_value())));
                    },
                }
            },
            CommandKind::Accept {
                token,
                op_id,
                timeout,
                cancel,
            } => {
                let Some(done) = done.take() else {
                    return;
                };
                self.enqueue_accept(
                    token,
                    Request {
                        op_id,
                        timeout,
                        cancel,
                        done,
                    },
                );
            },
            CommandKind::Connect {
                addr,
                op_id,
                timeout,
                cancel,
            } => {
                let Some(done) = done.take() else {
                    return;
                };
                self.enqueue_connect(
                    addr,
                    Request {
                        op_id,
                        timeout,
                        cancel,
                        done,
                    },
                );
            },
            CommandKind::Read {
                token,
                op_id,
                mode,
                max,
                timeout,
                cancel,
            } => {
                let Some(done) = done.take() else {
                    return;
                };
                self.enqueue_read(
                    token,
                    Request {
                        op_id,
                        timeout,
                        cancel,
                        done,
                    },
                    mode,
                    max,
                );
            },
            CommandKind::Write {
                token,
                op_id,
                bytes,
                all,
                timeout,
                cancel,
            } => {
                let Some(done) = done.take() else {
                    return;
                };
                self.enqueue_write(
                    token,
                    Request {
                        op_id,
                        timeout,
                        cancel,
                        done,
                    },
                    bytes,
                    all,
                );
            },
            CommandKind::Close { token } => self.close_source(token),
            CommandKind::Cancel { op_id } => self.cancel_op(op_id),
            CommandKind::ShutdownWrite { token } => {
                let Some(done) = done.take() else {
                    return;
                };
                let Some(Source::Stream(state)) = self.sources.get_mut(&token) else {
                    done(Ok(result_err(SocketError::Closed.to_value())));
                    return;
                };
                match state.socket.shutdown(Shutdown::Write) {
                    Ok(()) => done(Ok(result_ok(Value::Nil))),
                    Err(error) => done(Ok(result_err(from_io(&error).to_value()))),
                }
            },
        }
    }

    fn new_token(&mut self) -> usize {
        let token = self.next_token;
        self.next_token = self.next_token.checked_add(1).unwrap_or(1);
        token
    }

    fn enqueue_accept(&mut self, token: usize, request: Request) {
        let Request {
            op_id,
            timeout,
            cancel,
            done,
        } = request;
        if cancel.load(Ordering::Acquire) {
            return;
        }
        let Some(Source::Listener(state)) = self.sources.get_mut(&token) else {
            done(Ok(result_err(SocketError::Closed.to_value())));
            return;
        };
        if let Some(connection) = state.connections.pop_front() {
            let peer = self.source_peer(connection);
            let handle = Arc::new(SocketHandle::new(connection, peer));
            done(Ok(result_ok(Value::Socket(handle))));
            return;
        }
        let op = Op::new(token, OpKind::Accept, timeout, done, cancel);
        self.ops.insert(op_id, op);
        if let Some(Source::Listener(state)) = self.sources.get_mut(&token) {
            state.accepts.push_back(op_id);
        }
        self.update_interest(token);
        if timeout.is_zero() {
            self.accept_ready(token);
        }
    }

    fn enqueue_connect(&mut self, addr: SocketAddr, request: Request) {
        let Request {
            op_id,
            timeout,
            cancel,
            done,
        } = request;
        if cancel.load(Ordering::Acquire) {
            return;
        }
        let socket = match TcpStream::connect(addr) {
            Ok(socket) => socket,
            Err(error) => {
                done(Ok(result_err(from_io(&error).to_value())));
                return;
            },
        };
        let token = self.new_token();
        match socket.peer_addr() {
            Ok(peer) => {
                let peer = peer.to_string();
                self.sources
                    .insert(token, Source::Stream(StreamState::new(socket, peer.clone())));
                let handle = Arc::new(SocketHandle::new(token, peer));
                done(Ok(result_ok(Value::Socket(handle))));
                self.update_interest(token);
            },
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::NotConnected
                ) =>
            {
                let op = Op::new(token, OpKind::Connect, timeout, done, cancel);
                self.ops.insert(op_id, op);
                self.sources.insert(
                    token,
                    Source::Connecting(ConnectingState {
                        socket,
                        op: op_id,
                        registered: false,
                    }),
                );
                self.update_interest(token);
            },
            Err(error) => {
                done(Ok(result_err(from_io(&error).to_value())));
            },
        }
    }

    fn enqueue_read(&mut self, token: usize, request: Request, mode: ReadMode, max: usize) {
        let Request {
            op_id,
            timeout,
            cancel,
            done,
        } = request;
        if cancel.load(Ordering::Acquire) {
            return;
        }
        if !matches!(self.sources.get(&token), Some(Source::Stream(_))) {
            done(Ok(result_err(SocketError::Closed.to_value())));
            return;
        }
        let op = Op::new(token, OpKind::Read { mode, max }, timeout, done, cancel);
        self.ops.insert(op_id, op);
        if let Some(Source::Stream(state)) = self.sources.get_mut(&token) {
            state.reads.push_back(op_id);
        }
        self.process_reads(token);
    }

    fn enqueue_write(&mut self, token: usize, request: Request, bytes: Vec<u8>, all: bool) {
        let Request {
            op_id,
            timeout,
            cancel,
            done,
        } = request;
        if cancel.load(Ordering::Acquire) {
            return;
        }
        if !matches!(self.sources.get(&token), Some(Source::Stream(_))) {
            done(Ok(result_err(SocketError::Closed.to_value())));
            return;
        }
        let op = Op::new(
            token,
            OpKind::Write {
                bytes,
                offset: 0,
                all,
            },
            timeout,
            done,
            cancel,
        );
        self.ops.insert(op_id, op);
        if let Some(Source::Stream(state)) = self.sources.get_mut(&token) {
            state.writes.push_back(op_id);
        }
        self.process_writes(token);
    }

    fn process_reads(&mut self, token: usize) {
        let Some(source) = self.sources.remove(&token) else {
            return;
        };
        let Source::Stream(mut state) = source else {
            self.sources.insert(token, source);
            return;
        };
        while let Some(&op_id) = state.reads.front() {
            let Some(mut op) = self.ops.remove(&op_id) else {
                state.reads.pop_front();
                continue;
            };
            if op.is_canceled() {
                state.reads.pop_front();
                continue;
            }
            if !op.timeout.is_zero() && op.expired(Instant::now()) {
                state.reads.pop_front();
                finish_error(op, &SocketError::TimedOut);
                continue;
            }
            match try_read(&mut state, &mut op) {
                ReadOutcome::Complete(value) => {
                    state.reads.pop_front();
                    finish_ok(op, value);
                },
                ReadOutcome::Error(error) => {
                    state.reads.pop_front();
                    finish_error(op, &error);
                },
                ReadOutcome::Pending => {
                    if op.timeout.is_zero() {
                        state.reads.pop_front();
                        finish_error(op, &SocketError::TimedOut);
                    } else {
                        self.ops.insert(op_id, op);
                        break;
                    }
                },
            }
        }
        self.sources.insert(token, Source::Stream(state));
        self.update_interest(token);
    }

    fn process_writes(&mut self, token: usize) {
        let Some(source) = self.sources.remove(&token) else {
            return;
        };
        let Source::Stream(mut state) = source else {
            self.sources.insert(token, source);
            return;
        };
        while let Some(&op_id) = state.writes.front() {
            let Some(mut op) = self.ops.remove(&op_id) else {
                state.writes.pop_front();
                continue;
            };
            if op.is_canceled() {
                state.writes.pop_front();
                continue;
            }
            if !op.timeout.is_zero() && op.expired(Instant::now()) {
                state.writes.pop_front();
                finish_error(op, &SocketError::TimedOut);
                continue;
            }
            match try_write(&mut state, &mut op) {
                WriteOutcome::Complete(value) => {
                    state.writes.pop_front();
                    finish_ok(op, value);
                },
                WriteOutcome::Error(error) => {
                    state.writes.pop_front();
                    finish_error(op, &error);
                },
                WriteOutcome::Pending => {
                    if op.timeout.is_zero() {
                        state.writes.pop_front();
                        finish_error(op, &SocketError::TimedOut);
                    } else {
                        self.ops.insert(op_id, op);
                        break;
                    }
                },
            }
        }
        self.sources.insert(token, Source::Stream(state));
        self.update_interest(token);
    }

    fn accept_ready(&mut self, token: usize) {
        let Some(source) = self.sources.remove(&token) else {
            return;
        };
        let Source::Listener(mut state) = source else {
            self.sources.insert(token, source);
            return;
        };
        loop {
            match state.socket.accept() {
                Ok((stream, addr)) => {
                    let connection = self.new_token();
                    let peer = addr.to_string();
                    self.sources
                        .insert(connection, Source::Stream(StreamState::new(stream, peer)));
                    let Some(op_id) = state.accepts.pop_front() else {
                        state.connections.push_back(connection);
                        continue;
                    };
                    let Some(op) = self.ops.remove(&op_id) else {
                        state.connections.push_back(connection);
                        continue;
                    };
                    if op.is_canceled() {
                        state.connections.push_back(connection);
                        continue;
                    }
                    if !op.timeout.is_zero() && op.expired(Instant::now()) {
                        state.connections.push_back(connection);
                        finish_error(op, &SocketError::TimedOut);
                        continue;
                    }
                    let peer = self.source_peer(connection);
                    let handle = Arc::new(SocketHandle::new(connection, peer));
                    finish_ok(op, Value::Socket(handle));
                },
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                Err(error) => {
                    let socket_error = from_io(&error);
                    while let Some(op_id) = state.accepts.pop_front() {
                        if let Some(op) = self.ops.remove(&op_id) {
                            finish_error(op, &socket_error);
                        }
                    }
                    break;
                },
            }
        }
        self.sources.insert(token, Source::Listener(state));
        self.update_interest(token);
    }

    fn connect_ready(&mut self, token: usize) {
        let Some(source) = self.remove_source(token) else {
            return;
        };
        let Source::Connecting(state) = source else {
            self.sources.insert(token, source);
            return;
        };
        let ConnectingState {
            socket,
            op: connect_op,
            ..
        } = state;
        let Some(op) = self.ops.remove(&connect_op) else {
            return;
        };
        if op.is_canceled() {
            return;
        }
        if !op.timeout.is_zero() && op.expired(Instant::now()) {
            finish_error(op, &SocketError::TimedOut);
            return;
        }
        match socket.take_error() {
            Ok(Some(error)) | Err(error) => {
                finish_error(op, &from_io(&error));
                return;
            },
            Ok(None) => {},
        }
        match socket.peer_addr() {
            Ok(peer) => {
                let peer = peer.to_string();
                self.sources
                    .insert(token, Source::Stream(StreamState::new(socket, peer.clone())));
                let handle = Arc::new(SocketHandle::new(token, peer));
                finish_ok(op, Value::Socket(handle));
                self.update_interest(token);
            },
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::NotConnected
                ) =>
            {
                self.ops.insert(connect_op, op);
                self.sources.insert(
                    token,
                    Source::Connecting(ConnectingState {
                        socket,
                        op: connect_op,
                        registered: false,
                    }),
                );
                self.update_interest(token);
            },
            Err(error) => {
                finish_error(op, &from_io(&error));
            },
        }
    }

    fn handle_events(&mut self) {
        let events: Vec<(usize, bool, bool)> = self
            .events
            .iter()
            .map(|event| {
                (
                    event.token().0,
                    event.is_readable() || event.is_read_closed() || event.is_error(),
                    event.is_writable() || event.is_write_closed() || event.is_error(),
                )
            })
            .collect();
        for (token, readable, writable) in events {
            if token == WAKER_TOKEN.0 {
                continue;
            }
            let kind = match self.sources.get(&token) {
                Some(Source::Listener(_)) => SourceKind::Listener,
                Some(Source::Stream(_)) => SourceKind::Stream,
                Some(Source::Connecting(_)) => SourceKind::Connecting,
                None => continue,
            };
            match kind {
                SourceKind::Listener => self.accept_ready(token),
                SourceKind::Stream => {
                    if readable {
                        self.process_reads(token);
                    }
                    if writable {
                        self.process_writes(token);
                    }
                },
                SourceKind::Connecting => self.connect_ready(token),
            }
        }
    }

    fn next_timeout(&self) -> Option<Duration> {
        let now = Instant::now();
        self.ops
            .values()
            .map(|op| op.deadline.saturating_duration_since(now))
            .min()
    }

    fn expire_deadlines(&mut self) {
        let now = Instant::now();
        let expired: Vec<u64> = self
            .ops
            .iter()
            .filter(|(_, op)| op.expired(now))
            .map(|(id, _)| *id)
            .collect();
        for op_id in expired {
            self.timeout_op(op_id);
        }
    }

    fn timeout_op(&mut self, op_id: u64) {
        let Some(op) = self.ops.remove(&op_id) else {
            return;
        };
        let token = op.token;
        match op.kind {
            OpKind::Accept => {
                if let Some(Source::Listener(state)) = self.sources.get_mut(&token) {
                    state.accepts.retain(|id| *id != op_id);
                }
            },
            OpKind::Connect => {
                self.remove_source(token);
            },
            OpKind::Read { .. } => {
                if let Some(Source::Stream(state)) = self.sources.get_mut(&token) {
                    state.reads.retain(|id| *id != op_id);
                }
            },
            OpKind::Write { .. } => {
                if let Some(Source::Stream(state)) = self.sources.get_mut(&token) {
                    state.writes.retain(|id| *id != op_id);
                }
            },
        }
        finish_error(op, &SocketError::TimedOut);
        self.update_interest(token);
    }

    fn cancel_op(&mut self, op_id: u64) {
        let Some(op) = self.ops.remove(&op_id) else {
            return;
        };
        let token = op.token;
        match op.kind {
            OpKind::Accept => {
                if let Some(Source::Listener(state)) = self.sources.get_mut(&token) {
                    state.accepts.retain(|id| *id != op_id);
                }
            },
            OpKind::Connect => {
                self.remove_source(token);
            },
            OpKind::Read { .. } => {
                if let Some(Source::Stream(state)) = self.sources.get_mut(&token) {
                    state.reads.retain(|id| *id != op_id);
                }
            },
            OpKind::Write { .. } => {
                if let Some(Source::Stream(state)) = self.sources.get_mut(&token) {
                    state.writes.retain(|id| *id != op_id);
                }
            },
        }
        self.update_interest(token);
    }

    fn remove_source(&mut self, token: usize) -> Option<Source> {
        let mut source = self.sources.remove(&token)?;
        let registry = self.poll.registry();
        match &mut source {
            Source::Listener(state) => {
                deregister(registry, &mut state.socket, &mut state.registered);
            },
            Source::Stream(state) => {
                deregister(registry, &mut state.socket, &mut state.registered);
            },
            Source::Connecting(state) => {
                deregister(registry, &mut state.socket, &mut state.registered);
            },
        }
        Some(source)
    }

    fn close_source(&mut self, token: usize) {
        let Some(source) = self.remove_source(token) else {
            return;
        };
        match source {
            Source::Listener(mut state) => {
                while let Some(op_id) = state.accepts.pop_front() {
                    if let Some(op) = self.ops.remove(&op_id) {
                        finish_error(op, &SocketError::Closed);
                    }
                }
                while let Some(connection) = state.connections.pop_front() {
                    self.remove_source(connection);
                }
            },
            Source::Stream(state) => {
                for op_id in state.reads.into_iter().chain(state.writes) {
                    if let Some(op) = self.ops.remove(&op_id) {
                        finish_error(op, &SocketError::Closed);
                    }
                }
            },
            Source::Connecting(state) => {
                if let Some(op) = self.ops.remove(&state.op) {
                    finish_error(op, &SocketError::Closed);
                }
            },
        }
    }

    fn update_interest(&mut self, token: usize) {
        let registry = self.poll.registry();
        let Some(source) = self.sources.get_mut(&token) else {
            return;
        };
        match source {
            Source::Listener(state) => {
                if state.accepts.is_empty() {
                    deregister(registry, &mut state.socket, &mut state.registered);
                } else {
                    register(
                        registry,
                        &mut state.socket,
                        token,
                        Interest::READABLE,
                        &mut state.registered,
                    );
                }
            },
            Source::Stream(state) => {
                let read = !state.reads.is_empty() && !state.eof;
                let write = !state.writes.is_empty();
                let interest = match (read, write) {
                    (true, true) => Some(Interest::READABLE | Interest::WRITABLE),
                    (true, false) => Some(Interest::READABLE),
                    (false, true) => Some(Interest::WRITABLE),
                    (false, false) => None,
                };
                if let Some(interest) = interest {
                    register(
                        registry,
                        &mut state.socket,
                        token,
                        interest,
                        &mut state.registered,
                    );
                } else {
                    deregister(registry, &mut state.socket, &mut state.registered);
                }
            },
            Source::Connecting(state) => {
                register(
                    registry,
                    &mut state.socket,
                    token,
                    Interest::WRITABLE,
                    &mut state.registered,
                );
            },
        }
    }

    fn source_peer(&self, token: usize) -> String {
        match self.sources.get(&token) {
            Some(Source::Stream(state)) => state.peer.clone(),
            _ => String::new(),
        }
    }

    fn fail_all(&mut self, error: &Error) {
        let ops: Vec<Op> = self.ops.drain().map(|(_, op)| op).collect();
        for op in ops {
            if let Some(done) = op.done {
                done(Err(error.clone()));
            }
        }
        self.sources.clear();
    }
}

fn register(
    registry: &mio::Registry,
    socket: &mut impl mio::event::Source,
    token: usize,
    interest: Interest,
    registered: &mut bool,
) {
    let result = if *registered {
        registry.reregister(socket, Token(token), interest)
    } else {
        registry.register(socket, Token(token), interest)
    };
    if result.is_ok() {
        *registered = true;
    }
}

fn deregister(
    registry: &mio::Registry,
    socket: &mut impl mio::event::Source,
    registered: &mut bool,
) {
    if *registered {
        let _ = registry.deregister(socket);
        *registered = false;
    }
}

fn finish_ok(op: Op, value: Value) {
    if let Some(done) = op.done {
        done(Ok(result_ok(value)));
    }
}

fn finish_error(op: Op, error: &SocketError) {
    if let Some(done) = op.done {
        done(Ok(result_err(error.to_value())));
    }
}

enum ReadOutcome {
    Complete(Value),
    Pending,
    Error(SocketError),
}

enum WriteOutcome {
    Complete(Value),
    Pending,
    Error(SocketError),
}

fn try_read(state: &mut StreamState, op: &mut Op) -> ReadOutcome {
    let OpKind::Read { mode, max } = &op.kind else {
        return ReadOutcome::Error(SocketError::Other("读操作类型不符".to_owned()));
    };
    let (mode, max) = (*mode, *max);
    loop {
        if let Some(outcome) = satisfy_read(state, mode, max) {
            return outcome;
        }
        if state.eof {
            return eof_read(state, mode, max);
        }
        state.compact_read_buffer();
        let mut buffer = [0_u8; 8192];
        match state.socket.read(&mut buffer) {
            Ok(0) => {
                state.eof = true;
            },
            Ok(count) => {
                state.read_buffer.extend_from_slice(&buffer[..count]);
                op.on_progress();
            },
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return ReadOutcome::Pending;
            },
            Err(error) => return ReadOutcome::Error(from_io(&error)),
        }
    }
}

fn satisfy_read(state: &mut StreamState, mode: ReadMode, max: usize) -> Option<ReadOutcome> {
    match mode {
        ReadMode::Read => {
            if state.buffered() == 0 {
                None
            } else {
                let count = max.min(state.buffered());
                Some(ReadOutcome::Complete(option_some(bytes_value(take_bytes(
                    state, count,
                )))))
            }
        },
        ReadMode::Exact => {
            if state.buffered() >= max {
                Some(ReadOutcome::Complete(bytes_value(take_bytes(state, max))))
            } else {
                None
            }
        },
        ReadMode::All => {
            if state.buffered() > max {
                Some(ReadOutcome::Error(SocketError::TooLarge))
            } else {
                None
            }
        },
        ReadMode::Line => {
            if let Some(position) = state.buffer().iter().position(|byte| *byte == b'\n') {
                if position > max {
                    return Some(ReadOutcome::Error(SocketError::LineTooLong));
                }
                match line_value(take_bytes(state, position + 1)) {
                    Ok(value) => Some(ReadOutcome::Complete(value)),
                    Err(error) => Some(ReadOutcome::Error(error)),
                }
            } else if state.buffered() > max {
                Some(ReadOutcome::Error(SocketError::LineTooLong))
            } else {
                None
            }
        },
    }
}

fn eof_read(state: &mut StreamState, mode: ReadMode, max: usize) -> ReadOutcome {
    match mode {
        ReadMode::Read => ReadOutcome::Complete(option_none()),
        ReadMode::Exact => {
            if state.buffered() >= max {
                ReadOutcome::Complete(bytes_value(take_bytes(state, max)))
            } else {
                ReadOutcome::Error(SocketError::UnexpectedEof)
            }
        },
        ReadMode::All => {
            if state.buffered() > max {
                ReadOutcome::Error(SocketError::TooLarge)
            } else {
                ReadOutcome::Complete(bytes_value(take_remainder(state)))
            }
        },
        ReadMode::Line => {
            if state.buffered() == 0 {
                ReadOutcome::Complete(option_none())
            } else {
                let bytes = take_remainder(state);
                match line_value(bytes) {
                    Ok(value) => ReadOutcome::Complete(value),
                    Err(error) => ReadOutcome::Error(error),
                }
            }
        },
    }
}

fn take_bytes(state: &mut StreamState, count: usize) -> Vec<u8> {
    let bytes = state.buffer()[..count].to_vec();
    state.consume(count);
    bytes
}

/// 取走全部未消费缓冲并清空读缓冲.
fn take_remainder(state: &mut StreamState) -> Vec<u8> {
    let bytes = state.buffer().to_vec();
    state.read_buffer.clear();
    state.read_start = 0;
    bytes
}

fn bytes_value(bytes: Vec<u8>) -> Value {
    Value::List(Arc::new(List::u8s(bytes)))
}

fn line_value(mut bytes: Vec<u8>) -> Result<Value, SocketError> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    } else if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(option_some(Value::Str(Arc::from(text)))),
        Err(_) => Err(SocketError::InvalidUtf8),
    }
}

fn try_write(state: &mut StreamState, op: &mut Op) -> WriteOutcome {
    let OpKind::Write { bytes, offset, all } = &mut op.kind else {
        return WriteOutcome::Error(SocketError::Other("写操作类型不符".to_owned()));
    };
    loop {
        if *offset == bytes.len() {
            return if *all {
                WriteOutcome::Complete(Value::Nil)
            } else {
                WriteOutcome::Complete(Value::U64(*offset as u64))
            };
        }
        match state.socket.write(&bytes[*offset..]) {
            Ok(0) => return WriteOutcome::Error(SocketError::ConnectionClosed),
            Ok(count) => {
                *offset += count;
                if !op.timeout.is_zero() {
                    op.deadline = deadline_after(op.timeout);
                }
                if !*all {
                    return WriteOutcome::Complete(Value::U64(count as u64));
                }
            },
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                return WriteOutcome::Pending;
            },
            Err(error) => return WriteOutcome::Error(from_io(&error)),
        }
    }
}
