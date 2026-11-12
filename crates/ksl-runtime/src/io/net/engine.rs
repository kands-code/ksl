//! TCP 网络引擎.
//!
//! 单个线程持有 mio 轮询器, 监听器与套接字状态.
//! 语言层的网络原语经命令队列提交, 完成结果经回调交回调度器.
//! 所有套接字注册读与写两种兴趣, 提交操作时先立即尝试, 再等待事件.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use arcstr::ArcStr;
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use ksl_frontend::error::RuntimeError as Error;
use mio::event::Event;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token, Waker};
use socket2::{Domain, Protocol, Socket, Type};

use super::error::{SocketError, from_io};
use super::handle::{SocketHandle, TcpListenerHandle};
use crate::io::Done;
use crate::timer_heap::TimerHeap;
use crate::value::{List, Value, option_none, option_some, result_err, result_ok};

/// 唤醒轮询器的保留标记; 连接编号从 1 开始.
const WAKER: Token = Token(usize::MAX);

/// 单轮轮询的事件容量.
const EVENTS_CAPACITY: usize = 1024;

/// 单次底层读取的块大小.
const READ_CHUNK: usize = 64 * 1024;

/// 读操作形态.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ReadMode {
    /// 最多读 `max` 字节.
    Read,
    /// 精确读 `max` 字节.
    Exact,
    /// 读到 EOF, 至多 `max` 字节.
    All,
    /// 读到换行, 行长至多 `max`.
    Line,
}

impl ReadMode {
    /// 是否按每次等待计时, 而不是从提交开始计总时限.
    fn idle_timeout(self) -> bool {
        matches!(self, Self::Exact | Self::All | Self::Line)
    }

    /// 本次底层读取的长度.
    ///
    /// `All` 与 `Line` 最多多读一个字节, 用于判定是否超出 `max`.
    fn read_len(self, max: usize, buffered: usize) -> usize {
        let len = match self {
            Self::Read => max,
            Self::Exact => max.saturating_sub(buffered),
            Self::All | Self::Line => max.saturating_sub(buffered).saturating_add(1),
        };
        len.min(READ_CHUNK)
    }
}

/// 一次网络操作; 由语言层构造.
pub(crate) enum Op {
    Listen {
        addr: SocketAddr,
    },
    Accept {
        listener: u64,
        timeout: Duration,
    },
    Connect {
        addr: SocketAddr,
        timeout: Duration,
    },
    Read {
        socket: u64,
        mode: ReadMode,
        max: usize,
        timeout: Duration,
    },
    Write {
        socket: u64,
        bytes: Vec<u8>,
        all: bool,
        timeout: Duration,
    },
    ShutdownWrite {
        socket: u64,
        timeout: Duration,
    },
}

/// 网络引擎的提交端; 句柄与运行时共享.
#[derive(Clone)]
pub(crate) struct EngineHandle {
    commands: Sender<Command>,
    waker: Arc<Waker>,
}

impl EngineHandle {
    /// 启动网络引擎线程; 每个运行时一个.
    pub(crate) fn start() -> io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), WAKER)?);
        let (commands, receiver) = unbounded();
        let handle = Self {
            commands,
            waker: Arc::clone(&waker),
        };
        let thread_handle = handle.clone();
        std::thread::Builder::new()
            .name("ksl-net".to_owned())
            .spawn(move || run(poll, receiver, thread_handle))?;
        Ok(handle)
    }

    /// 提交一个操作; 引擎已停止时以内部错误完成.
    pub(crate) fn submit(&self, op_id: u64, op: Op, cancel: Arc<AtomicBool>, done: Done) {
        self.send(Command::Op {
            op_id,
            op,
            cancel,
            done,
        });
    }

    /// 取消一个尚未完成的操作.
    pub(crate) fn cancel(&self, op_id: u64) {
        self.send(Command::Cancel { op_id });
    }

    /// 关闭套接字; 由语言层句柄调用.
    pub(super) fn close_socket(&self, id: u64) {
        self.send(Command::CloseSocket { id });
    }

    /// 关闭监听器; 由语言层句柄调用.
    pub(super) fn close_listener(&self, id: u64) {
        self.send(Command::CloseListener { id });
    }

    /// 运行时释放时停止引擎.
    pub(crate) fn shutdown(&self) {
        self.send(Command::Shutdown);
    }

    fn send(&self, command: Command) {
        match self.commands.send(command) {
            Ok(()) => {
                let _ = self.waker.wake();
            },
            Err(error) => {
                if let Command::Op { done, .. } = error.into_inner() {
                    done(Err(Error::internal("网络引擎已停止")));
                }
            },
        }
    }
}

enum Command {
    Op {
        op_id: u64,
        op: Op,
        cancel: Arc<AtomicBool>,
        done: Done,
    },
    Cancel {
        op_id: u64,
    },
    CloseSocket {
        id: u64,
    },
    CloseListener {
        id: u64,
    },
    Shutdown,
}

/// 操作在引擎中的位置; 取消与超时按编号定位.
#[derive(Clone, Copy)]
enum Location {
    Accept { listener: u64 },
    Connect { socket: u64 },
    Read { socket: u64 },
    Write { socket: u64 },
}

