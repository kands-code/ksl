//! 专用标准输入线程.
//!
//! 标准输入与终端事件源都要求串行访问.
//! 读取请求与 REPL 的终端会话由同一个常驻线程处理.
//! `event::poll` 与 `event::read` 只出现在这条线程.
//! 线程空闲时只等待请求, 不主动读取标准输入.
//!
//! 取消是协作式的: 排队中的请求在开始前检查取消标记.
//! 终端模式下的 `ReadLine` 与 `ReadKey` 以短轮询检查取消标记与控制请求;
//! 原始模式持有被释放时, 借用中的读取会中止.
//! 管道请求等待预读缓冲时也按短周期检查, 已消费未交付的行退回缓冲.
//!
//! 读取流程在 `read`, 管道预读在 `pipe`.

use std::collections::VecDeque;
use std::io::IsTerminal as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::{Value, result_err, result_ok};

use self::pipe::{PipeShared, start_reader};
use self::read::{InputBuffer, ReadContext, read_key, read_line};
use super::display::TerminalShared;
use super::service::OutputService;
use super::{Done, input_error};
use crate::terminal::session::{self, SessionEvent};
use crate::terminal::state::{RawModeHolder, TerminalState};
use crate::terminal::{Session, TerminalHandle};

mod pipe;
mod read;

pub(super) enum Request {
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
        /// 终端恢复后通知客户端.
        restored: Sender<()>,
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

impl InputService {
    /// 启动专用输入线程.
    pub(super) fn start(shared: &Arc<TerminalShared>, output: &Arc<OutputService>) -> Self {
        let (requests, receiver) = unbounded();
        let pipe: PipeShared = pipe::shared();
        let thread_pipe = pipe.clone();
        let thread_shared = Arc::clone(shared);
        let thread_output = Arc::clone(output);
        let started =
            std::thread::Builder::new()
                .name("ksl-input".to_owned())
                .spawn(move || {
                    let mut state = InputLoop {
                        receiver,
                        buffer: InputBuffer::default(),
                        terminal: TerminalState::default(),
                        pending: VecDeque::new(),
                        controls: VecDeque::new(),
                        shared: thread_shared,
                        output: thread_output,
                        pipe: thread_pipe,
                    };
                    while let Some(request) = state.next() {
                        handle_request(&mut state, request);
                    }
                });
        if started.is_err() {
            // 接收端随闭包释放; 后续请求发送失败并由 `finish_stopped` 完成.
            return Self { requests };
        }
        if !std::io::stdin().is_terminal() {
            start_reader(pipe);
        }
        Self { requests }
    }
}

/// 输入线程循环持有的状态.
struct InputLoop {
    receiver: Receiver<Request>,
    buffer: InputBuffer,
    terminal: TerminalState,
    pending: VecDeque<Request>,
    controls: VecDeque<Request>,
    shared: Arc<TerminalShared>,
    output: Arc<OutputService>,
    pipe: PipeShared,
}

impl InputLoop {
    /// 按控制请求, 待处理请求, 新请求的顺序取下一条.
    fn next(&mut self) -> Option<Request> {
        self.controls
            .pop_front()
            .or_else(|| self.pending.pop_front())
            .or_else(|| self.receiver.recv().ok())
    }

    /// 读取一行; 读取期间的 panic 转为内部错误.
    fn handle_line(&mut self, prompt: &str, cancel: &AtomicBool) -> Result<Value, Error> {
        let mut ctx = ReadContext::new(
            &self.receiver,
            &mut self.pending,
            &mut self.controls,
            &mut self.terminal,
            &self.shared,
        );
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_line(
                prompt,
                cancel,
                &mut self.buffer,
                &mut ctx,
                &self.shared,
                &self.output,
                &self.pipe,
            )
        }))
        .unwrap_or_else(|_| {
            super::display::abandon(&self.shared);
            Err(Error::internal("标准输入线程异常"))
        })
    }

    /// 读取单键; 读取期间的 panic 转为内部错误.
    fn handle_key(&mut self, cancel: &AtomicBool) -> Result<Value, Error> {
        let mut ctx = ReadContext::new(
            &self.receiver,
            &mut self.pending,
            &mut self.controls,
            &mut self.terminal,
            &self.shared,
        );
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_key(cancel, &mut ctx, &self.shared, &self.output)
        }))
        .unwrap_or_else(|_| Err(Error::internal("标准输入线程异常")))
    }
}

