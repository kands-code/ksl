//! 终端后端.
//!
//! `IoRuntime` 的终端共享状态持有一个句柄; 输入线程是主要使用者.
//! 原始模式, 光标尺寸与 bracketed paste 经该句柄统一切换.
//! 事件读取使用可克隆的读取器, 不持有终端锁.

use std::io::{self, Write as _};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

pub use termina::WindowSize;
pub use termina::event::{Event, KeyCode, KeyEvent, KeyEventKind, Modifiers as KeyModifiers};
use termina::{EventReader, PlatformTerminal, Terminal as _};

struct Backend {
    terminal: PlatformTerminal,
    reader: EventReader,
    /// 当前是否由本进程置于原始模式.
    raw: bool,
    /// bracketed paste 是否已开启.
    paste: bool,
}

/// 一次运行时的终端句柄; 克隆共享同一后端.
#[derive(Clone)]
pub(crate) struct TerminalHandle {
    inner: Arc<Mutex<Option<Backend>>>,
}

impl TerminalHandle {
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Backend>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn with_backend<T>(
        &self,
        action: impl FnOnce(&mut Backend) -> io::Result<T>,
    ) -> io::Result<T> {
        let mut guard = self.lock();
        if guard.is_none() {
            let terminal = PlatformTerminal::new()?;
            let reader = terminal.event_reader();
            *guard = Some(Backend {
                terminal,
                reader,
                raw: false,
                paste: false,
            });
        }
        let backend = guard.as_mut().expect("后端已初始化");
        action(backend)
    }

    /// 终端尺寸.
    pub(crate) fn size(&self) -> io::Result<(u16, u16)> {
        self.with_backend(|backend| {
            let size = backend.terminal.get_dimensions()?;
            Ok((size.cols, size.rows))
        })
    }

    /// 当前是否处于原始模式; 未初始化时返回 false.
    pub(crate) fn is_raw_mode_enabled(&self) -> bool {
        self.lock().as_ref().is_some_and(|backend| backend.raw)
    }

    /// 进入原始模式.
    pub(crate) fn enable_raw_mode(&self) -> io::Result<()> {
        self.with_backend(|backend| {
            if backend.raw {
                return Ok(());
            }
            backend.terminal.enter_raw_mode()?;
            backend.raw = true;
            Ok(())
        })
    }

    /// 恢复终端模式; 未初始化或未进入原始模式时为空操作.
    pub(crate) fn disable_raw_mode(&self) -> io::Result<()> {
        let mut guard = self.lock();
        let Some(backend) = guard.as_mut() else {
            return Ok(());
        };
        if !backend.raw {
            return Ok(());
        }
        backend.terminal.enter_cooked_mode()?;
        backend.raw = false;
        Ok(())
    }

    /// 开启或关闭 bracketed paste.
    pub(crate) fn set_bracketed_paste(&self, enable: bool) -> io::Result<()> {
        self.with_backend(|backend| {
            if backend.paste == enable {
                return Ok(());
            }
            let sequence = if enable {
                b"\x1b[?2004h".as_slice()
            } else {
                b"\x1b[?2004l".as_slice()
            };
            backend.terminal.write_all(sequence)?;
            backend.terminal.flush()?;
            backend.paste = enable;
            Ok(())
        })
    }

    fn reader(&self) -> io::Result<EventReader> {
        self.with_backend(|backend| Ok(backend.reader.clone()))
    }

    /// 等待事件; 超时返回 false.
    pub(crate) fn poll(&self, timeout: Duration) -> io::Result<bool> {
        self.reader()?.poll(Some(timeout), |_| true)
    }

    /// 读取一个事件.
    pub(crate) fn read(&self) -> io::Result<Event> {
        self.reader()?.read(|_| true)
    }
}
