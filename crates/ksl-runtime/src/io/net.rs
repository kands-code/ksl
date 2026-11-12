//! 套接字原语与任务构造.
//!
//! 语言层只暴露不透明句柄. 参数校验在此完成, 实际的非阻塞 I/O 由网络引擎执行.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use engine::{Op, ReadMode};
use error::SocketError;
pub use handle::{SocketHandle, TcpListenerHandle};
use ksl_base::source::Span;
use ksl_frontend::error::RuntimeError as Error;

use super::Task;
use crate::value::{List, Value, result_err};

pub(crate) mod engine;
mod error;
mod handle;

fn closed() -> Value {
    result_err(SocketError::Closed.to_value())
}

fn invalid(message: impl Into<String>) -> Value {
    result_err(SocketError::InvalidArgument(message.into()).to_value())
}

fn string_arg(args: &[Value], index: usize, context: &str) -> Result<String, Error> {
    match args.get(index) {
        Some(Value::Str(text)) => Ok(text.to_string()),
        _ => Err(Error::internal(format!("{context} 不是 string"))),
    }
}

fn u64_arg(args: &[Value], index: usize, context: &str) -> Result<u64, Error> {
    match args.get(index) {
        Some(Value::U64(value)) => Ok(*value),
        _ => Err(Error::internal(format!("{context} 不是 u64"))),
    }
}

fn timeout_arg(args: &[Value], index: usize, context: &str) -> Result<Duration, Error> {
    Ok(Duration::from_millis(u64_arg(args, index, context)?))
}

fn socket_arg(args: &[Value], index: usize) -> Result<Arc<SocketHandle>, Error> {
    match args.get(index) {
        Some(Value::Socket(socket)) => Ok(Arc::clone(socket)),
        _ => Err(Error::internal("套接字实参类型不符")),
    }
}

fn listener_arg(args: &[Value], index: usize) -> Result<Arc<TcpListenerHandle>, Error> {
    match args.get(index) {
        Some(Value::TcpListener(listener)) => Ok(Arc::clone(listener)),
        _ => Err(Error::internal("监听器实参类型不符")),
    }
}

fn bytes_arg(args: &[Value], index: usize) -> Result<Vec<u8>, Error> {
    let Some(Value::List(list)) = args.get(index) else {
        return Err(Error::internal("字节实参不是列表"));
    };
    match &**list {
        List::U8(items) => Ok(items.iter().copied().collect()),
        List::Values(items) => items
            .iter()
            .map(|value| match value {
                Value::U8(byte) => Ok(*byte),
                _ => Err(Error::internal("字节列表包含非 u8 元素")),
            })
            .collect(),
        _ => Err(Error::internal("字节列表不是 u8 存储")),
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

pub(crate) fn tcp_listen_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let host = string_arg(args, 0, "TcpListen 的 host")?;
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

pub(crate) fn tcp_accept_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let listener = listener_arg(args, 0)?;
    let timeout = timeout_arg(args, 1, "TcpAccept 的 timeout")?;
    if listener.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let id = listener.id();
    Ok(net_task(
        Op::Accept {
            listener: id,
            timeout,
        },
        Some(Value::TcpListener(listener)),
    ))
}

pub(crate) fn tcp_connect_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let host = string_arg(args, 0, "TcpConnect 的 host")?;
    let port = u64_arg(args, 1, "TcpConnect 的 port")?;
    let timeout = timeout_arg(args, 2, "TcpConnect 的 timeout")?;
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

pub(crate) fn tcp_close_listener_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let listener = listener_arg(args, 0)?;
    listener.close();
    Ok(Task::Ready(Value::Nil))
}

pub(crate) fn tcp_local_port_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let listener = listener_arg(args, 0)?;
    Ok(Task::Ready(Value::U64(u64::from(listener.local_port()))))
}

pub(crate) fn socket_peer_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    Ok(Task::Ready(Value::Str(socket.peer())))
}

pub(crate) fn socket_close_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    socket.close();
    Ok(Task::Ready(Value::Nil))
}

pub(crate) fn socket_is_closed_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    Ok(Task::Ready(Value::Bool(socket.is_closed())))
}

pub(crate) fn socket_shutdown_write_task(
    args: &[Value],
    _: Option<Span>,
) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let timeout = timeout_arg(args, 1, "SocketShutdownWrite 的 timeout")?;
    if socket.is_closed() {
        return Ok(Task::Ready(closed()));
    }
    let id = socket.id();
    Ok(net_task(
        Op::ShutdownWrite {
            socket: id,
            timeout,
        },
        Some(Value::Socket(socket)),
    ))
}

pub(crate) fn socket_read_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let max = u64_arg(args, 1, "SocketRead 的 max")?;
    let timeout = timeout_arg(args, 2, "SocketRead 的 timeout")?;
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
        Some(Value::Socket(socket)),
    ))
}

pub(crate) fn socket_read_exact_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let count = u64_arg(args, 1, "SocketReadExact 的 count")?;
    let timeout = timeout_arg(args, 2, "SocketReadExact 的 timeout")?;
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
        Some(Value::Socket(socket)),
    ))
}

pub(crate) fn socket_read_all_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let max = u64_arg(args, 1, "SocketReadAll 的 max")?;
    let timeout = timeout_arg(args, 2, "SocketReadAll 的 timeout")?;
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
        Some(Value::Socket(socket)),
    ))
}

pub(crate) fn socket_read_line_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let max = u64_arg(args, 1, "SocketReadLine 的 max")?;
    let timeout = timeout_arg(args, 2, "SocketReadLine 的 timeout")?;
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
        Some(Value::Socket(socket)),
    ))
}

pub(crate) fn socket_write_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let bytes = bytes_arg(args, 1)?;
    let timeout = timeout_arg(args, 2, "SocketWrite 的 timeout")?;
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
        Some(Value::Socket(socket)),
    ))
}

pub(crate) fn socket_write_all_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let bytes = bytes_arg(args, 1)?;
    let timeout = timeout_arg(args, 2, "SocketWriteAll 的 timeout")?;
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
        Some(Value::Socket(socket)),
    ))
}

pub(crate) fn socket_write_line_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let socket = socket_arg(args, 0)?;
    let text = string_arg(args, 1, "SocketWriteLine 的 text")?;
    let timeout = timeout_arg(args, 2, "SocketWriteLine 的 timeout")?;
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
        Some(Value::Socket(socket)),
    ))
}
