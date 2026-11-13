//! 终端会话.
//!
//! 输入服务线程独占标准输入与终端状态: raw mode, bracketed paste 与
//! crossterm 事件源都只能从这条线程进入. REPL 编辑器通过会话接收事件,
//! 不直接调用 `event::read`; 程序的 `ReadLine` 与 `ReadKey` 在输入线程内
//! 使用同一份原始模式守卫.
//!
//! 会话是串行的: 同一时刻只有一条会话或一个程序输入请求拥有终端.
//! 客户端丢弃会话时关闭事件接收端, 服务线程在下一次轮询前退出并恢复
//! 终端状态.

use std::io::Write as _;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use crossterm::event::{self, Event};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, is_raw_mode_enabled};
use crossterm::{execute, terminal};

use crate::error::RuntimeError as Error;

/// 会话事件; 读取失败以错误事件结束, 客户端不用猜测通道断开的原因.
pub(crate) enum SessionEvent {
    Event(Event),
    Error(Error),
}

/// REPL 编辑器使用的终端会话.
pub struct Session {
    events: Option<Receiver<SessionEvent>>,
    control: Sender<()>,
    finished: Receiver<()>,
    size: (usize, usize),
}

impl Session {
    /// 开启会话; 返回时 raw mode 与事件源已就绪.
    ///
    /// # Errors
    ///
    /// 终端不可用或无法进入原始模式.
    pub fn open(bracketed_paste: bool) -> Result<Self, Error> {
        crate::io::open_terminal_session(bracketed_paste)
    }

    /// 阻塞等待下一个终端事件.
    ///
    /// # Errors
    ///
    /// 终端读取失败或服务线程已退出.
    pub fn recv(&self) -> Result<Event, Error> {
        let events = self
            .events
            .as_ref()
            .ok_or_else(|| Error::internal("终端会话已关闭"))?;
        match events.recv() {
            Ok(SessionEvent::Event(event)) => Ok(event),
            Ok(SessionEvent::Error(error)) => Err(error),
            Err(_) => Err(Error::internal("终端会话已结束")),
        }
    }

    /// 会话开启时的终端尺寸.
    #[must_use]
    pub fn size(&self) -> (usize, usize) {
        self.size
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // 先关闭事件接收端, 让可能阻塞在发送上的服务线程退出.
        self.events = None;
        let _ = self.control.send(());
        // 等待终端状态恢复; 服务线程异常退出时不能无限等待.
        let _ = self.finished.recv_timeout(Duration::from_secs(1));
    }
}

impl Session {
    pub(crate) fn new(
        events: Receiver<SessionEvent>,
        control: Sender<()>,
        finished: Receiver<()>,
        size: (usize, usize),
    ) -> Self {
        Self {
            events: Some(events),
            control,
            finished,
            size,
        }
    }
}

/// 原始模式守卫; 只由输入线程创建.
pub(crate) struct RawGuard {
    restore_raw: bool,
    restore_paste: bool,
}

impl RawGuard {
    /// 进入原始模式; 需要时同时开启 bracketed paste.
    ///
    /// 终端可能已经由调用方置于原始模式, 此时离开不负责恢复.
    pub(crate) fn start(bracketed_paste: bool) -> std::io::Result<Self> {
        let already_raw = is_raw_mode_enabled().unwrap_or(false);
        if !already_raw {
            enable_raw_mode()?;
        }
        let mut guard = Self {
            restore_raw: !already_raw,
            restore_paste: false,
        };
        if bracketed_paste {
            execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)?;
            guard.restore_paste = true;
        }
        Ok(guard)
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        if self.restore_paste {
            let _ = execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
        }
        if self.restore_raw {
            let _ = disable_raw_mode();
        }
        let _ = std::io::stdout().flush();
    }
}

/// 服务线程处理一次转发会话, 直到客户端结束或事件通道断开.
pub(crate) fn serve(
    bracketed_paste: bool,
    events: &Sender<SessionEvent>,
    control: &Receiver<()>,
    ready: &Sender<Result<(usize, usize), Error>>,
    finished: &Sender<()>,
) {
    let guard = match RawGuard::start(bracketed_paste) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = ready.send(Err(Error::internal(format!("开启终端会话失败: {error}"))));
            let _ = finished.send(());
            return;
        },
    };
    let size = terminal::size().map_or((80, 24), |(width, height)| {
        let width = if width == 0 { 80 } else { usize::from(width) };
        let height = if height == 0 { 24 } else { usize::from(height) };
        (width, height)
    });
    if ready.send(Ok(size)).is_err() {
        return;
    }
    loop {
        if control.try_recv().is_ok() {
            break;
        }
        match event::poll(Duration::from_millis(50)) {
            Ok(true) => match event::read() {
                Ok(event) => {
                    if events.send(SessionEvent::Event(event)).is_err() {
                        break;
                    }
                },
                Err(error) => {
                    let _ = events.send(SessionEvent::Error(Error::internal(format!(
                        "读取终端事件失败: {error}"
                    ))));
                    break;
                },
            },
            Ok(false) => {},
            Err(error) => {
                let _ = events.send(SessionEvent::Error(Error::internal(format!(
                    "轮询终端事件失败: {error}"
                ))));
                break;
            },
        }
    }
    drop(guard);
    let _ = finished.send(());
}
