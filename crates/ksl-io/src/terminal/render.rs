//! 当前输入区的终端重绘.
//!
//! 每次按键全量重绘当前输入区: 回到输入区第一行, 清到屏幕末尾, 打印视觉行,
//! 再把光标放回目标位置.

use std::io::{self, Write};

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
        // `CSI 0 A` 在多数终端上会上移一行, 只在需要时发送.
        if state.cursor_row > 0 {
            move_up(out, state.cursor_row)?;
        }
        move_to_column(out, 0)?;
    }
    out.write_all(b"\x1b[0J")?;
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
        move_up(out, up)?;
    }
    move_to_column(out, layout.cursor_col)?;
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
    move_to_column(out, 0)?;
    if down > 0 {
        move_down(out, down)?;
    }
    move_to_column(out, layout.end_col())?;
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
            move_up(out, state.cursor_row)?;
        }
        move_to_column(out, 0)?;
        out.write_all(b"\x1b[0J")?;
        out.flush()?;
        state.reset();
    }
    Ok(())
}

fn clamp(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn move_up(out: &mut impl Write, rows: usize) -> io::Result<()> {
    write!(out, "\x1b[{}A", clamp(rows))
}

fn move_down(out: &mut impl Write, rows: usize) -> io::Result<()> {
    write!(out, "\x1b[{}B", clamp(rows))
}

fn move_to_column(out: &mut impl Write, column: usize) -> io::Result<()> {
    // CHA 列号从 1 开始, 布局列号从 0 开始.
    write!(out, "\x1b[{}G", clamp(column.saturating_add(1)))
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
    fn cursor_column_is_one_based() {
        let line = layout("1> ", "abc", 3, 80, |_| Ink::Plain);
        assert_eq!(line.cursor_col, 6);

        let mut state = RenderState::new();
        let text = output(|out| draw(out, &mut state, &line).expect("绘制应成功"));
        assert!(text.contains("\x1b[7G"));
        assert!(!text.contains("\x1b[6G"));

        let text = output(|out| finish(out, &mut state, &line).expect("提交应成功"));
        assert!(text.contains("\x1b[7G"));
        assert!(!text.contains("\x1b[0G"));
    }

    #[test]
    fn redraw_moves_by_actual_rows() {
        // 单行重绘不发出零行移动.
        let single = layout("1> ", "", 0, 80, |_| Ink::Plain);
        let mut state = RenderState::new();
        let text = output(|out| draw(out, &mut state, &single).expect("首次绘制应成功"));
        assert!(!text.contains("\x1b[0A"));
        assert!(!text.contains("\x1b[0B"));

        let text = output(|out| draw(out, &mut state, &single).expect("重绘应成功"));
        assert!(!text.contains("\x1b[0A"));
        assert!(!text.contains("\x1b[0B"));

        // 首行清除同样不向上移动.
        let mut state = RenderState::new();
        state.rows = 1;
        let text = output(|out| clear(out, &mut state).expect("清除应成功"));
        assert!(!text.contains("\x1b[0A"));
        assert_eq!(state.rows, 0);

        // 多行重绘按实际行数移动.
        let multi = layout("1> ", "b", 1, 3, |_| Ink::Plain);
        assert_eq!(multi.rows.len(), 2);
        let mut state = RenderState::new();
        output(|out| draw(out, &mut state, &multi).expect("首次绘制应成功"));
        let text = output(|out| draw(out, &mut state, &multi).expect("重绘应成功"));
        assert!(text.contains("\x1b[1A"));
        assert!(!text.contains("\x1b[0A"));
    }
}
