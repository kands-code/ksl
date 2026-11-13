//! 多行编辑缓冲: 若干逻辑行加焦点行.
//!
//! REPL 的多行输入: 段内任意行可编辑, 编辑动作落在焦点行上. 行内编辑
//! 委托给 `Line`; 跨行的拆分与合并在本层完成. `BufferEditor` 与
//! `LineEditor` 共享 `KeyTarget` 键位映射, 只在编辑目标上区分多行缓冲
//! 与 `ReadLine` 的单行场景; 两者共享 `Line` 原语.

use crossterm::event::KeyEvent;

use super::edit::{KeyTarget, LineAction, handle_key};
use super::line::Line;

/// 多行缓冲; 光标即焦点行内的字节偏移, 始终是字素边界.
pub struct Buffer {
    lines: Vec<Line>,
    focus: usize,
}

impl Buffer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            lines: vec![Line::new()],
            focus: 0,
        }
    }

    /// 由多行文本构造; 焦点落在最后一行, 光标在行尾.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let mut lines: Vec<Line> = text
            .split('\n')
            .map(|part| {
                let mut line = Line::new();
                line.insert_str(part);
                line
            })
            .collect();
        if lines.is_empty() {
            lines.push(Line::new());
        }
        let focus = lines.len() - 1;
        Self { lines, focus }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
        self.lines.push(Line::new());
        self.focus = 0;
    }

    /// 整段文本, 行以 `\n` 分隔.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines.iter().map(Line::text).collect::<Vec<_>>().join("\n")
    }

    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    #[must_use]
    pub fn focus(&self) -> usize {
        self.focus
    }

    #[must_use]
    pub fn focus_line(&self) -> &Line {
        &self.lines[self.focus]
    }

    /// 唯一一行且为空; Ctrl-D 以此判定结束输入.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    #[must_use]
    pub fn cursor(&self) -> usize {
        self.lines[self.focus].cursor()
    }

    pub fn insert_char(&mut self, c: char) {
        self.lines[self.focus].insert_char(c);
    }

    /// 在焦点行光标处插入不含换行的文本.
    pub fn insert_str(&mut self, text: &str) {
        self.lines[self.focus].insert_str(text);
    }

    /// 粘贴多行文本: 首段插入焦点行光标处, 中间段成为新行, 末段成为新焦点行.
    pub fn insert_block(&mut self, text: &str) {
        let mut parts = text.split('\n');
        let head = parts.next().unwrap_or("");
        self.lines[self.focus].insert_str(head);
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            return;
        }
        let mut new_lines: Vec<Line> = Vec::with_capacity(rest.len());
        for part in rest {
            let mut line = Line::new();
            line.insert_str(part);
            new_lines.push(line);
        }
        let Some(last) = new_lines.pop() else {
            return;
        };
        let at = self.focus + 1;
        let inserted = new_lines.len();
        self.lines.splice(at..at, new_lines);
        self.focus = at + inserted;
        self.lines.insert(self.focus, last);
    }

    /// 删除焦点行光标前的一个字素; 行首时合并上一行, 光标留在合并点.
    pub fn backspace(&mut self) -> bool {
        if self.lines[self.focus].backspace() {
            return true;
        }
        if self.focus == 0 {
            return false;
        }
        let current = self.lines.remove(self.focus);
        self.focus -= 1;
        let previous = &mut self.lines[self.focus];
        previous.move_end();
        previous.insert_str_keep_cursor(current.text());
        true
    }

    /// 删除焦点行光标处的一个字素; 行尾时合并下一行.
    pub fn delete(&mut self) -> bool {
        let line = &mut self.lines[self.focus];
        if line.cursor() < line.text().len() {
            return line.delete();
        }
        if self.focus + 1 >= self.lines.len() {
            return false;
        }
        let next = self.lines.remove(self.focus + 1);
        let line = &mut self.lines[self.focus];
        line.move_end();
        line.insert_str_keep_cursor(next.text());
        true
    }

    /// 删除末尾的空焦点行; 用于末尾空行上的 Ctrl-D.
    pub fn remove_trailing_focus(&mut self) -> bool {
        if self.lines.len() <= 1 || !self.lines[self.focus].is_empty() {
            return false;
        }
        self.lines.remove(self.focus);
        if self.focus >= self.lines.len() {
            self.focus = self.lines.len() - 1;
            self.lines[self.focus].move_end();
        }
        true
    }

    pub fn move_left(&mut self) -> bool {
        self.lines[self.focus].move_left()
    }

    pub fn move_right(&mut self) -> bool {
        self.lines[self.focus].move_right()
    }

    pub fn move_home(&mut self) {
        self.lines[self.focus].move_home();
    }

    pub fn move_end(&mut self) {
        self.lines[self.focus].move_end();
    }

    pub fn kill_to_end(&mut self) -> String {
        self.lines[self.focus].kill_to_end()
    }

    pub fn kill_to_start(&mut self) -> String {
        self.lines[self.focus].kill_to_start()
    }

    pub fn kill_previous_word(&mut self) -> String {
        self.lines[self.focus].kill_previous_word()
    }

    pub fn yank(&mut self, text: &str) {
        self.lines[self.focus].yank(text);
    }

    /// 在光标处拆行; 下半段成为新焦点行, 光标落在其行首.
    pub fn newline(&mut self) {
        let tail = self.lines[self.focus].kill_to_end();
        let mut next = Line::new();
        next.insert_str(&tail);
        next.move_home();
        self.focus += 1;
        self.lines.insert(self.focus, next);
    }

    /// 焦点上移一行; 光标钳到目标行行尾以内.
    pub fn move_focus_up(&mut self) -> bool {
        if self.focus == 0 {
            return false;
        }
        self.focus -= 1;
        self.clamp_cursor();
        true
    }

    /// 焦点下移一行; 光标钳到目标行行尾以内.
    pub fn move_focus_down(&mut self) -> bool {
        if self.focus + 1 >= self.lines.len() {
            return false;
        }
        self.focus += 1;
        self.clamp_cursor();
        true
    }

    fn clamp_cursor(&mut self) {
        let line = &mut self.lines[self.focus];
        if line.cursor() > line.text().len() {
            line.move_end();
        }
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyTarget for Buffer {
    fn insert_char(&mut self, c: char) {
        Buffer::insert_char(self, c);
    }

    fn backspace(&mut self) {
        Buffer::backspace(self);
    }

    fn delete(&mut self) {
        Buffer::delete(self);
    }

    fn move_left(&mut self) {
        Buffer::move_left(self);
    }

    fn move_right(&mut self) {
        Buffer::move_right(self);
    }

    fn move_home(&mut self) {
        Buffer::move_home(self);
    }

    fn move_end(&mut self) {
        Buffer::move_end(self);
    }

    fn kill_to_end(&mut self) -> String {
        Buffer::kill_to_end(self)
    }

    fn kill_to_start(&mut self) -> String {
        Buffer::kill_to_start(self)
    }

    fn kill_previous_word(&mut self) -> String {
        Buffer::kill_previous_word(self)
    }

    fn yank(&mut self, text: &str) {
        Buffer::yank(self, text);
    }

    fn ctrl_d(&mut self) -> LineAction {
        if self.is_empty() {
            return LineAction::Eof;
        }
        if !self.delete() {
            // 末尾空行: 没有下一行可合并, 改为删除该空行.
            self.remove_trailing_focus();
        }
        LineAction::Continue
    }
}

/// 多行缓冲的按键处理; 与 `LineEditor` 共享键位映射, 只保留多行目标差异.
pub struct BufferEditor {
    buffer: Buffer,
    /// 最近一次杀掉的文本, 供 Ctrl-Y 粘贴.
    kill: String,
}

impl BufferEditor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: Buffer::new(),
            kill: String::new(),
        }
    }

    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffer
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer = Buffer::from_text(text);
    }

    /// 处理一次按键; 调用方负责过滤松开事件.
    ///
    /// 与 `LineEditor` 共享键位映射; 目标差异由 `KeyTarget for Buffer`
    /// 承担: Backspace 在焦点行行首合并上一行, Ctrl-D 在焦点行行尾合并
    /// 下一行或删除末尾空行. 焦点行转移由调用方拦截 Ctrl-P/N 处理.
    pub fn handle_key(&mut self, key: KeyEvent) -> LineAction {
        handle_key(&mut self.buffer, &mut self.kill, key)
    }
}

