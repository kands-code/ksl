//! 专用标准输入线程.
//!
//! 标准输入与 crossterm 事件源都要求串行访问.
//! 因此读取请求与 REPL 的终端会话由同一个常驻线程处理.
//! `event::poll` 与 `event::read` 只出现在这条线程.
//! 线程空闲时只等待请求, 不主动读取标准输入.
//!
//! 取消是协作式的: 排队中的请求在开始前检查取消标记.
//! 终端模式下的 `ReadLine` 与 `ReadKey` 以短轮询检查取消标记与控制请求;
//! 原始模式持有被释放时, 借用中的读取会中止.
//! 管道请求等待预读缓冲时也按短周期检查, 已消费未交付的行退回缓冲.

use std::collections::VecDeque;
use std::io::IsTerminal as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use arcstr::ArcStr;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use crossterm::event::{Event, KeyEventKind};
use ksl_frontend::error::RuntimeError as Error;
use parking_lot::{Condvar, Mutex};

use super::display::TerminalShared;
use super::service::OutputService;
use super::{Done, key_value, output_error, strip_line_ending};
use crate::terminal::session::{self, SessionEvent};
use crate::terminal::state::{RawModeHolder, TerminalState};
use crate::terminal::{LineAction, Session};
use crate::value::{Value, option_none, option_some, result_err, result_ok};

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
    /// 进入或退出终端原始模式; 与读键在同一线程串行执行.
    RawMode {
        enable: bool,
        holder: RawModeHolder,
        cancel: Arc<AtomicBool>,
        done: Done,
    },
    /// 释放持有者; Actor 终止时投递, 无回复.
    ReleaseHolder {
        holder: RawModeHolder,
    },
    /// 清理持有者并恢复终端; 宿主在运行边界投递.
    ForceReset {
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
    /// 取走终端模式中已消费但未交付的一行.
    TakeLine {
        ack: Sender<Option<String>>,
    },
    /// 取走终端模式中已编辑但未交付的文本.
    TakeText {
        ack: Sender<Option<String>>,
    },
}

pub(super) struct InputService {
    requests: Sender<Request>,
}

/// 输入线程跨请求保留的未消费输入.
///
/// 请求被取消时, 已经读入但尚未交付的内容不能丢弃.
/// 管道模式的行退回共享预读缓冲, 终端模式已输入的行文本退回编辑器初值.
/// 标准输入是终端而标准输出不是终端时没有预读线程, 行暂存在 `lines` 中.
#[derive(Default)]
pub(super) struct InputBuffer {
    lines: VecDeque<String>,
    text: Option<String>,
}

/// 管道模式预读缓冲上限, 按未消费字节计; 单行自身可以超过上限.
const PIPE_BUFFER_LIMIT: usize = 4 * 1024;

/// 管道模式的预读缓冲.
///
/// 读取线程持续读取标准输入, 消费端从缓冲取行.
/// 缓冲达到上限时读取线程暂停, 为下游施加背压.
/// 单行超过上限时整行仍会入缓冲, 避免半截行卡死.
#[derive(Default)]
struct PipeBuffer {
    lines: VecDeque<(String, usize)>,
    bytes: usize,
    closed: bool,
    error: Option<String>,
}

/// 管道模式的共享预读缓冲.
type PipeShared = Arc<(Mutex<PipeBuffer>, Condvar)>;

