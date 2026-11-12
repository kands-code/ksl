//! 终端行编辑与绘制.
//!
//! REPL 与 `ReadLine` 共用行缓冲, 键位处理, 单行布局与重绘.
//! REPL 在外层另加历史, 高亮与续行.

/// 终端尺寸; 查询失败或某一维为 0 时使用对应的回退值.
#[must_use]
pub fn size_or(fallback_width: usize, fallback_height: usize) -> (usize, usize) {
    crossterm::terminal::size().map_or((fallback_width, fallback_height), |(width, height)| {
        (
            dimension(width, fallback_width),
            dimension(height, fallback_height),
        )
    })
}

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
pub(crate) mod session;
pub(crate) mod state;

pub use buffer::{Buffer, BufferEditor};
pub use edit::{LineAction, LineEditor};
pub use ink::Ink;
pub use layout::{LineLayout, compose, layout};
pub use line::Line;
pub use render::{RenderState, clear, draw, finish};
pub use session::Session;
