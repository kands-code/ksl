//! 当前输入区的终端重绘.
//!
//! 每次按键全量重绘当前输入区: 回到输入区第一行, 清到屏幕末尾, 打印视觉行,
//! 再把光标放回目标位置.

use std::io::{self, Write};

use crossterm::cursor::{MoveDown, MoveToColumn, MoveUp};
use crossterm::queue;
use crossterm::terminal::{Clear, ClearType};

use super::layout::LineLayout;

/// 上一次绘制后光标在输入区内的行号与总行数.
pub struct RenderState {
    cursor_row: usize,
    rows: usize,
}

impl RenderState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cursor_row: 0,
            rows: 0,
        }
    }

    pub fn reset(&mut self) {
        self.cursor_row = 0;
        self.rows = 0;
    }
}

impl Default for RenderState {
    fn default() -> Self {
        Self::new()
    }
}

/// 重绘当前输入区.
///
/// # Errors
///
/// 写出终端控制序列失败时返回底层错误.
pub fn draw(
    out: &mut impl Write,
    state: &mut RenderState,
    layout: &LineLayout,
) -> io::Result<()> {
    if state.rows > 0 {
        // `CSI 0 A` 在多数终端上等于上移一行, 而不是原地不动; 只有实际需要时才发送.
        if state.cursor_row > 0 {
            queue!(out, MoveUp(clamp(state.cursor_row)))?;
        }
        queue!(out, MoveToColumn(0))?;
    }
    queue!(out, Clear(ClearType::FromCursorDown))?;
    for (index, row) in layout.rows.iter().enumerate() {
        for segment in row {
            write!(out, "{}", segment.ink.paint(&segment.text))?;
        }
        if index + 1 < layout.rows.len() {
            write!(out, "\r\n")?;
        }
    }
    let up = layout.rows.len().saturating_sub(1 + layout.cursor_row);
    if up > 0 {
        queue!(out, MoveUp(clamp(up)))?;
    }
    queue!(out, MoveToColumn(clamp(layout.cursor_col)))?;
    out.flush()?;
    state.cursor_row = layout.cursor_row;
    state.rows = layout.rows.len();
    Ok(())
}

/// 把光标移到输入末尾并换行, 接受或提交当前行时使用.
///
/// # Errors
///
/// 写出终端控制序列失败时返回底层错误.
pub fn finish(
    out: &mut impl Write,
    state: &mut RenderState,
    layout: &LineLayout,
) -> io::Result<()> {
    let down = layout.rows.len().saturating_sub(1 + layout.cursor_row);
    queue!(out, MoveToColumn(0))?;
    if down > 0 {
        queue!(out, MoveDown(clamp(down)))?;
    }
    queue!(out, MoveToColumn(clamp(layout.end_col())))?;
    write!(out, "\r\n")?;
    out.flush()?;
    state.reset();
    Ok(())
}

/// 清除当前输入区, 回到它的第一行行首.
///
/// # Errors
///
/// 写出终端控制序列失败时返回底层错误.
pub fn clear(out: &mut impl Write, state: &mut RenderState) -> io::Result<()> {
    if state.rows > 0 {
        if state.cursor_row > 0 {
            queue!(out, MoveUp(clamp(state.cursor_row)))?;
        }
        queue!(out, MoveToColumn(0), Clear(ClearType::FromCursorDown))?;
        out.flush()?;
        state.reset();
    }
    Ok(())
}

fn clamp(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::super::ink::Ink;
    use super::super::layout::layout;
    use super::*;

    fn output(run: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut buffer = Vec::new();
        run(&mut buffer);
        String::from_utf8(buffer).expect("输出应为 UTF-8")
    }

    #[test]
    fn zero_move_is_not_emitted() {
        let layout = layout("1> ", "", 0, 80, |_| Ink::Plain);
        let mut state = RenderState::new();
        let text = output(|out| draw(out, &mut state, &layout).expect("首次绘制应成功"));
        assert!(!text.contains("\x1b[0A"));
        assert!(!text.contains("\x1b[0B"));

        let text = output(|out| draw(out, &mut state, &layout).expect("重绘应成功"));
        assert!(!text.contains("\x1b[0A"));
        assert!(!text.contains("\x1b[0B"));
    }

    #[test]
    fn clear_at_first_row_does_not_move_up() {
        let mut state = RenderState::new();
        state.rows = 1;
        let text = output(|out| clear(out, &mut state).expect("清除应成功"));
        assert!(!text.contains("\x1b[0A"));
        assert_eq!(state.rows, 0);
    }

    #[test]
    fn multi_row_redraw_moves_by_actual_rows() {
        let layout = layout("1> ", "b", 1, 3, |_| Ink::Plain);
        assert_eq!(layout.rows.len(), 2);
        let mut state = RenderState::new();
        output(|out| draw(out, &mut state, &layout).expect("首次绘制应成功"));
        let text = output(|out| draw(out, &mut state, &layout).expect("重绘应成功"));
        assert!(text.contains("\x1b[1A"));
        assert!(!text.contains("\x1b[0A"));
    }
}
