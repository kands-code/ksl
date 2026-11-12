//! 终端行编辑与绘制.
//!
//! REPL 与 `ReadLine` 共用行缓冲, 键位处理, 单行布局与重绘; REPL 在外层
//! 另加历史, 高亮与续行.

mod edit;
mod ink;
mod layout;
mod line;
mod render;

pub use edit::{LineAction, LineEditor};
pub use ink::Ink;
pub use layout::{LineLayout, layout};
pub use line::Line;
pub use render::{RenderState, clear, draw, finish};
