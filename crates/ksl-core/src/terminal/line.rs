//! 单行缓冲区; 光标始终落在字素边界.
//!
//! 不保存多行: 续行由外层追加到固定前缀. 这样光标位置与重绘都只需
//! 考虑当前一条逻辑行.

use unicode_segmentation::UnicodeSegmentation;

pub struct Line {
    text: String,
    /// 字节偏移, 始终是字素边界.
    cursor: usize,
}

impl Line {
    #[must_use]
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
        }
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.text.len();
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor = next_boundary(&self.text, self.cursor);
    }

    pub fn insert_str(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.cursor = next_boundary(&self.text, self.cursor);
    }

    /// 在光标处插入文本但光标保持在插入点; 合并行时使用, 光标留在合并点.
    pub fn insert_str_keep_cursor(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
    }

    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        let start = previous_boundary(&self.text, self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        true
    }

    pub fn delete(&mut self) -> bool {
        if self.cursor >= self.text.len() {
            return false;
        }
        let end = next_boundary(&self.text, self.cursor);
        self.text.replace_range(self.cursor..end, "");
        true
    }

    pub fn move_left(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor = previous_boundary(&self.text, self.cursor);
        true
    }

    pub fn move_right(&mut self) -> bool {
        if self.cursor >= self.text.len() {
            return false;
        }
        self.cursor = next_boundary(&self.text, self.cursor);
        true
    }

    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn kill_to_end(&mut self) -> String {
        let out = self.text[self.cursor..].to_string();
        self.text.truncate(self.cursor);
        out
    }

    pub fn kill_to_start(&mut self) -> String {
        let out = self.text[..self.cursor].to_string();
        self.text.drain(..self.cursor);
        self.cursor = 0;
        out
    }

    /// 删除光标前一个单词, 连同其前面的空白.
    pub fn kill_previous_word(&mut self) -> String {
        let mut start = self.cursor;
        let mut chars = self.text[..start].char_indices().rev().peekable();
        while let Some((index, c)) = chars.peek().copied() {
            if c.is_whitespace() {
                start = index;
                chars.next();
            } else {
                break;
            }
        }
        while let Some((index, c)) = chars.peek().copied() {
            if is_word_char(c) {
                start = index;
                chars.next();
            } else {
                break;
            }
        }
        let out = self.text[start..self.cursor].to_string();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        out
    }

    pub fn yank(&mut self, text: &str) {
        if !text.is_empty() {
            self.insert_str(text);
        }
    }
}

impl Default for Line {
    fn default() -> Self {
        Self::new()
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor..]
        .graphemes(true)
        .next()
        .map_or(text.len(), |grapheme| cursor + grapheme.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_by_grapheme() {
        // ASCII 与宽字符都按字素边界编辑.
        let mut line = Line::new();
        line.insert_str("abc");
        assert_eq!(line.cursor(), 3);
        line.backspace();
        assert_eq!(line.text(), "ab");
        line.move_left();
        line.delete();
        assert_eq!(line.text(), "a");

        let mut line = Line::new();
        line.insert_str("中");
        assert_eq!(line.cursor(), 3);
        line.backspace();
        assert_eq!(line.text(), "");

        let mut line = Line::new();
        line.insert_str("e\u{301}");
        assert_eq!(line.cursor(), 3);
        line.backspace();
        assert_eq!(line.text(), "");
    }

    #[test]
    fn kills_and_yanks() {
        let mut line = Line::new();
        line.insert_str("one two");
        line.move_home();
        line.move_right();
        line.move_right();
        line.move_right();
        let killed = line.kill_to_end();
        assert_eq!(killed, " two");
        assert_eq!(line.text(), "one");
        line.yank(&killed);
        assert_eq!(line.text(), "one two");

        // 删除前一个单词时连带其前面的空白.
        let mut line = Line::new();
        line.insert_str("one two  ");
        let killed = line.kill_previous_word();
        assert_eq!(killed, "two  ");
        assert_eq!(line.text(), "one ");
    }
}
