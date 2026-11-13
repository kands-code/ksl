//! 专用标准输入线程.
//!
//! 标准输入与 crossterm 事件源都要求串行访问. 由同一个常驻线程处理读取请求,
//! 也由它执行 REPL 的终端会话, 保证 `event::poll` 与 `event::read` 只出现在
//! 这条线程. 线程空闲时只等待请求, 不主动读取标准输入.
//!
//! 取消是协作式的: 排队中的请求在开始前检查取消标记, 终端模式下的 `ReadLine`
//! 与 `ReadKey` 以短轮询检查标记; 管道请求等待预读缓冲时同样按短周期检查,
//! 已消费未交付的行退回缓冲.

use std::collections::VecDeque;
use std::io::IsTerminal as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use crossterm::event::{Event, KeyEventKind};
use parking_lot::{Condvar, Mutex};

use super::{Done, flush_output, key_value, output_error, strip_line_ending, write_stdout};
use crate::error::RuntimeError as Error;
use crate::terminal::session::{self, SessionEvent};
use crate::terminal::{
    Ink, LineAction, LineEditor, RenderState, Session, clear, draw, finish, layout,
};
use crate::value::{Value, option_none, option_some, result_err, result_ok};

enum Request {
    Line {
        prompt: String,
        plain: bool,
        cancel: Arc<AtomicBool>,
        done: Done,
    },
    Key {
        cancel: Arc<AtomicBool>,
        done: Done,
    },
    /// REPL 终端会话; 由输入线程持有终端状态并转发事件.
    Session {
        bracketed_paste: bool,
        events: Sender<SessionEvent>,
        control: Receiver<()>,
        ready: Sender<Result<(usize, usize), Error>>,
        finished: Sender<()>,
    },
    /// 取走管道模式中已消费但未交付的一行.
    TakeLine {
        ack: Sender<Option<String>>,
    },
    /// 取走终端模式中已编辑但未交付的文本.
    TakeText {
        ack: Sender<Option<String>>,
    },
    /// REPL 逐行输入; 先消费缓冲, 再从标准输入读取.
    ReadPlainLine {
        ack: Sender<Result<Option<String>, Error>>,
    },
}

struct InputService {
    requests: Sender<Request>,
}

/// 输入线程跨请求保留的未消费输入.
///
/// 请求被取消时, 已经读入但尚未交付的内容不能丢弃: 管道模式的行退回
/// 共享预读缓冲, 终端模式已输入的行文本退回编辑器初值. 标准输入是终端
/// 而标准输出不是终端时没有预读线程, 行暂存在这里的 `lines` 中.
#[derive(Default)]
pub(super) struct InputBuffer {
    lines: VecDeque<String>,
    text: Option<String>,
}

/// 管道模式预读缓冲上限, 按未消费字节计; 单行自身可以超过上限.
const PIPE_BUFFER_LIMIT: usize = 4 * 1024;

/// 管道模式的预读缓冲.
///
/// 读取线程持续读取标准输入, 消费端从缓冲取行; 缓冲达到上限时读取线程
/// 暂停, 为下游施加背压. 单行超过上限时整行仍会入缓冲, 避免半截行卡死.
#[derive(Default)]
struct PipeBuffer {
    lines: VecDeque<(String, usize)>,
    bytes: usize,
    closed: bool,
    error: Option<String>,
}

static PIPE: LazyLock<(Mutex<PipeBuffer>, Condvar)> =
    LazyLock::new(|| (Mutex::new(PipeBuffer::default()), Condvar::new()));

static INPUT: LazyLock<InputService> = LazyLock::new(InputService::start);

