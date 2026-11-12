//! 终端行编辑与绘制.
//!
//! REPL 与 `ReadLine` 共用行缓冲, 键位处理, 单行布局与重绘.
//! REPL 在外层另加历史, 高亮与续行.

/// 终端报告的宽或高; 为 0 时使用回退值.
#[must_use]
pub fn dimension(value: u16, fallback: usize) -> usize {
    if value == 0 {
        fallback
    } else {
        usize::from(value)
    }
}

mod buffer;
mod edit;
mod ink;
mod layout;
mod line;
mod render;
pub mod session;
pub mod state;
pub(crate) mod sys;

pub use buffer::{Buffer, BufferEditor};
pub use edit::{LineAction, LineEditor};
pub use ink::Ink;
pub use layout::{LineLayout, compose, layout};
pub use line::Line;
pub use render::{RenderState, clear, draw, finish};
pub use session::Session;
pub(crate) use sys::TerminalHandle;
pub use sys::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, WindowSize};
