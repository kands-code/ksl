//! 套接字原语与任务构造.
//!
//! 语言层只暴露不透明句柄. 参数校验在此完成, 非阻塞 I/O 由网络引擎执行.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine::{Op, ReadMode};
use error::SocketError;
pub use handle::{SocketHandle, TcpListenerHandle};
use ksl_base::source::Span;
use ksl_diagnostics::error::{ErrorKind, RuntimeError as Error};
use ksl_value::args::{bytes_arg, str_arg, u64_arg};
use ksl_value::value::{Value, result_err};

use super::Task;
use crate::host;

pub mod engine;
mod error;
mod handle;

fn closed() -> Value {
    result_err(SocketError::Closed.to_value())
}

fn invalid(message: impl Into<String>) -> Value {
    result_err(SocketError::InvalidArgument(message.into()).to_value())
}

fn checked_timeout(timeout: Duration, span: Option<Span>) -> Result<Duration, Error> {
    if Instant::now().checked_add(timeout).is_none() {
        return Err(Error::runtime(ErrorKind::TimeoutOutOfRange, span));
    }
    Ok(timeout)
}

fn timeout_arg(
    args: &[Value],
    index: usize,
    context: &str,
    span: Option<Span>,
) -> Result<Duration, Error> {
    checked_timeout(Duration::from_millis(u64_arg(args, index, context)?), span)
}

fn socket_arg(args: &[Value], index: usize) -> Result<Arc<SocketHandle>, Error> {
    match args.get(index).and_then(host::as_socket) {
        Some(socket) => Ok(Arc::clone(socket)),
        None => Err(Error::internal("套接字实参类型不符")),
    }
}

fn listener_arg(args: &[Value], index: usize) -> Result<Arc<TcpListenerHandle>, Error> {
    match args.get(index).and_then(host::as_tcp_listener) {
        Some(listener) => Ok(Arc::clone(listener)),
        None => Err(Error::internal("监听器实参类型不符")),
    }
}

fn count_arg(value: u64) -> Option<usize> {
    usize::try_from(value).ok()
}

/// 构造网络任务; `target` 在操作完成前保持存活.
fn net_task(op: Op, target: Option<Value>) -> Task {
    Task::Net {
        op,
        roots: target.into_iter().collect(),
    }
}

/// 构造 TCP 监听任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn tcp_listen_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let host = str_arg(args, 0, "TcpListen 的 host")?.to_string();
    let port = u64_arg(args, 1, "TcpListen 的 port")?;
    let Ok(host) = host.parse::<IpAddr>() else {
        return Ok(Task::Ready(invalid("TcpListen 的 host 必须是 IP 字面量")));
    };
    let Ok(port) = u16::try_from(port) else {
        return Ok(Task::Ready(invalid("TcpListen 的 port 超出 0..=65535")));
    };
    Ok(net_task(
        Op::Listen {
            addr: SocketAddr::new(host, port),
        },
        None,
    ))
}

/// 构造 TCP 接受连接任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn tcp_accept_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let listener = listener_arg(args, 0)?;
    let timeout = timeout_arg(args, 1, "TcpAccept 的 timeout", span)?;
    if listener.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let id = listener.id();
    Ok(net_task(
        Op::Accept {
            listener: id,
            timeout,
        },
        Some(host::tcp_listener(listener)),
    ))
}

/// 构造 TCP 连接任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn tcp_connect_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let host = str_arg(args, 0, "TcpConnect 的 host")?.to_string();
    let port = u64_arg(args, 1, "TcpConnect 的 port")?;
    let timeout = timeout_arg(args, 2, "TcpConnect 的 timeout", span)?;
    if timeout.is_zero() {
        return Ok(Task::Ready(invalid("TcpConnect 的 timeout 必须大于 0")));
    }
    let Ok(host) = host.parse::<IpAddr>() else {
        return Ok(Task::Ready(invalid("TcpConnect 的 host 必须是 IP 字面量")));
    };
    let Ok(port) = u16::try_from(port) else {
        return Ok(Task::Ready(invalid("TcpConnect 的 port 超出 0..=65535")));
    };
    Ok(net_task(
        Op::Connect {
            addr: SocketAddr::new(host, port),
            timeout,
        },
        None,
    ))
}

/// 构造关闭监听器任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn tcp_close_listener_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let listener = listener_arg(args, 0)?;
    listener.close();
    Ok(Task::Ready(Value::Nil))
}

/// 构造取监听端口任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn tcp_local_port_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let listener = listener_arg(args, 0)?;
    Ok(Task::Ready(Value::U64(u64::from(listener.local_port()))))
}