/// 操作自身的数据.
enum PendingOp {
    Accept,
    Connect,
    Read {
        mode: ReadMode,
        max: usize,
    },
    Write {
        bytes: Vec<u8>,
        all: bool,
        offset: usize,
    },
    ShutdownWrite,
}

/// 操作的超时形态.
#[derive(Clone, Copy)]
enum Timeout {
    /// 从提交开始的绝对时限.
    Total(Duration),
    /// 到达队首后, 每次等待重新计时.
    Idle(Duration),
}

impl Timeout {
    fn duration(self) -> Duration {
        match self {
            Self::Total(duration) | Self::Idle(duration) => duration,
        }
    }

    fn is_zero(self) -> bool {
        self.duration().is_zero()
    }

    fn is_idle(self) -> bool {
        matches!(self, Self::Idle(_))
    }
}

struct Pending {
    id: u64,
    operation: PendingOp,
    done: Done,
    timeout: Timeout,
    /// 总时限的截止时刻; 空闲超时在开始等待后写入.
    deadline: Option<Instant>,
    /// 是否已登记到定时堆.
    armed: bool,
}

impl Pending {
    fn new(id: u64, operation: PendingOp, done: Done, timeout: Timeout) -> Self {
        let deadline = match timeout {
            Timeout::Total(duration) => Some(deadline(duration)),
            Timeout::Idle(_) => None,
        };
        Self {
            id,
            operation,
            done,
            timeout,
            deadline,
            armed: false,
        }
    }

    fn zero_timeout(&self) -> bool {
        self.timeout.is_zero()
    }

    /// 总时限是否已经到期; 空闲超时与 0 超时不在这里判定.
    fn expired(&self) -> bool {
        !self.timeout.is_idle()
            && !self.timeout.is_zero()
            && self.deadline.is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// 登记定时器; 总时限使用提交时的截止时刻, 空闲超时从当前开始.
    fn ensure_timer(&mut self, timers: &mut TimerHeap<u64, ()>) {
        if self.armed || self.timeout.is_zero() {
            return;
        }
        let deadline = match self.timeout {
            Timeout::Total(_) => self.deadline.expect("总时限缺少截止时刻"),
            Timeout::Idle(duration) => *self.deadline.get_or_insert_with(|| deadline(duration)),
        };
        timers.insert(deadline, self.id, ());
        self.armed = true;
    }

    /// 空闲超时在有进展后重新计时; 总时限保持不变.
    fn restart_idle(&mut self, timers: &mut TimerHeap<u64, ()>) {
        let Timeout::Idle(duration) = self.timeout else {
            return;
        };
        if duration.is_zero() {
            return;
        }
        let deadline = deadline(duration);
        timers.insert(deadline, self.id, ());
        self.deadline = Some(deadline);
        self.armed = true;
    }
}

struct ListenerEntry {
    listener: TcpListener,
    accepts: VecDeque<Pending>,
    closed: Arc<AtomicBool>,
}

struct SocketEntry {
    stream: TcpStream,
    read_buffer: ReadBuffer,
    read_queue: VecDeque<Pending>,
    write_queue: VecDeque<Pending>,
    connect: Option<Pending>,
    closed: Arc<AtomicBool>,
    peer: String,
}

struct Engine {
    poll: Poll,
    commands: Receiver<Command>,
    handle: EngineHandle,
    listeners: HashMap<u64, ListenerEntry>,
    sockets: HashMap<u64, SocketEntry>,
    locations: HashMap<u64, Location>,
    timers: TimerHeap<u64, ()>,
    events: Events,
    read_chunk: Vec<u8>,
    next_id: u64,
}

fn run(poll: Poll, commands: Receiver<Command>, handle: EngineHandle) {
    let mut engine = Engine::new(poll, commands, handle);
    let outcome = catch_unwind(AssertUnwindSafe(|| engine.run()));
    if outcome.is_err() {
        engine.fail_all();
    }
}

impl Engine {
    fn new(poll: Poll, commands: Receiver<Command>, handle: EngineHandle) -> Self {
        Self {
            poll,
            commands,
            handle,
            listeners: HashMap::new(),
            sockets: HashMap::new(),
            locations: HashMap::new(),
            timers: TimerHeap::new(),
            events: Events::with_capacity(EVENTS_CAPACITY),
            read_chunk: vec![0; READ_CHUNK],
            next_id: 1,
        }
    }

