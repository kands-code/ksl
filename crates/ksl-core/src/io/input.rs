//! 专用标准输入线程.
//!
//! 标准输入与 crossterm 事件源都要求串行访问. 由同一个常驻线程处理读取请求,
//! 既避免每次调用新建线程, 也保证 `event::poll` 与 `event::read` 在同一线程.
//! 线程空闲时只等待请求, 不主动读取标准输入, 因此不会抢走 REPL 的输入.
//!
//! 取消是协作式的: 排队中的请求在开始前检查取消标记, 终端模式下的 `ReadLine`
//! 与 `ReadKey` 以短轮询检查标记; 管道模式的阻塞读取仍要等到一行到达.

use std::io::IsTerminal as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crossbeam_channel::{Sender, unbounded};
use crossterm::event::{Event, KeyEventKind};

use super::{key_value, output_error, strip_line_ending, write_stdout};
use crate::error::RuntimeError as Error;
use crate::terminal::{Ink, LineAction, LineEditor, RenderState, clear, draw, finish, layout};
use crate::value::{Value, option_none, option_some, result_err, result_ok};

/// 输入任务完成回调.
type Done = Box<dyn FnOnce(Result<Value, Error>) + Send + 'static>;

enum Request {
    Line {
        prompt: String,
        cancel: Arc<AtomicBool>,
        done: Done,
    },
    Key {
        cancel: Arc<AtomicBool>,
        done: Done,
    },
}

struct InputService {
    requests: Sender<Request>,
}

static INPUT: LazyLock<InputService> = LazyLock::new(InputService::start);

impl InputService {
    fn start() -> Self {
        let (requests, receiver) = unbounded();
        std::thread::Builder::new()
            .name("ksl-input".to_owned())
            .spawn(move || {
                for request in receiver {
                    match request {
                        Request::Line {
                            prompt,
                            cancel,
                            done,
                        } => {
                            let result = if canceled(&cancel) {
                                Err(cancelled())
                            } else {
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    read_line(&prompt, &cancel)
                                }))
                                .unwrap_or_else(|_| Err(Error::internal("标准输入线程异常")))
                            };
                            done(result);
                        },
                        Request::Key { cancel, done } => {
                            let result = if canceled(&cancel) {
                                Err(cancelled())
                            } else {
                                std::panic::catch_unwind(|| read_key(&cancel)).unwrap_or_else(
                                    |_| Err(Error::internal("标准输入线程异常")),
                                )
                            };
                            done(result);
                        },
                    }
                }
            })
            .expect("无法启动标准输入线程");
        Self { requests }
    }
}

pub(super) fn submit_line(
    prompt: String,
    cancel: Arc<AtomicBool>,
    done: impl FnOnce(Result<Value, Error>) + Send + 'static,
) {
    let _ = INPUT.requests.send(Request::Line {
        prompt,
        cancel,
        done: Box::new(done),
    });
}

pub(super) fn submit_key(
    cancel: Arc<AtomicBool>,
    done: impl FnOnce(Result<Value, Error>) + Send + 'static,
) {
    let _ = INPUT.requests.send(Request::Key {
        cancel,
        done: Box::new(done),
    });
}

fn canceled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn cancelled() -> Error {
    Error::internal("标准输入读取已取消")
}

/// 读一行标准输入.
///
/// 终端下复用 REPL 的行编辑, 提示先写出并冲刷; 非终端输入走逐行读取.
pub(super) fn read_line(prompt: &str, cancel: &AtomicBool) -> Result<Value, Error> {
    if canceled(cancel) {
        return Err(cancelled());
    }
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        read_line_terminal(prompt, cancel)
    } else {
        read_line_plain(prompt)
    }
}

/// 非终端输入: 写出提示后按行读取, 不做行编辑.
fn read_line_plain(prompt: &str) -> Result<Value, Error> {
    write_stdout(prompt).map_err(|error| output_error(&error, "写出提示"))?;
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Ok(result_ok(option_none())),
        Ok(_) => Ok(result_ok(option_some(Value::Str(Arc::from(
            strip_line_ending(line),
        ))))),
        Err(error) => Ok(result_err(super::io_error("Other", &error.to_string()))),
    }
}