impl InputService {
    /// 启动专用输入线程.
    pub(super) fn start(shared: &Arc<TerminalShared>, output: &Arc<OutputService>) -> Self {
        let (requests, receiver) = unbounded();
        let pipe: PipeShared = Arc::new((Mutex::new(PipeBuffer::default()), Condvar::new()));
        let thread_pipe = Arc::clone(&pipe);
        let thread_shared = Arc::clone(shared);
        let thread_output = Arc::clone(output);
        let started =
            std::thread::Builder::new()
                .name("ksl-input".to_owned())
                .spawn(move || {
                    let mut buffer = InputBuffer::default();
                    let mut terminal = TerminalState::default();
                    let mut pending: VecDeque<Request> = VecDeque::new();
                    let mut controls: VecDeque<Request> = VecDeque::new();
                    loop {
                        let request = if let Some(request) = controls.pop_front() {
                            request
                        } else if let Some(request) = pending.pop_front() {
                            request
                        } else {
                            match receiver.recv() {
                                Ok(request) => request,
                                Err(_) => break,
                            }
                        };
                        handle_request(
                            request,
                            &receiver,
                            &mut buffer,
                            &mut terminal,
                            &mut pending,
                            &mut controls,
                            &thread_shared,
                            &thread_output,
                            &thread_pipe,
                        );
                    }
                });
        if started.is_err() {
            // 接收端随闭包释放; 后续请求发送失败并由 `finish_stopped` 完成.
            return Self { requests };
        }
        if !std::io::stdin().is_terminal() {
            start_pipe_reader(pipe);
        }
        Self { requests }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_request(
    request: Request,
    receiver: &Receiver<Request>,
    buffer: &mut InputBuffer,
    terminal: &mut TerminalState,
    pending: &mut VecDeque<Request>,
    controls: &mut VecDeque<Request>,
    shared: &TerminalShared,
    output: &OutputService,
    pipe: &PipeShared,
) {
    match request {
        Request::Line {
            prompt,
            cancel,
            done,
        } => {
            let result = if canceled(&cancel) {
                Err(cancelled())
            } else {
                let mut ctx = ReadContext {
                    requests: receiver,
                    pending,
                    controls,
                    terminal,
                    shared,
                };
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    read_line(&prompt, &cancel, buffer, &mut ctx, shared, output, pipe)
                }))
                .unwrap_or_else(|_| {
                    super::display::abandon(shared);
                    Err(Error::internal("标准输入线程异常"))
                })
            };
            done(result);
        },
        Request::Key { cancel, done } => {
            let result = if canceled(&cancel) {
                Err(cancelled())
            } else {
                let mut ctx = ReadContext {
                    requests: receiver,
                    pending,
                    controls,
                    terminal,
                    shared,
                };
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    read_key(&cancel, &mut ctx, shared, output)
                }))
                .unwrap_or_else(|_| Err(Error::internal("标准输入线程异常")))
            };
            done(result);
        },
        Request::RawMode {
            enable,
            holder,
            cancel,
            done,
        } => {
            if canceled(&cancel) {
                done(Err(cancelled()));
            } else {
                let value = if enable {
                    acquire(terminal, holder, shared)
                } else {
                    release(terminal, holder, shared)
                };
                // 获取期间 Actor 可能已终止; 立即归还, 避免死 Actor 持有终端.
                if enable && canceled(&cancel) && terminal.holder() == Some(holder) {
                    let _ = release(terminal, holder, shared);
                    done(Err(cancelled()));
                } else {
                    done(Ok(value));
                }
            }
        },
        Request::ReleaseHolder { holder } => {
            release_holder(terminal, holder, shared);
        },
        Request::ForceReset { done } => {
            done(Ok(force_reset(terminal, shared)));
        },
        Request::Session {
            bracketed_paste,
            events,
            control,
            ready,
            finished,
        } => {
            begin_session(terminal, shared);
            session::serve(
                shared.flags_handle(),
                bracketed_paste,
                &events,
                &control,
                &ready,
                &finished,
            );
            end_session(terminal, shared);
        },
        Request::TakeLine { ack } => {
            let line = buffer.lines.pop_front();
            let _ = ack.send(line);
        },
        Request::TakeText { ack } => {
            let _ = ack.send(buffer.text.take());
        },
    }
}