    fn run(&mut self) {
        loop {
            if !self.drain_commands() {
                break;
            }
            let timeout = self
                .timers
                .next_deadline()
                .map(|deadline| deadline.saturating_duration_since(Instant::now()));
            match self.poll.poll(&mut self.events, timeout) {
                Ok(()) => {},
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
            let events = std::mem::replace(&mut self.events, Events::with_capacity(0));
            for event in &events {
                self.handle_event(event);
            }
            self.events = events;
            // 就绪事件优先; 到期定时器只处理事件未完成的操作.
            self.fire_timers();
        }
        self.fail_all();
    }

    fn drain_commands(&mut self) -> bool {
        loop {
            match self.commands.try_recv() {
                Ok(Command::Shutdown) | Err(TryRecvError::Disconnected) => return false,
                Ok(command) => self.handle_command(command),
                Err(TryRecvError::Empty) => return true,
            }
        }
    }

    fn handle_command(&mut self, command: Command) {
        match command {
            Command::Op {
                op_id,
                op,
                cancel,
                done,
            } => {
                if !cancel.load(Ordering::Relaxed) {
                    self.submit_op(op_id, op, done);
                }
            },
            Command::Cancel { op_id } => self.cancel_op(op_id),
            Command::CloseSocket { id } => self.close_socket(id),
            Command::CloseListener { id } => self.close_listener(id),
            Command::Shutdown => {},
        }
    }

    fn submit_op(&mut self, op_id: u64, op: Op, done: Done) {
        match op {
            Op::Listen { addr } => self.listen(addr, done),
            Op::Accept { listener, timeout } => {
                self.start_accept(op_id, listener, timeout, done);
            },
            Op::Connect { addr, timeout } => self.start_connect(op_id, addr, timeout, done),
            Op::Read {
                socket,
                mode,
                max,
                timeout,
            } => self.start_read(op_id, socket, mode, max, timeout, done),
            Op::Write {
                socket,
                bytes,
                all,
                timeout,
            } => self.start_write(op_id, socket, bytes, all, timeout, done),
            Op::ShutdownWrite { socket, timeout } => {
                self.start_shutdown_write(op_id, socket, timeout, done);
            },
        }
    }

    fn listen(&mut self, addr: SocketAddr, done: Done) {
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
        done(Ok(result_ok(Value::TcpListener(Arc::new(handle)))));
    }

    fn start_accept(&mut self, op_id: u64, listener: u64, timeout: Duration, done: Done) {
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

    fn service_accept(&mut self, listener: u64) {
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
            Ok(handle) => self.finish(pending, result_ok(Value::Socket(handle))),
            Err(error) => self.finish(pending, result_err(error.to_value())),
        }
    }

    fn start_connect(&mut self, op_id: u64, addr: SocketAddr, timeout: Duration, done: Done) {
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

    fn service_connect(&mut self, socket: u64) {
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
                self.finish(pending, result_ok(Value::Socket(handle)));
            },
            Some((pending, Err(error))) => {
                self.finish(pending, error_value(&error));
                self.close_socket(socket);
            },
            None => {},
        }
    }

    fn insert_socket(
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

    fn start_read(
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

    fn service_read(&mut self, socket: u64) {
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

    fn start_write(
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

    fn start_shutdown_write(&mut self, op_id: u64, socket: u64, timeout: Duration, done: Done) {
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

    fn service_write(&mut self, socket: u64) {
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

    fn handle_event(&mut self, event: &Event) {
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

    fn cancel_op(&mut self, op_id: u64) {
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

    fn timeout(&mut self, op_id: u64) {
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

    fn close_socket(&mut self, id: u64) {
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

    fn close_listener(&mut self, id: u64) {
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

    fn fire_timers(&mut self) {
        let due = self.timers.take_due(Instant::now());
        for (op_id, ()) in due {
            self.timeout(op_id);
        }
    }

    fn finish(&mut self, pending: Pending, value: Value) {
        self.locations.remove(&pending.id);
        self.timers.remove(&pending.id);
        (pending.done)(Ok(value));
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn fail_all(&mut self) {
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

/// 连接编号转为轮询标记; 编号由引擎单调分配, 不会超出 usize.
fn token(id: u64) -> usize {
    usize::try_from(id).expect("网络连接编号超出平台范围")
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

/// 信号中断; 同一操作可以重试.
fn interrupted(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Interrupted
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

/// 超时截止时刻; 超大超时按一年处理, 避免平台时刻溢出.
fn deadline(timeout: Duration) -> Instant {
    Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(|| Instant::now() + Duration::from_hours(8760))
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

fn closed_value() -> Value {
    result_err(SocketError::Closed.to_value())
}

fn timeout_value() -> Value {
    result_err(SocketError::TimedOut.to_value())
}

fn error_value(error: &io::Error) -> Value {
    result_err(from_io(error).to_value())
}

/// 共享读缓冲.
#[derive(Default)]
struct ReadBuffer {
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

#[cfg(test)]
mod tests {
    use super::{READ_CHUNK, ReadMode};

    #[test]
    fn read_lengths_follow_mode_limits() {
        assert_eq!(ReadMode::Read.read_len(10, 0), 10);
        assert_eq!(ReadMode::Read.read_len(READ_CHUNK * 2, 0), READ_CHUNK);
        assert_eq!(ReadMode::Exact.read_len(100, 40), 60);
        assert_eq!(ReadMode::Exact.read_len(READ_CHUNK * 2, 0), READ_CHUNK);
        assert_eq!(ReadMode::All.read_len(10, 10), 1);
        assert_eq!(ReadMode::Line.read_len(10, 0), 11);
        assert_eq!(ReadMode::Line.read_len(10, 4), 7);
        assert_eq!(ReadMode::All.read_len(READ_CHUNK * 2, 0), READ_CHUNK);
    }
}