fn handle_request(state: &mut InputLoop, request: Request) {
    match request {
        Request::Line {
            prompt,
            cancel,
            done,
        } => {
            let result = if canceled(&cancel) {
                Err(cancelled())
            } else {
                state.handle_line(&prompt, &cancel)
            };
            done(result);
        },
        Request::Key { cancel, done } => {
            let result = if canceled(&cancel) {
                Err(cancelled())
            } else {
                state.handle_key(&cancel)
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
                    acquire(&mut state.terminal, holder, &state.shared)
                } else {
                    release(&mut state.terminal, holder, &state.shared)
                };
                // 获取期间 Actor 可能已终止; 取到后立即归还.
                if enable && canceled(&cancel) && state.terminal.holder() == Some(holder) {
                    let _ = release(&mut state.terminal, holder, &state.shared);
                    done(Err(cancelled()));
                } else {
                    done(Ok(value));
                }
            }
        },
        Request::ReleaseHolder { holder } => {
            release_holder(&mut state.terminal, holder, &state.shared);
        },
        Request::ForceReset { done } => {
            done(Ok(force_reset(&mut state.terminal, &state.shared)));
        },
        Request::Session {
            bracketed_paste,
            events,
            control,
            ready,
            restored,
        } => {
            begin_session(&mut state.terminal, &state.shared);
            // 先恢复终端, 再通知客户端.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session::serve(
                    state.shared.terminal(),
                    state.shared.flags_handle(),
                    bracketed_paste,
                    &events,
                    &control,
                    &ready,
                );
            }));
            end_session(&mut state.terminal, &state.shared);
            let _ = restored.send(());
        },
        Request::TakeLine { ack } => {
            let _ = ack.send(state.buffer.take_line());
        },
        Request::TakeText { ack } => {
            let _ = ack.send(state.buffer.take_text());
        },
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

/// 输入线程已停止时直接完成请求.
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

pub(super) fn acquire(
    terminal: &mut TerminalState,
    holder: RawModeHolder,
    shared: &TerminalShared,
) -> Value {
    if let Some(current) = terminal.holder() {
        if current != holder {
            return result_err(input_error("RawModeHeld", ""));
        }
        if shared.terminal().is_raw_mode_enabled() {
            return result_ok(Value::Nil);
        }
    }
    if !std::io::stdin().is_terminal() {
        return result_err(input_error("NotATerminal", ""));
    }
    match shared.terminal().enable_raw_mode() {
        Ok(()) => {
            shared.flags().mark_raw();
            shared.flags().set_held(true);
            terminal.set_holder(holder);
            result_ok(Value::Nil)
        },
        Err(error) => result_err(input_error("Other", &format!("开启原始模式失败: {error}"))),
    }
}

pub(super) fn release(
    terminal: &mut TerminalState,
    holder: RawModeHolder,
    shared: &TerminalShared,
) -> Value {
    match terminal.holder() {
        None => result_ok(Value::Nil),
        Some(current) if current == holder => {
            terminal.clear();
            shared.flags().set_held(false);
            match shared.terminal().disable_raw_mode() {
                Ok(()) => {
                    shared.flags().clear_raw();
                    result_ok(Value::Nil)
                },
                Err(error) => {
                    result_err(input_error("Other", &format!("恢复终端状态失败: {error}")))
                },
            }
        },
        Some(_) => result_err(input_error("NotRawModeHolder", "")),
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
    match shared.terminal().disable_raw_mode() {
        Ok(()) => {
            shared.flags().clear_raw();
            result_ok(Value::Nil)
        },
        Err(error) => result_err(input_error("Other", &format!("恢复终端状态失败: {error}"))),
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
    if shared.terminal().disable_raw_mode().is_ok() {
        shared.flags().clear_raw();
    }
}

impl InputService {
    /// 恢复终端状态; 程序执行边界调用.
    ///
    /// 程序已显式退出原始模式且没有持有者时为空操作.
    /// 终端状态只由输入线程改动, 恢复同样走输入线程;
    /// 输入线程超时后直接恢复终端.
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
            && shared.terminal().disable_raw_mode().is_ok()
        {
            shared.flags().clear_raw();
            shared.flags().set_held(false);
        }
    }

    /// 请求输入线程开启终端会话; 返回时会话已就绪.
    pub(super) fn open_session(
        &self,
        terminal: TerminalHandle,
        bracketed_paste: bool,
    ) -> Result<Session, Error> {
        let (events_tx, events_rx) = bounded(64);
        let (control_tx, control_rx) = bounded(1);
        let (ready_tx, ready_rx) = bounded(1);
        let (restored_tx, restored_rx) = bounded(1);
        self.requests
            .send(Request::Session {
                bracketed_paste,
                events: events_tx,
                control: control_rx,
                ready: ready_tx,
                restored: restored_tx,
            })
            .map_err(|_| Error::internal("标准输入线程已停止"))?;
        match ready_rx.recv() {
            Ok(Ok(size)) => {
                Ok(Session::new(events_rx, control_tx, restored_rx, terminal, size))
            },
            Ok(Err(error)) => Err(error),
            Err(_) => Err(Error::internal("标准输入线程已停止")),
        }
    }
}

impl InputService {
    /// 取走终端模式中已消费但未交付的一行.
    ///
    /// REPL 进入读取前先取走该行, 避免取消程序读取后整行留在缓冲.
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