/// 启动管道预读线程; 只在标准输入不是终端时调用.
fn start_pipe_reader(pipe: PipeShared) {
    let failure = Arc::clone(&pipe);
    let started = std::thread::Builder::new()
        .name("ksl-pipe-input".to_owned())
        .spawn(move || {
            loop {
                // 缓冲满时暂停读取; 消费端取行后会唤醒.
                {
                    let mut buffer = pipe.0.lock();
                    while buffer.bytes >= PIPE_BUFFER_LIMIT && !buffer.closed {
                        pipe.1.wait(&mut buffer);
                    }
                    if buffer.closed {
                        break;
                    }
                }
                let mut raw = String::new();
                match std::io::stdin().read_line(&mut raw) {
                    Ok(0) => {
                        let mut buffer = pipe.0.lock();
                        buffer.closed = true;
                        pipe.1.notify_all();
                        break;
                    },
                    Ok(_) => {
                        let line = strip_line_ending(raw);
                        let bytes = line.len() + 1;
                        let mut buffer = pipe.0.lock();
                        buffer.bytes += bytes;
                        buffer.lines.push_back((line, bytes));
                        pipe.1.notify_all();
                    },
                    Err(error) => {
                        let mut buffer = pipe.0.lock();
                        buffer.error = Some(error.to_string());
                        buffer.closed = true;
                        pipe.1.notify_all();
                        break;
                    },
                }
            }
        });
    if started.is_err() {
        let mut buffer = failure.0.lock();
        buffer.error = Some("无法启动管道输入线程".to_string());
        buffer.closed = true;
        failure.1.notify_all();
    }
}

impl InputService {
    pub(super) fn submit_line(&self, prompt: String, cancel: Arc<AtomicBool>, done: Done) {
        let request = Request::Line {
            prompt,
            cancel,
            done,
        };
        if let Err(error) = self.requests.send(request) {
            finish_stopped(error.0);
        }
    }

    pub(super) fn submit_key(&self, cancel: Arc<AtomicBool>, done: Done) {
        let request = Request::Key { cancel, done };
        if let Err(error) = self.requests.send(request) {
            finish_stopped(error.0);
        }
    }

    /// 提交原始模式切换请求; 由输入线程执行.
    pub(super) fn submit_raw_mode(
        &self,
        enable: bool,
        holder: RawModeHolder,
        cancel: Arc<AtomicBool>,
        done: Done,
    ) {
        let request = Request::RawMode {
            enable,
            holder,
            cancel,
            done,
        };
        if let Err(error) = self.requests.send(request) {
            finish_stopped(error.0);
        }
    }

    /// 提交持有者释放请求; 无回复.
    pub(super) fn submit_release_holder(&self, holder: RawModeHolder) {
        let _ = self.requests.send(Request::ReleaseHolder { holder });
    }

    /// 提交强制清理请求; 宿主在运行边界使用.
    pub(super) fn submit_force_reset(&self, done: Done) {
        let request = Request::ForceReset { done };
        if let Err(error) = self.requests.send(request) {
            finish_stopped(error.0);
        }
    }
}

/// 输入线程已停止时完成请求; 避免调用方永久等待.
fn finish_stopped(request: Request) {
    match request {
        Request::Line { done, .. }
        | Request::Key { done, .. }
        | Request::RawMode { done, .. }
        | Request::ForceReset { done } => {
            done(Err(Error::internal("标准输入线程已停止")));
        },
        Request::Session { ready, .. } => {
            let _ = ready.send(Err(Error::internal("标准输入线程已停止")));
        },
        Request::TakeLine { ack } | Request::TakeText { ack } => {
            let _ = ack.send(None);
        },
        Request::ReleaseHolder { .. } => {},
    }
}

/// 读取期间处理控制请求.
///
/// 返回真表示当前读取应停止, 下一轮由外层先处理 `controls`.
struct ReadContext<'a> {
    requests: &'a Receiver<Request>,
    pending: &'a mut VecDeque<Request>,
    controls: &'a mut VecDeque<Request>,
    terminal: &'a mut TerminalState,
    shared: &'a TerminalShared,
}

impl ReadContext<'_> {
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

fn acquire(
    terminal: &mut TerminalState,
    holder: RawModeHolder,
    shared: &TerminalShared,
) -> Value {
    if let Some(current) = terminal.holder() {
        if current != holder {
            return result_err(super::input_error("RawModeHeld", ""));
        }
        if crossterm::terminal::is_raw_mode_enabled().unwrap_or(false) {
            return result_ok(Value::Nil);
        }
    }
    if !std::io::stdin().is_terminal() {
        return result_err(super::input_error("NotATerminal", ""));
    }
    match crossterm::terminal::enable_raw_mode() {
        Ok(()) => {
            shared.flags().mark_raw();
            shared.flags().set_held(true);
            terminal.set_holder(holder);
            result_ok(Value::Nil)
        },
        Err(error) => result_err(super::input_error(
            "Other",
            &format!("开启原始模式失败: {error}"),
        )),
    }
}

