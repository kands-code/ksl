//! 单行编辑的按键处理.
//!
//! 只处理行缓冲的编辑动作; 历史, 高亮, 多行与粘贴由调用方在外层处理.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::line::Line;

/// 一次按键对行编辑的处置.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineAction {
    /// 行内容已更新, 调用方重绘.
    Continue,
    /// 接受当前行.
    Accept,
    /// 用户中断, 通常是 Ctrl-C.
    Interrupt,
    /// 空行上的 Ctrl-D.
    Eof,
    /// 清屏并重绘, 通常是 Ctrl-L.
    Clear,
}

/// 公共键位处理所需的编辑目标.
///
/// 只包含键位映射真正会用到的操作; 多行拆分, 焦点转移, 历史与高亮
/// 仍由 `Buffer` 或 REPL 外层处理. `Line` 与 `Buffer` 通过实现本 trait
/// 共享同一份键位映射.
pub(crate) trait KeyTarget {
    fn insert_char(&mut self, c: char);
    fn backspace(&mut self);
    fn delete(&mut self);
    fn move_left(&mut self);
    fn move_right(&mut self);
    fn move_home(&mut self);
    fn move_end(&mut self);
    fn kill_to_end(&mut self) -> String;
    fn kill_to_start(&mut self) -> String;
    fn kill_previous_word(&mut self) -> String;
    fn yank(&mut self, text: &str);
    fn ctrl_d(&mut self) -> LineAction;
}

/// 公共键位映射; `LineEditor` 与 `BufferEditor` 只在目标差异上不同.
pub(crate) fn handle_key<T: KeyTarget>(
    target: &mut T,
    kill: &mut String,
    key: KeyEvent,
) -> LineAction {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Enter => return LineAction::Accept,
        KeyCode::Backspace => target.backspace(),
        KeyCode::Delete => target.delete(),
        KeyCode::Left => target.move_left(),
        KeyCode::Right => target.move_right(),
        KeyCode::Home => target.move_home(),
        KeyCode::End => target.move_end(),
        KeyCode::Char('a') if ctrl => target.move_home(),
        KeyCode::Char('e') if ctrl => target.move_end(),
        KeyCode::Char('b') if ctrl => target.move_left(),
        KeyCode::Char('f') if ctrl => target.move_right(),
        KeyCode::Char('k') if ctrl => *kill = target.kill_to_end(),
        KeyCode::Char('u') if ctrl => *kill = target.kill_to_start(),
        KeyCode::Char('w') if ctrl => *kill = target.kill_previous_word(),
        KeyCode::Char('y') if ctrl => {
            let text = kill.clone();
            target.yank(&text);
        },
        KeyCode::Char('d') if ctrl => return target.ctrl_d(),
        KeyCode::Char('c') if ctrl => return LineAction::Interrupt,
        KeyCode::Char('l') if ctrl => return LineAction::Clear,
        KeyCode::Char('h') if ctrl => target.backspace(),
        KeyCode::Char('j' | 'm') if ctrl => return LineAction::Accept,
        KeyCode::Char(_) if ctrl || alt => return LineAction::Continue,
        KeyCode::Char(c) => target.insert_char(c),
        // Tab 与其余未列出按键不处理: Tab 不插入空白, 语言没有缩进语义,
        // 缩进约定为两个空格, 且没有补全. 粘贴内容中的 tab 仍按文本处理,
        // 绘制时展开为制表位.
        _ => {},
    }
    LineAction::Continue
}

impl KeyTarget for Line {
    fn insert_char(&mut self, c: char) {
        Line::insert_char(self, c);
    }

    fn backspace(&mut self) {
        Line::backspace(self);
    }

    fn delete(&mut self) {
        Line::delete(self);
    }

    fn move_left(&mut self) {
        Line::move_left(self);
    }

    fn move_right(&mut self) {
        Line::move_right(self);
    }

    fn move_home(&mut self) {
        Line::move_home(self);
    }

    fn move_end(&mut self) {
        Line::move_end(self);
    }

    fn kill_to_end(&mut self) -> String {
        Line::kill_to_end(self)
    }

    fn kill_to_start(&mut self) -> String {
        Line::kill_to_start(self)
    }

    fn kill_previous_word(&mut self) -> String {
        Line::kill_previous_word(self)
    }

    fn yank(&mut self, text: &str) {
        Line::yank(self, text);
    }

    fn ctrl_d(&mut self) -> LineAction {
        if self.is_empty() {
            LineAction::Eof
        } else {
            Line::delete(self);
            LineAction::Continue
        }
    }
}

pub struct LineEditor {
    line: Line,
    /// 最近一次杀掉的文本, 供 Ctrl-Y 粘贴.
    kill: String,
}

impl LineEditor {
    #[must_use]
    pub fn new() -> Self {
        Self {
            line: Line::new(),
            kill: String::new(),
        }
    }

    #[must_use]
    pub fn line(&self) -> &Line {
        &self.line
    }

    pub fn line_mut(&mut self) -> &mut Line {
        &mut self.line
    }

    #[must_use]
    pub fn text(&self) -> &str {
        self.line.text()
    }

    #[must_use]
    pub fn cursor(&self) -> usize {
        self.line.cursor()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.line.is_empty()
    }

    pub fn clear(&mut self) {
        self.line.clear();
    }

    pub fn set_text(&mut self, text: String) {
        self.line.set_text(text);
    }

    /// 处理一次按键; 调用方负责过滤松开事件.
    pub fn handle_key(&mut self, key: KeyEvent) -> LineAction {
        handle_key(&mut self.line, &mut self.kill, key)
    }
}

impl Default for LineEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{LineAction, LineEditor};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn inserts_moves_edits_and_accepts() {
        let mut editor = LineEditor::new();
        for c in "abc".chars() {
            editor.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(editor.text(), "abc");
        editor.handle_key(key(KeyCode::Left));
        editor.handle_key(key(KeyCode::Left));
        editor.handle_key(key(KeyCode::Backspace));
        assert_eq!(editor.text(), "bc");
        editor.handle_key(ctrl('a'));
        editor.handle_key(key(KeyCode::Delete));
        assert_eq!(editor.text(), "c");
        assert_eq!(editor.handle_key(key(KeyCode::Enter)), LineAction::Accept);
    }

    #[test]
    fn kill_and_yank() {
        let mut editor = LineEditor::new();
        for c in "one two".chars() {
            editor.handle_key(key(KeyCode::Char(c)));
        }
        editor.handle_key(ctrl('w'));
        assert_eq!(editor.text(), "one ");
        editor.handle_key(ctrl('y'));
        assert_eq!(editor.text(), "one two");
    }

    #[test]
    fn control_keys_signal_interrupt_eof_and_clear() {
        let mut editor = LineEditor::new();
        assert_eq!(editor.handle_key(ctrl('c')), LineAction::Interrupt);
        assert_eq!(editor.handle_key(ctrl('d')), LineAction::Eof);

        // 非空行上的 Ctrl-D 删除光标处字符, 不结束输入.
        editor.handle_key(key(KeyCode::Char('x')));
        assert_eq!(editor.handle_key(ctrl('d')), LineAction::Continue);
        assert_eq!(editor.text(), "x");
        editor.handle_key(ctrl('a'));
        assert_eq!(editor.handle_key(ctrl('d')), LineAction::Continue);
        assert_eq!(editor.text(), "");

        assert_eq!(editor.handle_key(ctrl('l')), LineAction::Clear);
    }
}
