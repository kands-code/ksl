//! 标准输出服务.
//!
//! 输出请求只投递, 不等待写出完成.
//! `Flush` 是宿主的同步屏障, 用于读取输出跟踪状态与程序结束前收尾.
//! 首次写出错误记录在服务线程中, 由下一个同步点报告.
//! 输入请求由 `input` 模块的专用线程处理, 不经过本服务.

use std::sync::Arc;

use crossbeam_channel::{Sender, bounded, unbounded};
use ksl_diagnostics::error::RuntimeError as Error;

use super::display::{self, TerminalShared};

/// 标准输出服务的内部请求.
enum Request {
    Print(String),
    PrintError(String),
    /// 等待先前请求全部完成, 并返回首次写出错误.
    Flush(Sender<Option<Error>>),
}

/// 一次运行时拥有的标准输出服务.
pub(super) struct OutputService {
    requests: Sender<Request>,
}

impl OutputService {
    pub(super) fn start(shared: Arc<TerminalShared>) -> Self {
        let (requests, receiver) = unbounded();
        std::thread::Builder::new()
            .name("ksl-io-output".to_owned())
            .spawn(move || {
                let mut output_error = None;
                for request in receiver {
                    match request {
                        Request::Print(text) => {
                            if let Err(error) = display::print(&shared, &text) {
                                record(&mut output_error, error);
                            }
                        },
                        Request::PrintError(text) => {
                            if let Err(error) = display::print_error(&shared, &text) {
                                record(&mut output_error, error);
                            }
                        },
                        Request::Flush(ack) => {
                            let _ = ack.send(output_error.clone());
                        },
                    }
                }
            })
            .expect("无法启动 IO 输出线程");
        Self { requests }
    }

    /// 投递一条输出请求.
    pub(super) fn print(&self, text: &str) -> Result<(), Error> {
        self.submit(Request::Print(text.to_owned()))
    }

    /// 投递一条标准错误请求.
    pub(super) fn print_error(&self, text: &str) -> Result<(), Error> {
        self.submit(Request::PrintError(text.to_owned()))
    }

    /// 等待先前投递的输出全部完成, 并报告首次写出错误.
    pub(super) fn flush(&self) -> Result<(), Error> {
        let (ack_tx, ack_rx) = bounded(1);
        self.submit(Request::Flush(ack_tx))?;
        match ack_rx.recv() {
            Ok(Some(error)) => Err(error),
            Ok(None) => Ok(()),
            Err(_) => Err(Error::internal("IO 服务已停止")),
        }
    }

    fn submit(&self, request: Request) -> Result<(), Error> {
        self.requests
            .send(request)
            .map_err(|_| Error::internal("IO 服务已停止"))
    }
}

/// 记录首次写出错误; 后续错误不覆盖.
fn record(slot: &mut Option<Error>, error: Error) {
    if slot.is_none() {
        *slot = Some(error);
    }
}