fn release(
    terminal: &mut TerminalState,
    holder: RawModeHolder,
    shared: &TerminalShared,
) -> Value {
    match terminal.holder() {
        None => result_ok(Value::Nil),
        Some(current) if current == holder => {
            terminal.clear();
            shared.flags().set_held(false);
            match crossterm::terminal::disable_raw_mode() {
                Ok(()) => {
                    shared.flags().clear_raw();
                    result_ok(Value::Nil)
                },
                Err(error) => result_err(super::input_error(
                    "Other",
                    &format!("恢复终端状态失败: {error}"),
                )),
            }
        },
        Some(_) => result_err(super::input_error("NotRawModeHolder", "")),
    }
}

/// 持有者终止后的释放; 不关心恢复失败, 宿主在退出时恢复.
fn release_holder(
    terminal: &mut TerminalState,
    holder: RawModeHolder,
    shared: &TerminalShared,
) {
    if terminal.holder() == Some(holder) {
        let _ = release(terminal, holder, shared);
    }
}

/// 强制清理持有者并恢复终端.
fn force_reset(terminal: &mut TerminalState, shared: &TerminalShared) -> Value {
    terminal.clear();
    shared.flags().set_held(false);
    match crossterm::terminal::disable_raw_mode() {
        Ok(()) => {
            shared.flags().clear_raw();
            result_ok(Value::Nil)
        },
        Err(error) => result_err(super::input_error(
            "Other",
            &format!("恢复终端状态失败: {error}"),
        )),
    }
}

/// REPL 会话开始: 清掉遗留持有者, 登记会话持有.
fn begin_session(terminal: &mut TerminalState, shared: &TerminalShared) {
    terminal.clear();
    shared.flags().set_held(true);
    terminal.set_holder(RawModeHolder::Session);
}

/// REPL 会话结束: 释放持有并恢复终端.
fn end_session(terminal: &mut TerminalState, shared: &TerminalShared) {
    terminal.clear();
    shared.flags().set_held(false);
    if crossterm::terminal::disable_raw_mode().is_ok() {
        shared.flags().clear_raw();
    }
}

impl InputService {
    /// 恢复终端状态; 程序执行边界调用.
    ///
    /// 程序已显式退出原始模式且没有持有者时为空操作.
    /// 终端状态只由输入线程改动, 因此恢复同样走输入线程;
    /// 输入线程被占用而超时后, 直接调用 crossterm 恢复, 避免终端留在原始模式.
    pub(super) fn restore_terminal(&self, shared: &TerminalShared) {
        if !shared.flags().raw_active() && !shared.flags().held() {
            return;
        }
        let (ack, ack_rx) = bounded(1);
        let done: Done = Box::new(move |_result| {
            let _ = ack.send(());
        });
        self.submit_force_reset(done);
        // 输入线程阻塞在读取中时无法应答, 等待超时后直接恢复.
        let _ = ack_rx.recv_timeout(Duration::from_millis(500));
        if (shared.flags().raw_active() || shared.flags().held())
            && crossterm::terminal::disable_raw_mode().is_ok()
        {
            shared.flags().clear_raw();
            shared.flags().set_held(false);
        }
    }

    /// 请求输入线程开启终端会话; 返回时会话已就绪.
    pub(super) fn open_session(&self, bracketed_paste: bool) -> Result<Session, Error> {
        let (events_tx, events_rx) = bounded(64);
        let (control_tx, control_rx) = bounded(1);
        let (ready_tx, ready_rx) = bounded(1);
        let (finished_tx, finished_rx) = bounded(1);
        self.requests
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
}

impl InputService {
    /// 取走终端模式中已消费但未交付的一行.
    ///
    /// REPL 进入读取前必须先取走该行. 否则取消某个程序的逐行读取后,
    /// 整行输入留在输入缓冲, REPL 因此等不到下一行.
    #[must_use]
    pub(super) fn take_buffered_line(&self) -> Option<String> {
        let (ack, ack_rx) = bounded(1);
        self.requests.send(Request::TakeLine { ack }).ok()?;
        ack_rx.recv_timeout(Duration::from_millis(500)).ok().flatten()
    }

