//! 逐行与逐键输入.
//!
//! 终端模式使用共享行编辑器, 管道模式由预读线程供给.
//! 读取期间以短轮询检查取消标记与控制请求; 未交付的输入退回缓冲.

use std::collections::VecDeque;
use std::io::IsTerminal as _;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use arcstr::ArcStr;
use crossbeam_channel::Receiver;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::{Value, option_none, option_some, result_err, result_ok};

use super::super::display::{self, TerminalShared};
use super::super::service::OutputService;
use super::super::{input_error, key_value, output_error, strip_line_ending};
use super::pipe::{self, PipeError, PipeShared};
use super::{Request, acquire, canceled, cancelled, release};
use crate::terminal::state::TerminalState;
use crate::terminal::{Event, KeyEventKind, LineAction, session};

/// 输入线程跨请求保留的未消费输入.
///
/// 请求被取消时, 已读入但尚未交付的内容不能丢弃.
/// 管道模式的行退回共享预读缓冲, 终端模式已输入的行文本退回编辑器初值.
/// 标准输入是终端而标准输出不是终端时没有预读线程, 行暂存在 `lines` 中.
#[derive(Default)]
pub(super) struct InputBuffer {
    lines: VecDeque<String>,
    text: Option<String>,
}

impl InputBuffer {
    /// 取走终端模式中已消费但未交付的一行.
    pub(super) fn take_line(&mut self) -> Option<String> {
        self.lines.pop_front()
    }

    /// 取走终端模式中已编辑但未交付的文本.
    pub(super) fn take_text(&mut self) -> Option<String> {
        self.text.take()
    }
}

/// 读取期间处理控制请求.
///
/// `drain` 返回真表示当前读取应停止, 下一轮由外层先处理 `controls`.
pub(super) struct ReadContext<'a> {
    requests: &'a Receiver<Request>,
    pending: &'a mut VecDeque<Request>,
    controls: &'a mut VecDeque<Request>,
    terminal: &'a mut TerminalState,
    shared: &'a TerminalShared,
}

impl<'a> ReadContext<'a> {
    pub(super) fn new(
        requests: &'a Receiver<Request>,
        pending: &'a mut VecDeque<Request>,
        controls: &'a mut VecDeque<Request>,
        terminal: &'a mut TerminalState,
        shared: &'a TerminalShared,
    ) -> Self {
        Self {
            requests,
            pending,
            controls,
            terminal,
            shared,
        }
    }

    fn drain(&mut self) -> bool {
        while let Ok(request) = self.requests.try_recv() {
            match request {
                Request::RawMode {
                    enable,
                    holder,
                    cancel,
                    done,
                } => {
                    if canceled(&cancel) {
                        done(Err(cancelled()));
                    } else if enable {
                        done(Ok(acquire(self.terminal, holder, self.shared)));
                    } else if self.terminal.holder() == Some(holder) {
                        self.controls.push_front(Request::RawMode {
                            enable,
                            holder,
                            cancel,
                            done,
                        });
                        return true;
                    } else {
                        done(Ok(release(self.terminal, holder, self.shared)));
                    }
                },
                Request::ReleaseHolder { holder } => {
                    if self.terminal.holder() == Some(holder) {
                        self.controls.push_front(Request::ReleaseHolder { holder });
                        return true;
                    }
                },
                Request::ForceReset { done } => {
                    self.controls.push_front(Request::ForceReset { done });
                    return true;
                },
                request => self.pending.push_back(request),
            }
        }
        false
    }
}

/// 读一行标准输入.
///
/// 标准输入与标准输出都是终端时使用行编辑, 否则按行读取.
/// 提示先写出并冲刷.
pub(super) fn read_line(
    prompt: &str,
    cancel: &AtomicBool,
    buffer: &mut InputBuffer,
    ctx: &mut ReadContext<'_>,
    shared: &TerminalShared,
    output: &OutputService,
    pipe: &PipeShared,
) -> Result<Value, Error> {
    if canceled(cancel) {
        return Err(cancelled());
    }
    // 先冲刷排队输出, 再显示提示.
    output.flush()?;
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        read_line_terminal(prompt, cancel, buffer, ctx, shared)
    } else {
        read_line_plain(prompt, cancel, buffer, shared, pipe)
    }
}