impl InputService {
    fn start() -> Self {
        let (requests, receiver) = unbounded();
        std::thread::Builder::new()
            .name("ksl-input".to_owned())
            .spawn(move || {
                let mut buffer = InputBuffer::default();
                for request in receiver {
                    match request {
                        Request::Line {
                            prompt,
                            plain,
                            cancel,
                            done,
                        } => {
                            let result = if canceled(&cancel) {
                                Err(cancelled())
                            } else {
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    read_line(&prompt, plain, &cancel, &mut buffer)
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
                        Request::Session {
                            bracketed_paste,
                            events,
                            control,
                            ready,
                            finished,
                        } => {
                            session::serve(
                                bracketed_paste,
                                &events,
                                &control,
                                &ready,
                                &finished,
                            );
                        },
                        Request::TakeLine { ack } => {
                            let line = if std::io::stdin().is_terminal() {
                                buffer.lines.pop_front()
                            } else {
                                take_pipe_line_now()
                            };
                            let _ = ack.send(line);
                        },
                        Request::TakeText { ack } => {
                            let _ = ack.send(buffer.text.take());
                        },
                        Request::ReadPlainLine { ack } => {
                            let result = if std::io::stdin().is_terminal() {
                                if let Some(line) = buffer.lines.pop_front() {
                                    Ok(Some(line))
                                } else {
                                    read_stdin_line().map_err(|error| {
                                        Error::internal(format!("读取输入失败: {error}"))
                                    })
                                }
                            } else {
                                match take_pipe_line(None) {
                                    Ok(line) => Ok(line),
                                    Err(PipeError::Cancelled) => {
                                        Err(Error::internal("标准输入读取已取消"))
                                    },
                                    Err(PipeError::Io(error)) => {
                                        Err(Error::internal(format!("读取输入失败: {error}")))
                                    },
                                }
                            };
                            let _ = ack.send(result);
                        },
                    }
                }
            })
            .expect("无法启动标准输入线程");
        if !std::io::stdin().is_terminal() {
            start_pipe_reader();
        }
        Self { requests }
    }
}

/// 启动管道预读线程; 只在标准输入不是终端时调用.
fn start_pipe_reader() {
    std::thread::Builder::new()
        .name("ksl-pipe-input".to_owned())
        .spawn(|| {
            loop {
                // 缓冲满时暂停读取; 消费端取行后会唤醒.
                {
                    let mut buffer = PIPE.0.lock();
                    while buffer.bytes >= PIPE_BUFFER_LIMIT && !buffer.closed {
                        PIPE.1.wait(&mut buffer);
                    }
                    if buffer.closed {
                        break;
                    }
                }
                let mut raw = String::new();
                match std::io::stdin().read_line(&mut raw) {
                    Ok(0) => {
                        let mut buffer = PIPE.0.lock();
                        buffer.closed = true;
                        PIPE.1.notify_all();
                        break;
                    },
                    Ok(_) => {
                        let line = strip_line_ending(raw);
                        let bytes = line.len() + 1;
                        let mut buffer = PIPE.0.lock();
                        buffer.bytes += bytes;
                        buffer.lines.push_back((line, bytes));
                        PIPE.1.notify_all();
                    },
                    Err(error) => {
                        let mut buffer = PIPE.0.lock();
                        buffer.error = Some(error.to_string());
                        buffer.closed = true;
                        PIPE.1.notify_all();
                        break;
                    },
                }
            }
        })
        .expect("无法启动管道输入线程");
}

pub(super) fn submit_line(prompt: String, plain: bool, cancel: Arc<AtomicBool>, done: Done) {
    let _ = INPUT.requests.send(Request::Line {
        prompt,
        plain,
        cancel,
        done,
    });
}

pub(super) fn submit_key(cancel: Arc<AtomicBool>, done: Done) {
    let _ = INPUT.requests.send(Request::Key { cancel, done });
}

/// 请求输入线程开启终端会话; 返回时会话已就绪.
pub(crate) fn open_session(bracketed_paste: bool) -> Result<Session, Error> {
    flush_output()?;
    let (events_tx, events_rx) = bounded(64);
    let (control_tx, control_rx) = bounded(1);
    let (ready_tx, ready_rx) = bounded(1);
    let (finished_tx, finished_rx) = bounded(1);
    INPUT
        .requests
        .send(Request::Session {
            bracketed_paste,
            events: events_tx,
            control: control_rx,
            ready: ready_tx,
            finished: finished_tx,
        })
        .map_err(|_| Error::internal("标准输入线程已停止"))?;
    match ready_rx.recv() {
        Ok(Ok(size)) => Ok(Session::new(events_rx, control_tx, finished_rx, size)),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(Error::internal("标准输入线程已停止")),
    }
}

/// 取走管道模式中已消费但未交付的一行.
///
/// REPL 进入读取前先消费这里的内容, 避免取消某个程序读取后把整行输入
/// 留在输入线程里, 而 REPL 又在标准输入上等待下一行.
#[must_use]
pub fn take_buffered_line() -> Option<String> {
    let (ack, ack_rx) = bounded(1);
    INPUT.requests.send(Request::TakeLine { ack }).ok()?;
    ack_rx.recv().ok().flatten()
}

/// 取走终端模式中已编辑但未交付的文本; REPL 用它作为编辑器初值.
#[must_use]
pub fn take_buffered_text() -> Option<String> {
    let (ack, ack_rx) = bounded(1);
    INPUT.requests.send(Request::TakeText { ack }).ok()?;
    ack_rx.recv().ok().flatten()
}

/// REPL 读取下一行; 先消费取消缓冲, 再从标准输入读取.
///
/// # Errors
///
/// 标准输入读取失败或输入线程已停止.
pub fn read_repl_line() -> Result<Option<String>, Error> {
    let (ack, ack_rx) = bounded(1);
    INPUT
        .requests
        .send(Request::ReadPlainLine { ack })
        .map_err(|_| Error::internal("标准输入线程已停止"))?;
    ack_rx
        .recv()
        .map_err(|_| Error::internal("标准输入线程已停止"))?
}

fn canceled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn cancelled() -> Error {
    Error::internal("标准输入读取已取消")
}

/// 管道读取的失败类别; 取消与宿主读取错误走不同的语言层映射.
enum PipeError {
    Cancelled,
    Io(String),
}

/// 从管道缓冲取一行; 缓冲为空时等待预读线程.
fn take_pipe_line(cancel: Option<&AtomicBool>) -> Result<Option<String>, PipeError> {
    loop {
        if cancel.is_some_and(canceled) {
            return Err(PipeError::Cancelled);
        }
        let mut buffer = PIPE.0.lock();
        if let Some((line, bytes)) = buffer.lines.pop_front() {
            // 取出后才发现取消: 行退回队列, 不调整字节计数.
            if cancel.is_some_and(canceled) {
                buffer.lines.push_front((line, bytes));
                return Err(PipeError::Cancelled);
            }
            buffer.bytes = buffer.bytes.saturating_sub(bytes);
            PIPE.1.notify_all();
            return Ok(Some(line));
        }
        if buffer.closed {
            return match &buffer.error {
                Some(error) => Err(PipeError::Io(error.clone())),
                None => Ok(None),
            };
        }
        PIPE.1.wait_for(&mut buffer, Duration::from_millis(50));
    }
}

/// 非阻塞取一行; 供 REPL 在进入读取前消费缓冲.
fn take_pipe_line_now() -> Option<String> {
    let mut buffer = PIPE.0.lock();
    let (line, bytes) = buffer.lines.pop_front()?;
    buffer.bytes = buffer.bytes.saturating_sub(bytes);
    PIPE.1.notify_all();
    Some(line)
}

/// 读一行标准输入.
///
/// `plain` 为真时始终按行读取; 否则终端下复用 REPL 的行编辑. 提示先写出
/// 并冲刷, 非终端输入走逐行读取.
pub(super) fn read_line(
    prompt: &str,
    plain: bool,
    cancel: &AtomicBool,
    buffer: &mut InputBuffer,
) -> Result<Value, Error> {
    if canceled(cancel) {
        return Err(cancelled());
    }
    // 先冲刷已有输出, 保证提示之前排队的 Print 已经落到终端.
    flush_output()?;
    if !plain && std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        read_line_terminal(prompt, cancel, buffer)
    } else {
        read_line_plain(prompt, cancel, buffer)
    }
}

/// 按行输入: 写出提示后按行读取, 不做行编辑.
fn read_line_plain(
    prompt: &str,
    cancel: &AtomicBool,
    buffer: &mut InputBuffer,
) -> Result<Value, Error> {
    write_stdout(prompt).map_err(|error| output_error(&error, "写出提示"))?;
    // 标准输入不是终端时由预读线程统一消费, 缓冲可跨请求保留.
    if !std::io::stdin().is_terminal() {
        return match take_pipe_line(Some(cancel)) {
            Ok(Some(line)) => Ok(result_ok(option_some(Value::Str(Arc::from(line))))),
            Ok(None) => Ok(result_ok(option_none())),
            Err(PipeError::Cancelled) => Err(cancelled()),
            Err(PipeError::Io(error)) => Ok(result_err(super::io_error("Other", &error))),
        };
    }
    if let Some(line) = buffer.lines.pop_front() {
        if canceled(cancel) {
            buffer.lines.push_front(line);
            return Err(cancelled());
        }
        return Ok(result_ok(option_some(Value::Str(Arc::from(line)))));
    }
    if canceled(cancel) {
        return Err(cancelled());
    }
    match read_stdin_line() {
        Ok(None) => Ok(result_ok(option_none())),
        Ok(Some(line)) => {
            // 读到行后才发现取消: 行已从标准输入消费, 退回缓冲供下一请求.
            if canceled(cancel) {
                buffer.lines.push_back(line);
                return Err(cancelled());
            }
            Ok(result_ok(option_some(Value::Str(Arc::from(line)))))
        },
        Err(error) => Ok(result_err(super::io_error("Other", &error.to_string()))),
    }
}

/// 从标准输入读一行并去掉行尾; EOF 返回 None.
fn read_stdin_line() -> std::io::Result<Option<String>> {
    let mut raw = String::new();
    match std::io::stdin().read_line(&mut raw) {
        Ok(0) => Ok(None),
        Ok(_) => Ok(Some(strip_line_ending(raw))),
        Err(error) => Err(error),
    }
}

/// 终端输入: 原始模式下用共享行编辑器读取一行.
fn read_line_terminal(
    prompt: &str,
    cancel: &AtomicBool,
    buffer: &mut InputBuffer,
) -> Result<Value, Error> {
    // 终端可能已由宿主置于原始模式, 守卫只恢复自己开启的部分.
    let _raw = match session::RawGuard::start(false) {
        Ok(guard) => guard,
        Err(error) => {
            return Ok(result_err(super::io_error(
                "Other",
                &format!("开启原始模式失败: {error}"),
            )));
        },
    };
    let mut editor = LineEditor::new();
    if let Some(text) = buffer.text.take() {
        editor.set_text(text);
    }
    let mut state = RenderState::new();
    let mut width = terminal_width();
    render_line(prompt, &editor, &mut state, width)?;
    loop {
        if canceled(cancel) {
            // 编辑中的文本属于未交付输入, 退回缓冲供下一请求继续.
            buffer.text = Some(editor.text().to_owned());
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
    // 与 ReadLine 相同: 应答输入请求前先冲刷已有输出.
    flush_output()?;
    if !std::io::stdin().is_terminal() {
        return Ok(result_err(super::io_error("NotATerminal", "")));
    }
    // 终端可能已由宿主置于原始模式, 守卫只恢复自己开启的部分.
    let _raw = match session::RawGuard::start(false) {
        Ok(guard) => guard,
        Err(error) => {
            return Ok(result_err(super::io_error(
                "Other",
                &format!("开启原始模式失败: {error}"),
            )));
        },
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
