//! 套接字句柄.
//!
//! 语言层只持有令牌与展示信息; 实际的非阻塞套接字由网络服务线程独占.
//! 句柄被最后一个 `Arc` 释放时自动提交关闭命令.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::service;

/// 已连接字节流的语言层句柄.
pub struct SocketHandle {
    token: usize,
    peer: Arc<str>,
    closed: AtomicBool,
}

impl SocketHandle {
    pub(crate) fn new(token: usize, peer: String) -> Self {
        Self {
            token,
            peer: Arc::from(peer),
            closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn token(&self) -> usize {
        self.token
    }

    /// 对端地址文本; 连接建立后固定.
    pub(crate) fn peer(&self) -> &str {
        &self.peer
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// 幂等关闭.
    pub(crate) fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            service::close(self.token);
        }
    }
}

impl Drop for SocketHandle {
    fn drop(&mut self) {
        self.close();
    }
}

impl fmt::Debug for SocketHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<socket>")
    }
}

/// TCP 监听器的语言层句柄.
pub struct TcpListenerHandle {
    token: usize,
    local_port: u16,
    closed: AtomicBool,
}

impl TcpListenerHandle {
    pub(crate) fn new(token: usize, local_port: u16) -> Self {
        Self {
            token,
            local_port,
            closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn token(&self) -> usize {
        self.token
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.local_port
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            service::close(self.token);
        }
    }
}

impl Drop for TcpListenerHandle {
    fn drop(&mut self) {
        self.close();
    }
}

impl fmt::Debug for TcpListenerHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<tcp-listener>")
    }
}
