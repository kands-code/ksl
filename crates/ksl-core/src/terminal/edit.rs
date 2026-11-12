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
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Enter => return LineAction::Accept,
            KeyCode::Backspace => {
                self.line.backspace();
            },
            KeyCode::Delete => {
                self.line.delete();
            },
            KeyCode::Left => {
                self.line.move_left();
            },
            KeyCode::Right => {
                self.line.move_right();
            },
            KeyCode::Home => self.line.move_home(),
            KeyCode::End => self.line.move_end(),
            KeyCode::Tab => self.line.insert_str("    "),
            KeyCode::Char('a') if ctrl => self.line.move_home(),
            KeyCode::Char('e') if ctrl => self.line.move_end(),
            KeyCode::Char('b') if ctrl => {
                self.line.move_left();
            },
            KeyCode::Char('f') if ctrl => {
                self.line.move_right();
            },
            KeyCode::Char('k') if ctrl => self.kill = self.line.kill_to_end(),
            KeyCode::Char('u') if ctrl => self.kill = self.line.kill_to_start(),
            KeyCode::Char('w') if ctrl => self.kill = self.line.kill_previous_word(),
            KeyCode::Char('y') if ctrl => self.line.yank(&self.kill),
            KeyCode::Char('d') if ctrl => {
                if self.line.is_empty() {
                    return LineAction::Eof;
                }
                self.line.delete();
            },
            KeyCode::Char('c') if ctrl => return LineAction::Interrupt,
            KeyCode::Char('l') if ctrl => return LineAction::Clear,
            KeyCode::Char('h') if ctrl => {
                self.line.backspace();
            },
            KeyCode::Char('j' | 'm') if ctrl => return LineAction::Accept,
            KeyCode::Char(_) if ctrl || alt => return LineAction::Continue,
            KeyCode::Char(c) => self.line.insert_char(c),
            _ => {},
        }
        LineAction::Continue
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