    /// 取走终端模式中已编辑但未交付的文本; REPL 用它作为编辑器初值.
    #[must_use]
    pub(super) fn take_buffered_text(&self) -> Option<String> {
        let (ack, ack_rx) = bounded(1);
        self.requests.send(Request::TakeText { ack }).ok()?;
        ack_rx.recv_timeout(Duration::from_millis(500)).ok().flatten()
    }
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
fn take_pipe_line(
    cancel: Option<&AtomicBool>,
    pipe: &PipeShared,
) -> Result<Option<String>, PipeError> {
    loop {
        if cancel.is_some_and(canceled) {
            return Err(PipeError::Cancelled);
        }
        let mut buffer = pipe.0.lock();
        if let Some((line, bytes)) = buffer.lines.pop_front() {
            // 取出后才发现取消: 行退回队列, 不调整字节计数.
            if cancel.is_some_and(canceled) {
                buffer.lines.push_front((line, bytes));
                return Err(PipeError::Cancelled);
            }
            buffer.bytes = buffer.bytes.saturating_sub(bytes);
            pipe.1.notify_all();
            return Ok(Some(line));
        }
        if buffer.closed {
            return match &buffer.error {
                Some(error) => Err(PipeError::Io(error.clone())),
                None => Ok(None),
            };
        }
        pipe.1.wait_for(&mut buffer, Duration::from_millis(50));
    }
}

/// 读一行标准输入.
///
/// 标准输入与标准输出都是终端时使用行编辑, 否则按行读取.
/// 提示先写出并冲刷.
fn read_line(
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
    // 先冲刷已有输出, 保证提示之前排队的 Print 已经落到终端.
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
        return Ok(result_err(super::input_error("RawModeActive", "")));
    }
    shared
        .stdio()
        .write_stdout(prompt)
        .map_err(|error| output_error(&error, "写出提示"))?;
    // 标准输入不是终端时由预读线程统一消费, 缓冲可跨请求保留.
    if !std::io::stdin().is_terminal() {
        return match take_pipe_line(Some(cancel), pipe) {
            Ok(Some(line)) => Ok(result_ok(option_some(Value::Str(ArcStr::from(line))))),
            Ok(None) => Ok(result_ok(option_none())),
            Err(PipeError::Cancelled) => Err(cancelled()),
            Err(PipeError::Io(error)) => Ok(result_err(super::input_error("Other", &error))),
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
        Err(error) => Ok(result_err(super::input_error("Other", &error.to_string()))),
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
    let _raw = match session::RawGuard::start(shared.flags_handle(), false) {
        Ok(guard) => guard,
        Err(error) => {
            return Ok(result_err(super::input_error(
                "Other",
                &format!("开启原始模式失败: {error}"),
            )));
        },
    };
    super::display::begin(shared, prompt.to_owned(), buffer.text.take(), terminal_width())?;
    loop {
        if ctx.drain() {
            // 持有者释放或强制重置: 保留已输入文本, 让外层先处理控制请求.
            if let Some(text) = super::display::finish(shared)? {
                buffer.text = Some(text);
            }
            return Ok(result_err(super::input_error("RawModeReleased", "")));
        }
        if canceled(cancel) {
            // 编辑中的文本属于未交付输入, 退回缓冲供下一请求继续.
            if let Some(text) = super::display::finish(shared)? {
                buffer.text = Some(text);
            }
            return Err(cancelled());
        }
        match crossterm::event::poll(Duration::from_millis(50)) {
            Ok(true) => match crossterm::event::read() {
                Ok(Event::Key(key)) if !matches!(key.kind, KeyEventKind::Release) => {
                    let action = match super::display::handle_key(shared, key) {
                        Ok(Some(action)) => action,
                        Ok(None) => {
                            super::display::abandon(shared);
                            return Err(Error::internal("终端编辑状态缺失"));
                        },
                        Err(error) => {
                            super::display::abandon(shared);
                            return Err(error);
                        },
                    };
                    match action {
                        LineAction::Continue => {},
                        LineAction::Accept => {
                            let text = super::display::finish(shared)?.unwrap_or_default();
                            return Ok(result_ok(option_some(Value::Str(ArcStr::from(text)))));
                        },
                        LineAction::Interrupt => {
                            let _ = super::display::interrupt(shared)?;
                            return Ok(result_err(super::input_error("Interrupted", "")));
                        },
                        LineAction::Eof => {
                            let _ = super::display::finish(shared)?;
                            return Ok(result_ok(option_none()));
                        },
                        LineAction::Clear => {
                            if let Err(error) = super::display::clear_screen(shared) {
                                super::display::abandon(shared);
                                return Err(error);
                            }
                        },
                    }
                },
                Ok(Event::Resize(cols, _)) => {
                    let width = if cols == 0 { 80 } else { usize::from(cols) };
                    if let Err(error) = super::display::resize(shared, width) {
                        super::display::abandon(shared);
                        return Err(error);
                    }
                },
                Ok(_) => {},
                Err(error) => {
                    super::display::abandon(shared);
                    return Ok(result_err(super::input_error(
                        "Other",
                        &format!("读取输入失败: {error}"),
                    )));
                },
            },
            Ok(false) => {},
            Err(error) => {
                super::display::abandon(shared);
                return Ok(result_err(super::input_error(
                    "Other",
                    &format!("轮询输入失败: {error}"),
                )));
            },
        }
    }
}

fn terminal_width() -> usize {
    crate::terminal::size_or(80, 24).0
}

/// 读一个按键.
///
/// 读键期间在必要时临时开启终端原始模式, 读完立即恢复.
/// 原始模式已有持有者时借用当前模式, 读取结束不恢复.
/// 只返回建模过的按键: 松开事件, 修饰键自身与媒体键一类事件被忽略.
fn read_key(
    cancel: &AtomicBool,
    ctx: &mut ReadContext<'_>,
    shared: &TerminalShared,
    output: &OutputService,
) -> Result<Value, Error> {
    // 与 ReadLine 相同: 应答输入请求前先冲刷已有输出.
    output.flush()?;
    if !std::io::stdin().is_terminal() {
        return Ok(result_err(super::input_error("NotATerminal", "")));
    }
    // 终端可能已由宿主置于原始模式, 守卫只恢复自己开启的部分.
    let _raw = match session::RawGuard::start(shared.flags_handle(), false) {
        Ok(guard) => guard,
        Err(error) => {
            return Ok(result_err(super::input_error(
                "Other",
                &format!("开启原始模式失败: {error}"),
            )));
        },
    };
    let value = loop {
        if ctx.drain() {
            return Ok(result_err(super::input_error("RawModeReleased", "")));
        }
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
                    return Ok(result_err(super::input_error(
                        "Other",
                        &format!("读取按键失败: {error}"),
                    )));
                },
            },
            Ok(false) => {},
            Err(error) => {
                return Ok(result_err(super::input_error(
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
    use ksl_base::symbol::resolve;

    use super::*;

    #[test]
    fn read_line_plain_rejects_raw_mode() {
        let _guard = session::TEST_LOCK.lock().expect("终端状态测试锁");
        let shared = TerminalShared::new();
        shared.flags().mark_raw();
        let mut buffer = InputBuffer::default();
        let cancel = AtomicBool::new(false);
        let pipe = Arc::new((Mutex::new(PipeBuffer::default()), Condvar::new()));
        let value = read_line_plain("", &cancel, &mut buffer, &shared, &pipe)
            .expect("模式错误应可恢复");
        shared.flags().clear_raw();
        let Value::Data(result) = value else {
            panic!("结果应为 Result");
        };
        assert_eq!(resolve(result.variant).as_ref(), "Err");
        let Value::Data(error) = &result.fields[0] else {
            panic!("错误负载应为 InputError");
        };
        assert_eq!(resolve(error.variant).as_ref(), "RawModeActive");
    }
}