impl Default for BufferEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Buffer, BufferEditor};
    use crate::terminal::edit::LineAction;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn newline_splits_and_backspace_joins() {
        let mut buffer = Buffer::new();
        buffer.insert_str("ab");
        buffer.move_home();
        buffer.move_right();
        buffer.newline();
        assert_eq!(buffer.text(), "a\nb");
        assert_eq!(buffer.focus(), 1);
        assert!(buffer.backspace());
        assert_eq!(buffer.text(), "ab");
        assert_eq!(buffer.focus(), 0);
        assert_eq!(buffer.cursor(), 1);
        // 首行行首不再合并.
        buffer.move_home();
        assert!(!buffer.backspace());
    }

    #[test]
    fn delete_merges_next_line() {
        let mut buffer = Buffer::from_text("ab\ncd");
        // 焦点在末行; 先移到首行行尾.
        buffer.move_focus_up();
        buffer.move_end();
        assert!(buffer.delete());
        assert_eq!(buffer.text(), "abcd");
        assert_eq!(buffer.cursor(), 2, "合并后光标留在合并点");
        buffer.move_end();
        assert!(!buffer.delete());
    }

    #[test]
    fn insert_block_places_tail_on_new_focus() {
        let mut buffer = Buffer::new();
        buffer.insert_str("x");
        buffer.insert_block("a\nb\n");
        assert_eq!(buffer.text(), "xa\nb\n");
        assert_eq!(buffer.focus(), 2);
        assert!(buffer.focus_line().is_empty());
    }

    #[test]
    fn focus_moves_clamp_cursor() {
        let mut buffer = Buffer::from_text("long line\ns");
        buffer.move_end();
        assert!(buffer.move_focus_up());
        assert_eq!(buffer.cursor(), 9);
        assert!(!buffer.move_focus_up());
        assert!(buffer.move_focus_down());
        assert!(!buffer.move_focus_down());
    }

    #[test]
    fn ctrl_d_removes_trailing_empty_line() {
        let mut editor = BufferEditor::new();
        for c in "ab".chars() {
            editor.handle_key(key(KeyCode::Char(c)));
        }
        editor.handle_key(key(KeyCode::Enter));
        // Enter 只报告 Accept, 拆行由调用方执行.
        editor.buffer_mut().newline();
        assert_eq!(editor.buffer().text(), "ab\n");
        // 拆行后末行为空: Ctrl-D 删除该空行而不是结束输入.
        assert_eq!(editor.handle_key(ctrl('d')), LineAction::Continue);
        assert_eq!(editor.buffer().text(), "ab");
        // 唯一空行上的 Ctrl-D 结束输入.
        editor.clear();
        assert_eq!(editor.handle_key(ctrl('d')), LineAction::Eof);
    }

    #[test]
    fn kill_and_yank_stay_line_local() {
        let mut editor = BufferEditor::new();
        for c in "one two".chars() {
            editor.handle_key(key(KeyCode::Char(c)));
        }
        editor.handle_key(ctrl('w'));
        assert_eq!(editor.buffer().focus_line().text(), "one ");
        editor.handle_key(ctrl('y'));
        assert_eq!(editor.buffer().text(), "one two");
    }
}
