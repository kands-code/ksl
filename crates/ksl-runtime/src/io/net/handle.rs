//! 套接字句柄.
//!
//! 语言层只持有身份与共享关闭标志; 字节流由网络引擎持有.
//! 句柄关闭时置位标志并通知引擎, 由引擎唤醒等待中的操作.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arcstr::ArcStr;

use super::engine::EngineHandle;

/// 已连接字节流的语言层句柄.
pub struct SocketHandle {
    engine: EngineHandle,
    id: u64,
    peer: ArcStr,
    closed: Arc<AtomicBool>,
}

impl SocketHandle {
    pub(super) fn new(
        engine: EngineHandle,
        id: u64,
        peer: String,
        closed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            engine,
            id,
            peer: ArcStr::from(peer),
            closed,
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn peer(&self) -> ArcStr {
        self.peer.clone()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.engine.close_socket(self.id);
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
    engine: EngineHandle,
    id: u64,
    port: u16,
    closed: Arc<AtomicBool>,
}

impl TcpListenerHandle {
    pub(super) fn new(
        engine: EngineHandle,
        id: u64,
        port: u16,
        closed: Arc<AtomicBool>,
    ) -> Self {
        Self {
            engine,
            id,
            port,
            closed,
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn local_port(&self) -> u16 {
        self.port
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub(crate) fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            self.engine.close_listener(self.id);
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
