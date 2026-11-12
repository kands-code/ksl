//! 标准输出与标准错误的写出状态.
//!
//! 每个运行时持有独立的跟踪状态; 实际写出仍由进程级标准流串行化.
//! 跟踪用于宿主在输入边界决定空行与补行, 不进入语言语义.

use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};

/// 标准流写出与行首跟踪状态.
#[derive(Default)]
pub(super) struct Stdio {
    /// 是否正在跟踪标准输出的行首状态.
    stdout_tracking: AtomicBool,
    /// 标准输出是否停在行首; 仅在跟踪开启时生效.
    stdout_at_line_start: AtomicBool,
    /// 跟踪期间标准输出是否写过非空文本.
    stdout_wrote: AtomicBool,
    /// 是否正在跟踪标准错误的写出.
    stderr_tracking: AtomicBool,
    /// 跟踪期间标准错误是否写过非空文本.
    stderr_wrote: AtomicBool,
}

impl Stdio {
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            stdout_tracking: AtomicBool::new(false),
            // 未开始跟踪时视为停在行首, 与 `take_stdout_line_start` 的默认一致.
            stdout_at_line_start: AtomicBool::new(true),
            stdout_wrote: AtomicBool::new(false),
            stderr_tracking: AtomicBool::new(false),
            stderr_wrote: AtomicBool::new(false),
        }
    }

    /// 写出标准输出; 写出后立即冲刷, 终端上不滞留.
    pub(super) fn write_stdout(&self, text: &str) -> std::io::Result<()> {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(text.as_bytes())?;
        stdout.flush()?;
        // 空文本不改变行首状态, 也不算写过输出.
        if !text.is_empty() && self.stdout_tracking.load(Ordering::Relaxed) {
            self.stdout_at_line_start
                .store(text.ends_with('\n'), Ordering::Relaxed);
            self.stdout_wrote.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    pub(super) fn write_stderr(&self, text: &str) -> std::io::Result<()> {
        let mut stderr = std::io::stderr().lock();
        stderr.write_all(text.as_bytes())?;
        stderr.flush()?;
        if !text.is_empty() && self.stderr_tracking.load(Ordering::Relaxed) {
            self.stderr_wrote.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    /// 开启标准输出的行首与写出跟踪.
    pub(super) fn begin_stdout_tracking(&self) {
        self.stdout_at_line_start.store(true, Ordering::Relaxed);
        self.stdout_wrote.store(false, Ordering::Relaxed);
        self.stdout_tracking.store(true, Ordering::Relaxed);
    }

    /// 结束行首跟踪, 返回输出当前是否停在行首.
    #[must_use]
    pub(super) fn take_stdout_line_start(&self) -> bool {
        self.stdout_tracking.store(false, Ordering::Relaxed);
        self.stdout_at_line_start.load(Ordering::Relaxed)
    }

    /// 取本次跟踪期间标准输出是否写过非空文本.
    #[must_use]
    pub(super) fn take_stdout_wrote(&self) -> bool {
        let wrote = self.stdout_wrote.load(Ordering::Relaxed);
        self.stdout_wrote.store(false, Ordering::Relaxed);
        wrote
    }

    /// 开启标准错误的写出跟踪.
    pub(super) fn begin_stderr_tracking(&self) {
        self.stderr_wrote.store(false, Ordering::Relaxed);
        self.stderr_tracking.store(true, Ordering::Relaxed);
    }

    /// 结束标准错误跟踪, 返回本次是否写过非空文本.
    #[must_use]
    pub(super) fn take_stderr_wrote(&self) -> bool {
        self.stderr_tracking.store(false, Ordering::Relaxed);
        let wrote = self.stderr_wrote.load(Ordering::Relaxed);
        self.stderr_wrote.store(false, Ordering::Relaxed);
        wrote
    }

    /// 计入终端行规程对输入的回显.
    pub(super) fn track_terminal_echo(&self, newline: bool) {
        if self.stdout_tracking.load(Ordering::Relaxed) {
            self.stdout_wrote.store(true, Ordering::Relaxed);
            self.stdout_at_line_start.store(newline, Ordering::Relaxed);
        }
    }
}
