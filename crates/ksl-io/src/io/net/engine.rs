//! TCP 网络引擎.
//!
//! 单个线程持有 mio 轮询器, 监听器与套接字状态.
//! 语言层的网络原语经命令队列提交, 完成结果经回调交回调度器.
//! 所有套接字注册读与写两种兴趣, 提交操作时先立即尝试, 再等待事件.

mod accept;
mod connect;
mod lifecycle;
mod read;
mod support;
mod write;

use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use ksl_diagnostics::error::RuntimeError as Error;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Poll, Waker};

use self::read::ReadBuffer;
use self::support::{READ_CHUNK, WAKER, deadline};
use crate::io::Done;
use crate::timer_heap::TimerHeap;

/// 单轮轮询的事件容量.
const EVENTS_CAPACITY: usize = 1024;

/// 读操作形态.
#[derive(Debug, Clone, Copy)]
pub enum ReadMode {
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
    /// 是否按每次等待计时; false 表示从提交开始计总时限.
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
pub enum Op {
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
pub struct EngineHandle {
    commands: Sender<Command>,
    waker: Arc<Waker>,
}

impl EngineHandle {
    /// 启动网络引擎线程; 每个运行时一个.
    ///
    /// # Errors
    ///
    /// 创建事件轮询或注册唤醒器失败时透传 I/O 错误.
    pub fn start() -> io::Result<Self> {
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
    pub fn submit(&self, op_id: u64, op: Op, cancel: Arc<AtomicBool>, done: Done) {
        self.send(Command::Op {
            op_id,
            op,
            cancel,
            done,
        });
    }

    /// 取消一个尚未完成的操作.
    pub fn cancel(&self, op_id: u64) {
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
    pub fn shutdown(&self) {
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

    /// 总时限是否已到期; 空闲超时与 0 超时不在这里判定.
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
}

#[cfg(test)]
mod tests;