/// 构造取对端地址任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn socket_peer_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    Ok(Task::Ready(Value::Str(socket.peer())))
}

/// 构造关闭套接字任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn socket_close_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    socket.close();
    Ok(Task::Ready(Value::Nil))
}

/// 构造套接字关闭状态查询任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn socket_is_closed_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    Ok(Task::Ready(Value::Bool(socket.is_closed())))
}

/// 构造关闭写方向任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_shutdown_write_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let timeout = timeout_arg(args, 1, "SocketShutdownWrite 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let id = socket.id();
    Ok(net_task(
        Op::ShutdownWrite {
            socket: id,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

/// 构造套接字读取任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_read_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let max = u64_arg(args, 1, "SocketRead 的 max")?;
    let timeout = timeout_arg(args, 2, "SocketRead 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let Some(max) = count_arg(max) else {
        return Ok(Task::Ready(invalid("SocketRead 的 max 超出平台可表示范围")));
    };
    if max == 0 {
        return Ok(Task::Ready(invalid("SocketRead 的 max 必须大于 0")));
    }
    let id = socket.id();
    Ok(net_task(
        Op::Read {
            socket: id,
            mode: ReadMode::Read,
            max,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

/// 构造读取定长数据任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_read_exact_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let count = u64_arg(args, 1, "SocketReadExact 的 count")?;
    let timeout = timeout_arg(args, 2, "SocketReadExact 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let Some(count) = count_arg(count) else {
        return Ok(Task::Ready(invalid(
            "SocketReadExact 的 count 超出平台可表示范围",
        )));
    };
    let id = socket.id();
    Ok(net_task(
        Op::Read {
            socket: id,
            mode: ReadMode::Exact,
            max: count,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

/// 构造读取全部数据任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_read_all_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let max = u64_arg(args, 1, "SocketReadAll 的 max")?;
    let timeout = timeout_arg(args, 2, "SocketReadAll 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let Some(max) = count_arg(max) else {
        return Ok(Task::Ready(invalid("SocketReadAll 的 max 超出平台可表示范围")));
    };
    if max == 0 {
        return Ok(Task::Ready(invalid("SocketReadAll 的 max 必须大于 0")));
    }
    let id = socket.id();
    Ok(net_task(
        Op::Read {
            socket: id,
            mode: ReadMode::All,
            max,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

/// 构造读取一行任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_read_line_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let max = u64_arg(args, 1, "SocketReadLine 的 max")?;
    let timeout = timeout_arg(args, 2, "SocketReadLine 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let Some(max) = count_arg(max) else {
        return Ok(Task::Ready(invalid("SocketReadLine 的 max 超出平台可表示范围")));
    };
    if max == 0 {
        return Ok(Task::Ready(invalid("SocketReadLine 的 max 必须大于 0")));
    }
    let id = socket.id();
    Ok(net_task(
        Op::Read {
            socket: id,
            mode: ReadMode::Line,
            max,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

/// 构造套接字写入任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_write_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let bytes = bytes_arg(args, 1, "套接字写入内容")?;
    let timeout = timeout_arg(args, 2, "SocketWrite 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let id = socket.id();
    Ok(net_task(
        Op::Write {
            socket: id,
            bytes,
            all: false,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

/// 构造套接字写满任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_write_all_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let bytes = bytes_arg(args, 1, "套接字写入内容")?;
    let timeout = timeout_arg(args, 2, "SocketWriteAll 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let id = socket.id();
    Ok(net_task(
        Op::Write {
            socket: id,
            bytes,
            all: true,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

/// 构造写入一行任务.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 超时超出平台范围时返回 `TimeoutOutOfRange`.
pub fn socket_write_line_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let text = str_arg(args, 1, "SocketWriteLine 的 text")?.to_string();
    let timeout = timeout_arg(args, 2, "SocketWriteLine 的 timeout", span)?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let mut bytes = text.into_bytes();
    bytes.push(b'\n');
    let id = socket.id();
    Ok(net_task(
        Op::Write {
            socket: id,
            bytes,
            all: true,
            timeout,
        },
        Some(host::socket(socket)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_beyond_platform_range_is_rejected() {
        let error = checked_timeout(Duration::MAX, None).expect_err("超时应在平台范围外报错");
        let Error::Runtime(error) = error else {
            panic!("应为语言级运行时错误");
        };
        assert!(matches!(error.kind(), Some(ErrorKind::TimeoutOutOfRange)));
    }
}
