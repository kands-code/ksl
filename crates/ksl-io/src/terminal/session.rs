//! 终端会话.
//!
//! 输入服务线程独占标准输入与终端状态.
//! 原始模式, bracketed paste 与终端事件源都只能从这条线程进入.
//! REPL 编辑器通过会话接收事件, 不直接调用 `event::read`.
//! 程序的 `ReadLine` 与 `ReadKey` 在输入线程内使用同一份原始模式守卫.
//!
//! 会话是串行的: 同一时刻只有一条会话或一个程序输入请求拥有终端.
//! 客户端丢弃会话时关闭事件接收端, 并等待服务线程恢复终端状态.

use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use ksl_diagnostics::error::RuntimeError as Error;

use super::dimension;
use super::state::TerminalFlags;
use super::sys::{Event, TerminalHandle};

#[cfg(test)]
pub static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 会话事件; 读取失败以错误事件结束.
pub enum SessionEvent {
    Event(Event),
    Error(Error),
}

/// REPL 编辑器使用的终端会话.
pub struct Session {
    events: Option<Receiver<SessionEvent>>,
    control: Sender<()>,
    /// 终端恢复信号.
    restored: Receiver<()>,
    terminal: TerminalHandle,
    size: (usize, usize),
}

impl Session {
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
        // 等待终端状态恢复, 至多 1 秒.
        if self.restored.recv_timeout(Duration::from_secs(1)).is_err() {
            // 服务线程未按时恢复时直接恢复终端.
            let _ = self.terminal.disable_raw_mode();
        }
    }
}

impl Session {
    #[must_use]
    pub(crate) fn new(
        events: Receiver<SessionEvent>,
        control: Sender<()>,
        restored: Receiver<()>,
        terminal: TerminalHandle,
        size: (usize, usize),
    ) -> Self {
        Self {
            events: Some(events),
            control,
            restored,
            terminal,
            size,
        }
    }
}

/// 原始模式守卫; 只由输入线程创建.
pub(crate) struct RawGuard {
    terminal: TerminalHandle,
    flags: Arc<TerminalFlags>,
    restore_raw: bool,
    restore_paste: bool,
}

impl RawGuard {
    /// 进入原始模式; 需要时同时开启 bracketed paste.
    ///
    /// 终端可能已由调用方置于原始模式, 此时离开不负责恢复.
    /// 存在显式持有者时, 临时守卫离开同样不恢复.
    ///
    /// # Errors
    ///
    /// 终端进入原始模式或开启 bracketed paste 失败时透传 I/O 错误.
    pub(crate) fn start(
        terminal: TerminalHandle,
        flags: Arc<TerminalFlags>,
        bracketed_paste: bool,
    ) -> std::io::Result<Self> {
        let already_raw = terminal.is_raw_mode_enabled();
        if !already_raw {
            terminal.enable_raw_mode()?;
        }
        flags.mark_raw();
        let mut guard = Self {
            terminal,
            flags,
            restore_raw: !already_raw,
            restore_paste: false,
        };
        if bracketed_paste {
            if let Err(error) = guard.terminal.set_bracketed_paste(true) {
                if guard.restore_raw && guard.terminal.disable_raw_mode().is_ok() {
                    guard.flags.clear_raw();
                }
                return Err(error);
            }
            guard.restore_paste = true;
        }
        Ok(guard)
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        if self.restore_paste {
            let _ = self.terminal.set_bracketed_paste(false);
        }
        // 临时守卫只在没有显式持有者时恢复; 持有期间由持有者负责退出.
        if self.restore_raw && !self.flags.held() && self.terminal.disable_raw_mode().is_ok() {
            self.flags.clear_raw();
        }
        let _ = std::io::stdout().flush();
    }
}

/// 服务线程处理一次转发会话, 直到客户端结束或事件通道断开.
///
/// 返回时事件循环已停止; 终端恢复与完成信号由调用方处理.
pub(crate) fn serve(
    terminal: &TerminalHandle,
    flags: Arc<TerminalFlags>,
    bracketed_paste: bool,
    events: &Sender<SessionEvent>,
    control: &Receiver<()>,
    ready: &Sender<Result<(usize, usize), Error>>,
) {
    let guard = match RawGuard::start(terminal.clone(), flags, bracketed_paste) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = ready.send(Err(Error::internal(format!("开启终端会话失败: {error}"))));
            return;
        },
    };
    let size = terminal.size().map_or((80, 24), |(width, height)| {
        (dimension(width, 80), dimension(height, 24))
    });
    if ready.send(Ok(size)).is_err() {
        return;
    }
    loop {
        if control.try_recv().is_ok() {
            break;
        }
        match terminal.poll(Duration::from_millis(50)) {
            Ok(true) => match terminal.read() {
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
}
