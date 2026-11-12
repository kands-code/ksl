//! 终端会话.
//!
//! 输入服务线程独占标准输入与终端状态.
//! 原始模式, bracketed paste 与 crossterm 事件源都只能从这条线程进入.
//! REPL 编辑器通过会话接收事件, 不直接调用 `event::read`.
//! 程序的 `ReadLine` 与 `ReadKey` 在输入线程内使用同一份原始模式守卫.
//!
//! 会话是串行的: 同一时刻只有一条会话或一个程序输入请求拥有终端.
//! 客户端丢弃会话时关闭事件接收端.
//! 服务线程在下一次轮询前退出并恢复终端状态.

use std::io::Write as _;
use std::sync::Arc;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, is_raw_mode_enabled};
use ksl_frontend::error::RuntimeError as Error;

use super::state::TerminalFlags;

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    flags: Arc<TerminalFlags>,
    restore_raw: bool,
    restore_paste: bool,
}

impl RawGuard {
    /// 进入原始模式; 需要时同时开启 bracketed paste.
    ///
    /// 终端可能已经由调用方置于原始模式, 此时离开不负责恢复.
    /// 存在显式持有者时, 临时守卫离开同样不恢复.
    pub(crate) fn start(
        flags: Arc<TerminalFlags>,
        bracketed_paste: bool,
    ) -> std::io::Result<Self> {
        let already_raw = is_raw_mode_enabled().unwrap_or(false);
        if !already_raw {
            enable_raw_mode()?;
        }
        flags.mark_raw();
        let mut guard = Self {
            flags,
            restore_raw: !already_raw,
            restore_paste: false,
        };
        if bracketed_paste {
            if let Err(error) =
                execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste)
            {
                if guard.restore_raw && disable_raw_mode().is_ok() {
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
            let _ = execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
        }
        // 临时守卫只在没有显式持有者时恢复; 持有期间由持有者负责退出.
        if self.restore_raw && !self.flags.held() && disable_raw_mode().is_ok() {
            self.flags.clear_raw();
        }
        let _ = std::io::stdout().flush();
    }
}

/// 服务线程处理一次转发会话, 直到客户端结束或事件通道断开.
pub(crate) fn serve(
    flags: Arc<TerminalFlags>,
    bracketed_paste: bool,
    events: &Sender<SessionEvent>,
    control: &Receiver<()>,
    ready: &Sender<Result<(usize, usize), Error>>,
    finished: &Sender<()>,
) {
    let guard = match RawGuard::start(flags, bracketed_paste) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = ready.send(Err(Error::internal(format!("开启终端会话失败: {error}"))));
            let _ = finished.send(());
            return;
        },
    };
    let size = crate::terminal::size_or(80, 24);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_state_stays_until_cleared() {
        let _guard = TEST_LOCK.lock().expect("终端状态测试锁");
        let flags = TerminalFlags::new();
        assert!(!flags.raw_active());
        flags.mark_raw();
        assert!(flags.raw_active());
        flags.mark_raw();
        assert!(flags.raw_active());
        flags.clear_raw();
        assert!(!flags.raw_active());
    }
}
