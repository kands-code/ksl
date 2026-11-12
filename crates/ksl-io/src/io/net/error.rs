//! 套接字错误.
//!
//! 网络操作的可恢复失败统一表示为 `SocketError` Data 值.
//! 只有运行时内部故障才使用 `RuntimeError`.

use std::io;

use ksl_base::name::Name;
use ksl_value::value::{Value, data_value, data_value0};

/// 套接字操作错误.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SocketError {
    TimedOut,
    InvalidArgument(String),
    ConnectionRefused,
    ConnectionReset,
    ConnectionClosed,
    Closed,
    AddrInUse,
    AddrNotAvailable,
    PermissionDenied,
    InvalidUtf8,
    LineTooLong,
    TooLarge,
    UnexpectedEof,
    Other(String),
}

impl SocketError {
    /// 转为语言层 `SocketError` Data 值.
    pub fn to_value(&self) -> Value {
        let ty = Name::new("SocketError");
        match self {
            Self::TimedOut => data_value0(ty, Name::new("TimedOut")),
            Self::InvalidArgument(message) => data_value(
                ty,
                Name::new("InvalidArgument"),
                [Value::Str(message.as_str().into())],
            ),
            Self::ConnectionRefused => data_value0(ty, Name::new("ConnectionRefused")),
            Self::ConnectionReset => data_value0(ty, Name::new("ConnectionReset")),
            Self::ConnectionClosed => data_value0(ty, Name::new("ConnectionClosed")),
            Self::Closed => data_value0(ty, Name::new("Closed")),
            Self::AddrInUse => data_value0(ty, Name::new("AddrInUse")),
            Self::AddrNotAvailable => data_value0(ty, Name::new("AddrNotAvailable")),
            Self::PermissionDenied => data_value0(ty, Name::new("PermissionDenied")),
            Self::InvalidUtf8 => data_value0(ty, Name::new("InvalidUtf8")),
            Self::LineTooLong => data_value0(ty, Name::new("LineTooLong")),
            Self::TooLarge => data_value0(ty, Name::new("TooLarge")),
            Self::UnexpectedEof => data_value0(ty, Name::new("UnexpectedEof")),
            Self::Other(message) => {
                data_value(ty, Name::new("Other"), [Value::Str(message.as_str().into())])
            },
        }
    }
}

/// 把宿主 I/O 错误映射为语言层套接字错误.
pub fn from_io(error: &io::Error) -> SocketError {
    match error.kind() {
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => SocketError::TimedOut,
        io::ErrorKind::ConnectionRefused => SocketError::ConnectionRefused,
        io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted => {
            SocketError::ConnectionReset
        },
        io::ErrorKind::BrokenPipe | io::ErrorKind::NotConnected => {
            SocketError::ConnectionClosed
        },
        io::ErrorKind::AddrInUse => SocketError::AddrInUse,
        io::ErrorKind::AddrNotAvailable => SocketError::AddrNotAvailable,
        io::ErrorKind::PermissionDenied => SocketError::PermissionDenied,
        io::ErrorKind::UnexpectedEof => SocketError::UnexpectedEof,
        io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => {
            SocketError::InvalidArgument(error.to_string())
        },
        _ => SocketError::Other(error.to_string()),
    }
}
