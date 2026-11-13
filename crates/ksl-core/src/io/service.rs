//! 标准 IO 服务.
//!
//! 服务是协议与执行分离的宿主线程: 输出内建函数只负责投递, 不等待写出
//! 完成; 输入请求与输出共用邮箱, 由专用输入线程执行. 请求按投递顺序串行
//! 执行, Flush 是宿主的同步屏障, 用于读取输出跟踪状态与程序结束前收尾.
//!
//! 输出请求没有回复, 首次写出错误记录在共享槽中, 由下一个同步点报告.
//! 输入请求经同一邮箱进入, 由专用输入线程执行, 服务线程等待期间继续处理
//! 输出请求; 回复经调度器的 I/O 完成通道回到等待者, 对应每请求一个的回复句柄.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, LazyLock};

use crossbeam_channel::{Sender, bounded, unbounded};
use parking_lot::Mutex;

use super::Done;
use crate::error::RuntimeError as Error;

/// 标准 IO 服务的内部请求; `IORequest` 只覆盖用户可见的协议变体.
pub(super) enum Request {
    Print(String),
    PrintRaw(String),
    PrintError(String),
    ReadLine {
        prompt: String,
        plain: bool,
        cancel: Arc<AtomicBool>,
        done: Done,
    },
    ReadKey {
        cancel: Arc<AtomicBool>,
        done: Done,
    },
    /// 等待此前请求全部完成; 不产生语言层可见的请求或回复.
    Flush(Sender<()>),
}

struct Service {
    requests: Sender<Request>,
}

static SERVICE: LazyLock<Service> = LazyLock::new(Service::start);

/// 首次输出错误; 输出请求没有回复, 错误在下一个同步点报告.
static OUTPUT_ERROR: Mutex<Option<Error>> = Mutex::new(None);

impl Service {
    fn start() -> Self {
        let (requests, receiver) = unbounded();
        std::thread::Builder::new()
            .name("ksl-io-service".to_owned())
            .spawn(move || {
                for request in receiver {
                    match request {
                        Request::Print(text) => {
                            let mut line = String::with_capacity(text.len() + 1);
                            line.push_str(&text);
                            line.push('\n');
                            write_stdout(&line, "写出标准输出");
                        },
                        Request::PrintRaw(text) => write_stdout(&text, "写出标准输出"),
                        Request::PrintError(text) => write_stderr(&text),
                        Request::ReadLine {
                            prompt,
                            plain,
                            cancel,
                            done,
                        } => super::input::submit_line(prompt, plain, cancel, done),
                        Request::ReadKey { cancel, done } => {
                            super::input::submit_key(cancel, done);
                        },
                        Request::Flush(ack) => {
                            let _ = ack.send(());
                        },
                    }
                }
            })
            .expect("无法启动 IO 服务线程");
        Self { requests }
    }
}

/// 投递一条输出请求; 服务线程已停止时报告内部错误.
fn submit(request: Request) -> Result<(), Error> {
    SERVICE
        .requests
        .send(request)
        .map_err(|_| Error::internal("IO 服务已停止"))
}

/// 投递一条带回复的请求; 服务停止时立即把错误交给回复句柄.
///
/// 输入请求的调用方会挂起等待回复, 不能像输出请求那样静默丢弃.
fn submit_reply(request: Request) -> Result<(), Error> {
    match SERVICE.requests.send(request) {
        Ok(()) => Ok(()),
        Err(error) => {
            let (Request::ReadLine { done, .. } | Request::ReadKey { done, .. }) = error.0
            else {
                return Err(Error::internal("IO 服务已停止"));
            };
            done(Err(Error::internal("IO 服务已停止")));
            Ok(())
        },
    }
}

pub(crate) fn print(text: &str) -> Result<(), Error> {
    submit(Request::Print(text.to_owned()))
}

pub(crate) fn print_raw(text: &str) -> Result<(), Error> {
    submit(Request::PrintRaw(text.to_owned()))
}

pub(crate) fn print_error(text: &str) -> Result<(), Error> {
    submit(Request::PrintError(text.to_owned()))
}

pub(crate) fn read_line(
    prompt: String,
    plain: bool,
    cancel: Arc<AtomicBool>,
    done: Done,
) -> Result<(), Error> {
    submit_reply(Request::ReadLine {
        prompt,
        plain,
        cancel,
        done,
    })
}

pub(crate) fn read_key(cancel: Arc<AtomicBool>, done: Done) -> Result<(), Error> {
    submit_reply(Request::ReadKey { cancel, done })
}

/// 等待此前投递的输出全部完成, 并报告首次写出错误.
///
/// 输出请求没有回复, 因此错误只能在同步点发现; 服务停止时同样报告,
/// 避免宿主在服务失效后继续无输出地运行.
pub(crate) fn flush() -> Result<(), Error> {
    let (ack_tx, ack_rx) = bounded(1);
    submit(Request::Flush(ack_tx))?;
    ack_rx.recv().map_err(|_| Error::internal("IO 服务已停止"))?;
    match OUTPUT_ERROR.lock().clone() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// 记录首次写出错误; 之后的错误不覆盖它, 保留最早的失败原因.
fn record(error: Error) {
    let mut slot = OUTPUT_ERROR.lock();
    if slot.is_none() {
        *slot = Some(error);
    }
}

fn write_stdout(text: &str, action: &str) {
    if let Err(error) = super::write_stdout(text) {
        record(super::output_error(&error, action));
    }
}

fn write_stderr(text: &str) {
    if let Err(error) = super::write_stderr(text) {
        record(super::output_error(&error, "写出标准错误"));
    }
}