/// 按行输入: 写出提示后按行读取, 不做行编辑.
fn read_line_plain(
    prompt: &str,
    cancel: &AtomicBool,
    buffer: &mut InputBuffer,
    shared: &TerminalShared,
    pipe: &PipeShared,
) -> Result<Value, Error> {
    if shared.flags().raw_active() {
        return Ok(result_err(input_error("RawModeActive", "")));
    }
    shared
        .stdio()
        .write_stdout(prompt)
        .map_err(|error| output_error(&error, "写出提示"))?;
    // 标准输入不是终端时由预读线程统一消费, 缓冲可跨请求保留.
    if !std::io::stdin().is_terminal() {
        return match pipe::take_line(Some(cancel), pipe) {
            Ok(Some(line)) => Ok(result_ok(option_some(Value::Str(ArcStr::from(line))))),
            Ok(None) => Ok(result_ok(option_none())),
            Err(PipeError::Cancelled) => Err(cancelled()),
            Err(PipeError::Io(error)) => Ok(result_err(input_error("Other", &error))),
        };
    }
    if let Some(line) = buffer.lines.pop_front() {
        if canceled(cancel) {
            buffer.lines.push_front(line);
            return Err(cancelled());
        }
        return Ok(result_ok(option_some(Value::Str(ArcStr::from(line)))));
    }
    if canceled(cancel) {
        return Err(cancelled());
    }
    match read_stdin_line(shared) {
        Ok(None) => Ok(result_ok(option_none())),
        Ok(Some(line)) => {
            // 读到行后才发现取消: 行已从标准输入消费, 退回缓冲供下一请求.
            if canceled(cancel) {
                buffer.lines.push_back(line);
                return Err(cancelled());
            }
            Ok(result_ok(option_some(Value::Str(ArcStr::from(line)))))
        },
        Err(error) => Ok(result_err(input_error("Other", &error.to_string()))),
    }
}

/// 从标准输入读一行并去掉行尾; EOF 返回 None.
fn read_stdin_line(shared: &TerminalShared) -> std::io::Result<Option<String>> {
    let mut raw = String::new();
    match std::io::stdin().read_line(&mut raw) {
        Ok(0) => Ok(None),
        Ok(_) => {
            track_stdin_echo(shared, &raw);
            Ok(Some(strip_line_ending(raw)))
        },
        Err(error) => Err(error),
    }
}

/// 把刚读到的输入回显计入标准输出跟踪.
///
/// 回显由终端行规程写到终端设备, 只有标准输出也是终端时才与程序输出在同一位置.
/// 未以换行结束时, 光标仍停在输入末尾.
fn track_stdin_echo(shared: &TerminalShared, raw: &str) {
    if std::io::stdout().is_terminal() {
        shared.stdio().track_terminal_echo(raw.ends_with('\n'));
    }
}

/// 终端输入: 原始模式下用共享行编辑器读取一行.
fn read_line_terminal(
    prompt: &str,
    cancel: &AtomicBool,
    buffer: &mut InputBuffer,
    ctx: &mut ReadContext<'_>,
    shared: &TerminalShared,
) -> Result<Value, Error> {
    // 终端可能已由宿主置于原始模式, 守卫只恢复自己开启的部分.
    let _raw = match start_raw(shared) {
        Ok(guard) => guard,
        Err(value) => return Ok(value),
    };
    display::begin(
        shared,
        prompt.to_owned(),
        buffer.text.take(),
        terminal_width(shared),
    )?;
    loop {
        if ctx.drain() {
            // 持有者释放或强制重置: 保留已输入文本, 让外层先处理控制请求.
            if let Some(text) = display::finish(shared)? {
                buffer.text = Some(text);
            }
            return Ok(result_err(input_error("RawModeReleased", "")));
        }
        if canceled(cancel) {
            // 编辑中的文本属于未交付输入, 退回缓冲供下一请求继续.
            if let Some(text) = display::finish(shared)? {
                buffer.text = Some(text);
            }
            return Err(cancelled());
        }
        match shared.terminal().poll(Duration::from_millis(50)) {
            Ok(true) => match shared.terminal().read() {
                Ok(Event::Key(key)) if !matches!(key.kind, KeyEventKind::Release) => {
                    let action = match display::handle_key(shared, key) {
                        Ok(Some(action)) => action,
                        Ok(None) => {
                            display::abandon(shared);
                            return Err(Error::internal("终端编辑状态缺失"));
                        },
                        Err(error) => {
                            display::abandon(shared);
                            return Err(error);
                        },
                    };
                    match action {
                        LineAction::Continue => {},
                        LineAction::Accept => {
                            let text = display::finish(shared)?.unwrap_or_default();
                            return Ok(result_ok(option_some(Value::Str(ArcStr::from(text)))));
                        },
                        LineAction::Interrupt => {
                            let _ = display::interrupt(shared)?;
                            return Ok(result_err(input_error("Interrupted", "")));
                        },
                        LineAction::Eof => {
                            let _ = display::finish(shared)?;
                            return Ok(result_ok(option_none()));
                        },
                        LineAction::Clear => {
                            if let Err(error) = display::clear_screen(shared) {
                                display::abandon(shared);
                                return Err(error);
                            }
                        },
                    }
                },
                Ok(Event::WindowResized(size)) => {
                    let width = crate::terminal::dimension(size.cols, 80);
                    if let Err(error) = display::resize(shared, width) {
                        display::abandon(shared);
                        return Err(error);
                    }
                },
                Ok(_) => {},
                Err(error) => {
                    display::abandon(shared);
                    return Ok(result_err(input_error(
                        "Other",
                        &format!("读取输入失败: {error}"),
                    )));
                },
            },
            Ok(false) => {},
            Err(error) => {
                display::abandon(shared);
                return Ok(result_err(input_error(
                    "Other",
                    &format!("轮询输入失败: {error}"),
                )));
            },
        }
    }
}

