//! 视觉行布局.
//!
//! `layout` 只处理一条逻辑行: 断行由本模块决定, 渲染时显式写出换行,
//! 因此不依赖终端的自动折行; 光标位置只由 `(行, 列)` 决定. 多行输入
//! 由 `compose` 把多条逻辑行的布局拼为一份, 并在超出视口高度时保留
//! 末尾窗口且保证焦点行可见.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::ink::Ink;

/// 视觉行内的一段同色文本.
pub(crate) struct Segment {
    pub(crate) text: String,
    pub(crate) ink: Ink,
    pub(crate) width: usize,
}

/// 当前行的视觉布局.
pub struct LineLayout {
    pub(crate) rows: Vec<Vec<Segment>>,
    pub(crate) cursor_row: usize,
    pub(crate) cursor_col: usize,
}

impl LineLayout {
    /// 最后一条视觉行的显示宽度, 用于把光标移到行尾.
    pub(crate) fn end_col(&self) -> usize {
        self.rows
            .last()
            .map_or(0, |row| row.iter().map(|segment| segment.width).sum())
    }
}

/// 计算提示符与当前行的视觉布局.
///
/// `width` 是终端列数, 至少按 1 处理. `ink_at` 接收当前行内的字节偏移,
/// 返回该位置的颜色; 调用方负责保证偏移落在字素起点.
pub fn layout(
    prompt: &str,
    line: &str,
    cursor: usize,
    width: usize,
    ink_at: impl Fn(usize) -> Ink,
) -> LineLayout {
    let width = width.max(1);
    let mut rows: Vec<Vec<Segment>> = vec![Vec::new()];
    let mut cursor_pos = None;

    let prompt_width = UnicodeWidthStr::width(prompt);
    push_segment(&mut rows[0], prompt.to_string(), Ink::Plain, prompt_width);
    let mut col = prompt_width;
    if col >= width {
        rows.push(Vec::new());
        col = 0;
    }

    for (offset, grapheme) in line.grapheme_indices(true) {
        let (text, grapheme_width) = if grapheme == "\t" {
            let spaces = 4 - (col % 4);
            if col > 0 && col + spaces > width {
                rows.push(Vec::new());
                col = 0;
                (" ".repeat(4), 4)
            } else {
                (" ".repeat(spaces), spaces)
            }
        } else {
            (grapheme.to_string(), UnicodeWidthStr::width(grapheme))
        };
        if grapheme_width > 0 && col + grapheme_width > width {
            rows.push(Vec::new());
            col = 0;
        }
        if offset == cursor {
            cursor_pos = Some((rows.len() - 1, col));
        }
        let ink = ink_at(offset);
        if let Some(row) = rows.last_mut() {
            push_segment(row, text, ink, grapheme_width);
        }
        col += grapheme_width;
    }

    if cursor_pos.is_none() {
        if col >= width {
            rows.push(Vec::new());
            col = 0;
        }
        cursor_pos = Some((rows.len() - 1, col));
    }
    let (cursor_row, cursor_col) = cursor_pos.unwrap_or((0, 0));
    LineLayout {
        rows,
        cursor_row,
        cursor_col,
    }
}

fn push_segment(row: &mut Vec<Segment>, text: String, ink: Ink, width: usize) {
    if let Some(last) = row.last_mut()
        && last.ink == ink
    {
        last.text.push_str(&text);
        last.width += width;
        return;
    }
    row.push(Segment { text, ink, width });
}

/// 把多条逻辑行的布局拼为一份, 并对超出视口高度的情况保留末尾窗口.
///
/// `parts` 按缓冲行序排列, `focus` 是焦点行的下标; 超出 `max_rows` 时
/// 丢弃开头多余的视觉行, 焦点行始终保留在窗口内. 光标列取焦点行的列.
#[must_use]
pub fn compose(parts: Vec<LineLayout>, focus: usize, max_rows: usize) -> LineLayout {
    let mut rows = Vec::new();
    let mut cursor_row = 0;
    let mut cursor_col = 0;
    for (index, part) in parts.into_iter().enumerate() {
        if index == focus {
            cursor_row = rows.len() + part.cursor_row;
            cursor_col = part.cursor_col;
        }
        rows.extend(part.rows);
    }
    let max_rows = max_rows.max(1);
    if rows.len() > max_rows {
        let tail_top = rows.len() - max_rows;
        let top = cursor_row.saturating_sub(max_rows - 1).min(tail_top);
        // 头部锚定 (top 为 0 且焦点不在末尾窗口) 时需要裁掉末尾多余的行.
        let mut window = rows.split_off(top);
        window.truncate(max_rows);
        rows = window;
        cursor_row -= top;
    }
    LineLayout {
        rows,
        cursor_row,
        cursor_col,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(_: usize) -> Ink {
        Ink::Plain
    }

    #[test]
    fn cursor_tracks_prompt_and_ascii() {
        let layout = layout("1> ", "abc", 3, 80, plain);
        assert_eq!(layout.rows.len(), 1);
        assert_eq!((layout.cursor_row, layout.cursor_col), (0, 6));
    }

    #[test]
    fn grapheme_width_controls_cursor_and_wrap() {
        // 宽字符按两列计算, 越过终端右边缘时整字换行.
        let wrapped = layout("1> ", "中", 3, 4, plain);
        assert_eq!(wrapped.rows.len(), 2);
        assert_eq!((wrapped.cursor_row, wrapped.cursor_col), (1, 2));

        // 组合附加符与基字符合为一个字素, 只占一列.
        let line = "e\u{301}";
        let combined = layout("1> ", line, line.len(), 80, plain);
        assert_eq!(combined.rows.len(), 1);
        assert_eq!((combined.cursor_row, combined.cursor_col), (0, 4));
    }

    #[test]
    fn cursor_stays_before_wrapped_grapheme() {
        let layout = layout("1> ", "ab中", 2, 5, plain);
        assert_eq!((layout.cursor_row, layout.cursor_col), (1, 0));
    }

    #[test]
    fn tab_expands_to_next_stop() {
        let layout = layout("1> ", "\t", 1, 80, plain);
        assert_eq!((layout.cursor_row, layout.cursor_col), (0, 4));
    }

    #[test]
    fn compose_flattens_and_keeps_focus_visible() {
        let first = layout("1> ", "ab", 2, 80, plain);
        let second = layout("1| ", "cd", 1, 80, plain);
        let joined = compose(vec![first, second], 1, 80);
        assert_eq!(joined.rows.len(), 2);
        assert_eq!((joined.cursor_row, joined.cursor_col), (1, 4));

        // 超出视口时保留末尾窗口, 焦点在末尾则窗口对齐末尾.
        let many: Vec<LineLayout> = (0..5).map(|_| layout("1| ", "x", 1, 80, plain)).collect();
        let windowed = compose(many, 4, 3);
        assert_eq!(windowed.rows.len(), 3);
        assert_eq!(windowed.cursor_row, 2);

        // 焦点在开头时窗口跟随焦点.
        let many: Vec<LineLayout> = (0..5).map(|_| layout("1| ", "x", 1, 80, plain)).collect();
        let windowed = compose(many, 0, 3);
        assert_eq!(windowed.rows.len(), 3);
        assert_eq!(windowed.cursor_row, 0);
    }
}