/// 终端输入: 原始模式下用共享行编辑器读取一行.
fn read_line_terminal(prompt: &str, cancel: &AtomicBool) -> Result<Value, Error> {
    // 终端可能已由宿主置于原始模式, 此时本次读取不负责恢复.
    let already_raw = crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
    if !already_raw && let Err(error) = crossterm::terminal::enable_raw_mode() {
        return Ok(result_err(super::io_error(
            "Other",
            &format!("开启原始模式失败: {error}"),
        )));
    }
    let _raw = RawModeGuard {
        active: !already_raw,
    };
    let mut editor = LineEditor::new();
    let mut state = RenderState::new();
    let mut width = terminal_width();
    render_line(prompt, &editor, &mut state, width)?;
    loop {
        if canceled(cancel) {
            let _ = finish_line(prompt, &editor, &mut state, width);
            return Err(cancelled());
        }
        match crossterm::event::poll(Duration::from_millis(50)) {
            Ok(true) => match crossterm::event::read() {
                Ok(Event::Key(key)) if !matches!(key.kind, KeyEventKind::Release) => {
                    match editor.handle_key(key) {
                        LineAction::Continue => {
                            render_line(prompt, &editor, &mut state, width)?;
                        },
                        LineAction::Accept => {
                            finish_line(prompt, &editor, &mut state, width)?;
                            return Ok(result_ok(option_some(Value::Str(Arc::from(
                                editor.text(),
                            )))));
                        },
                        LineAction::Interrupt => {
                            clear_line(&mut state)?;
                            write_stdout("^C\r\n")
                                .map_err(|error| output_error(&error, "写出中断"))?;
                            return Ok(result_err(super::io_error("Interrupted", "")));
                        },
                        LineAction::Eof => {
                            finish_line(prompt, &editor, &mut state, width)?;
                            return Ok(result_ok(option_none()));
                        },
                        LineAction::Clear => {
                            write_stdout("\x1b[H\x1b[2J")
                                .map_err(|error| output_error(&error, "清屏"))?;
                            state.reset();
                            render_line(prompt, &editor, &mut state, width)?;
                        },
                    }
                },
                Ok(Event::Resize(cols, _)) => {
                    width = if cols == 0 { 80 } else { usize::from(cols) };
                    render_line(prompt, &editor, &mut state, width)?;
                },
                Ok(_) => {},
                Err(error) => {
                    return Ok(result_err(super::io_error(
                        "Other",
                        &format!("读取输入失败: {error}"),
                    )));
                },
            },
            Ok(false) => {},
            Err(error) => {
                return Ok(result_err(super::io_error(
                    "Other",
                    &format!("轮询输入失败: {error}"),
                )));
            },
        }
    }
}

fn terminal_width() -> usize {
    crossterm::terminal::size().map_or(
        80,
        |(width, _)| {
            if width == 0 { 80 } else { usize::from(width) }
        },
    )
}

/// 重绘提示符与当前行, 并把光标放回编辑位置.
fn render_line(
    prompt: &str,
    editor: &LineEditor,
    state: &mut RenderState,
    width: usize,
) -> Result<(), Error> {
    let layout = layout(prompt, editor.text(), editor.cursor(), width, |_| Ink::Plain);
    let mut buffer = Vec::new();
    draw(&mut buffer, state, &layout).map_err(|error| output_error(&error, "绘制输入"))?;
    write_render(buffer)
}

/// 把光标移到当前行末尾并换行.
fn finish_line(
    prompt: &str,
    editor: &LineEditor,
    state: &mut RenderState,
    width: usize,
) -> Result<(), Error> {
    let layout = layout(prompt, editor.text(), editor.cursor(), width, |_| Ink::Plain);
    let mut buffer = Vec::new();
    finish(&mut buffer, state, &layout).map_err(|error| output_error(&error, "写完输入"))?;
    write_render(buffer)
}

/// 清除当前输入区, 光标回到第一行行首.
fn clear_line(state: &mut RenderState) -> Result<(), Error> {
    let mut buffer = Vec::new();
    clear(&mut buffer, state).map_err(|error| output_error(&error, "清除输入"))?;
    write_render(buffer)
}

fn write_render(buffer: Vec<u8>) -> Result<(), Error> {
    let text = String::from_utf8(buffer).map_err(|_| Error::internal("终端输出不是 UTF-8"))?;
    write_stdout(&text).map_err(|error| output_error(&error, "写出终端画面"))
}

/// 读一个按键.
///
/// 读键期间在必要时临时开启终端原始模式, 读完立即恢复; 调用方不需要自行
/// 管理终端模式. 只返回建模过的按键: 松开事件, 修饰键自身与媒体键一类
/// 事件被忽略.
pub(super) fn read_key(cancel: &AtomicBool) -> Result<Value, Error> {
    if !std::io::stdin().is_terminal() {
        return Ok(result_err(super::io_error("NotATerminal", "")));
    }
    // 终端可能已由宿主置于原始模式, 此时本次读取不负责恢复.
    let already_raw = crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
    if !already_raw && let Err(error) = crossterm::terminal::enable_raw_mode() {
        return Ok(result_err(super::io_error(
            "Other",
            &format!("开启原始模式失败: {error}"),
        )));
    }
    let _raw = RawModeGuard {
        active: !already_raw,
    };
    let value = loop {
        if canceled(cancel) {
            return Err(cancelled());
        }
        match crossterm::event::poll(Duration::from_millis(50)) {
            Ok(true) => match crossterm::event::read() {
                Ok(crossterm::event::Event::Key(key))
                    if matches!(
                        key.kind,
                        crossterm::event::KeyEventKind::Press
                            | crossterm::event::KeyEventKind::Repeat
                    ) =>
                {
                    if let Some(value) = key_value(key) {
                        break value;
                    }
                },
                Ok(_) => {},
                Err(error) => {
                    return Ok(result_err(super::io_error(
                        "Other",
                        &format!("读取按键失败: {error}"),
                    )));
                },
            },
            Ok(false) => {},
            Err(error) => {
                return Ok(result_err(super::io_error(
                    "Other",
                    &format!("轮询按键失败: {error}"),
                )));
            },
        }
    };
    Ok(result_ok(value))
}

/// 离开终端读取时为本次读取恢复终端模式.
///
/// 终端可能已被关闭或接管, 恢复失败没有补救手段, 忽略底层错误.
struct RawModeGuard {
    active: bool,
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        if self.active {
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}