/// 开启临时原始模式; 失败时给出语言层错误值.
fn start_raw(shared: &TerminalShared) -> Result<session::RawGuard, Value> {
    session::RawGuard::start(shared.terminal().clone(), shared.flags_handle(), false).map_err(
        |error| result_err(input_error("Other", &format!("开启原始模式失败: {error}"))),
    )
}

fn terminal_width(shared: &TerminalShared) -> usize {
    shared
        .terminal()
        .size()
        .map_or(80, |(width, _)| crate::terminal::dimension(width, 80))
}

/// 读键期间在必要时临时开启终端原始模式, 读完立即恢复.
/// 原始模式已有持有者时借用当前模式, 读取结束不恢复.
/// 只返回建模过的按键: 松开事件, 修饰键自身与媒体键一类事件被忽略.
pub(super) fn read_key(
    cancel: &AtomicBool,
    ctx: &mut ReadContext<'_>,
    shared: &TerminalShared,
    output: &OutputService,
) -> Result<Value, Error> {
    // 与 ReadLine 相同: 应答输入请求前先冲刷已有输出.
    output.flush()?;
    if !std::io::stdin().is_terminal() {
        return Ok(result_err(input_error("NotATerminal", "")));
    }
    // 终端可能已由宿主置于原始模式, 守卫只恢复自己开启的部分.
    let _raw = match start_raw(shared) {
        Ok(guard) => guard,
        Err(value) => return Ok(value),
    };
    let value = loop {
        if ctx.drain() {
            return Ok(result_err(input_error("RawModeReleased", "")));
        }
        if canceled(cancel) {
            return Err(cancelled());
        }
        match shared.terminal().poll(Duration::from_millis(50)) {
            Ok(true) => match shared.terminal().read() {
                Ok(Event::Key(key))
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    if let Some(value) = key_value(key) {
                        break value;
                    }
                },
                Ok(_) => {},
                Err(error) => {
                    return Ok(result_err(input_error(
                        "Other",
                        &format!("读取按键失败: {error}"),
                    )));
                },
            },
            Ok(false) => {},
            Err(error) => {
                return Ok(result_err(input_error(
                    "Other",
                    &format!("轮询按键失败: {error}"),
                )));
            },
        }
    };
    Ok(result_ok(value))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use ksl_value::value::Value;

    use super::super::pipe;
    use super::{InputBuffer, read_line_plain};
    use crate::io::display::TerminalShared;
    use crate::terminal::session;

    #[test]
    fn read_line_plain_rejects_raw_mode() {
        let _guard = session::TEST_LOCK.lock().expect("终端状态测试锁");
        let shared = TerminalShared::new();
        shared.flags().mark_raw();
        let mut buffer = InputBuffer::default();
        let cancel = AtomicBool::new(false);
        let pipe = pipe::shared();
        let value = read_line_plain("", &cancel, &mut buffer, &shared, &pipe)
            .expect("模式错误应可恢复");
        shared.flags().clear_raw();
        let Value::Data(result) = value else {
            panic!("结果应为 Result");
        };
        assert_eq!(result.variant.as_str(), "Err");
        let Value::Data(error) = &result.fields[0] else {
            panic!("错误负载应为 InputError");
        };
        assert_eq!(error.variant.as_str(), "RawModeActive");
    }
}
