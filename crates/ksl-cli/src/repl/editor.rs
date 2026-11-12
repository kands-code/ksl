//! 交互输入编辑器.
//!
//! 采用多行缓冲: 段内任意行可编辑, `n$ ` 提示正在编辑的焦点行.
//! 其余行按位置显示 `n> ` 与 `n| `. 超出终端高度时按视口窗口显示,
//! 焦点行始终可见.
//! Enter 在输入完整时整段提交, 否则在光标处拆行续输.
//! Backspace 在行首合并上一行, Ctrl-D 在行尾合并下一行.
//! 末尾空行上的 Ctrl-D 删除该空行.
//!
//! 键位是 Emacs 子集:
//! 行内移动与剪切/粘贴由 `BufferEditor` 处理.
//! Ctrl-P/N 在行间转移编辑焦点.
//! 方向键上下选择历史.
//! 括号匹配与语法着色由 `highlight` 模块按词法器计算.

use std::io::{self, IsTerminal, Write};

use ksl_host::Host;
use ksl_io::terminal::{
    BufferEditor, Event, Ink, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, LineAction,
    LineLayout, RenderState, clear, compose, draw, finish, layout,
};

use self::history::History;
use super::complete;
use super::highlight::{HighlightEnv, Highlighter, Span};

mod history;

/// 一次读入的结果.
pub(super) enum ReadOutcome {
    /// 读到完整输入.
    Line(String),
    /// Ctrl-C: 放弃本次输入, 会话继续.
    Interrupted,
    /// Ctrl-D: 结束会话.
    Eof,
    /// 读取失败, 无法继续.
    Failed,
}

/// 编辑器的按键处置.
enum KeyResult {
    Continue,
    Accept(String),
    Interrupt,
    Eof,
}

pub(super) struct Editor {
    history: History,
    buffer: BufferEditor,
    highlighter: Highlighter,
    width: usize,
    height: usize,
}

impl Editor {
    pub(super) fn new() -> Self {
        let (width, height) = (80, 24);
        Self {
            history: History::new(),
            buffer: BufferEditor::new(),
            highlighter: Highlighter::new(),
            width,
            height,
        }
    }

    pub(super) fn read(&mut self, host: &Host, round: usize) -> ReadOutcome {
        self.highlighter.set_env(HighlightEnv::from_host(host));
        // 会话开启后输入线程被会话占用, 缓冲请求必须在此之前取走.
        let initial = host.take_buffered_text().or_else(|| host.take_buffered_line());
        let mut out = io::stdout();
        let session = match host.open_terminal_session(true) {
            Ok(session) => session,
            Err(err) => {
                errln!("进入交互编辑失败: {err:?}");
                return ReadOutcome::Failed;
            },
        };
        let (width, height) = session.size();
        self.width = width;
        self.height = height;
        self.buffer.clear();
        // 取消的程序读取留下的文本作为编辑初值, 不丢输入.
        if let Some(text) = initial {
            self.buffer.set_text(&text);
        }
        let mut state = RenderState::new();
        if let Err(err) = self.redraw(&mut out, &mut state, round) {
            errln!("绘制输入失败: {err}");
            return ReadOutcome::Failed;
        }
        loop {
            let event = match session.recv() {
                Ok(event) => event,
                Err(err) => {
                    errln!("读取输入失败: {err:?}");
                    return ReadOutcome::Failed;
                },
            };
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    let result = match self.handle_key(key, round, &mut state, &mut out) {
                        Ok(result) => result,
                        Err(err) => {
                            errln!("绘制输入失败: {err}");
                            return ReadOutcome::Failed;
                        },
                    };
                    match result {
                        KeyResult::Continue => {},
                        KeyResult::Accept(text) => return ReadOutcome::Line(text),
                        KeyResult::Interrupt => return ReadOutcome::Interrupted,
                        KeyResult::Eof => return ReadOutcome::Eof,
                    }
                },
                Event::Paste(text) => {
                    if let Err(err) = self.handle_paste(&text, round, &mut state, &mut out) {
                        errln!("绘制输入失败: {err}");
                        return ReadOutcome::Failed;
                    }
                },
                Event::WindowResized(size) => {
                    self.width = ksl_io::terminal::dimension(size.cols, 80);
                    self.height = ksl_io::terminal::dimension(size.rows, 24);
                    if let Err(err) = self.redraw(&mut out, &mut state, round) {
                        errln!("绘制输入失败: {err}");
                        return ReadOutcome::Failed;
                    }
                },
                _ => {},
            }
        }
    }

    fn handle_key(
        &mut self,
        key: KeyEvent,
        round: usize,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<KeyResult> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up => {
                self.history_previous(round, state, out)?;
                return Ok(KeyResult::Continue);
            },
            KeyCode::Down => {
                self.history_next(round, state, out)?;
                return Ok(KeyResult::Continue);
            },
            // Emacs 语义的行上移与行下移.
            KeyCode::Char('p') if ctrl => {
                self.buffer.buffer_mut().move_focus_up();
                self.redraw(out, state, round)?;
                return Ok(KeyResult::Continue);
            },
            KeyCode::Char('n') if ctrl => {
                self.buffer.buffer_mut().move_focus_down();
                self.redraw(out, state, round)?;
                return Ok(KeyResult::Continue);
            },
            _ => {},
        }
        match self.buffer.handle_key(key) {
            LineAction::Continue => {
                self.redraw(out, state, round)?;
                Ok(KeyResult::Continue)
            },
            LineAction::Accept => self.accept_or_continue(round, state, out),
            LineAction::Interrupt => {
                clear(out, state)?;
                write!(out, "^C\r\n")?;
                out.flush()?;
                self.buffer.clear();
                self.history.reset();
                Ok(KeyResult::Interrupt)
            },
            LineAction::Eof => {
                let current = self.layout_for(round, false);
                draw(out, state, &current)?;
                finish(out, state, &current)?;
                self.buffer.clear();
                Ok(KeyResult::Eof)
            },
            LineAction::Clear => {
                // 清屏后从左上角重绘输入区.
                write!(out, "\x1b[H\x1b[2J")?;
                state.reset();
                self.redraw(out, state, round)?;
                Ok(KeyResult::Continue)
            },
        }
    }

    fn accept_or_continue(
        &mut self,
        round: usize,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<KeyResult> {
        let full = self.buffer.buffer().text();
        if complete(&full) {
            // 接受前先按终态重绘: 焦点行已完成编辑,
            // `$` 标记与括号强调固定回位置提示符,
            // 留下的回显不带编辑状态.
            let current = self.layout_for(round, false);
            draw(out, state, &current)?;
            finish(out, state, &current)?;
            if !full.trim().is_empty() {
                self.history.add(full.clone());
            }
            self.history.reset();
            self.buffer.clear();
            return Ok(KeyResult::Accept(full));
        }
        self.buffer.buffer_mut().newline();
        self.history.reset();
        self.redraw(out, state, round)?;
        Ok(KeyResult::Continue)
    }

    fn history_previous(
        &mut self,
        round: usize,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let current = self.buffer.buffer().text();
        let Some(entry) = self.history.previous(&current) else {
            return Ok(());
        };
        self.recall(&entry, round, state, out)
    }

    fn history_next(
        &mut self,
        round: usize,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let Some(entry) = self.history.next() else {
            return Ok(());
        };
        self.recall(&entry, round, state, out)
    }

    /// 把历史条目放回编辑区, 焦点落在最后一行.
    fn recall(
        &mut self,
        entry: &str,
        round: usize,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        self.buffer.set_text(entry);
        self.redraw(out, state, round)
    }

    /// 粘贴按行并入缓冲, 末段成为新的焦点行.
    fn handle_paste(
        &mut self,
        text: &str,
        round: usize,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.buffer.buffer_mut().insert_block(&text);
        self.history.reset();
        self.redraw(out, state, round)
    }

    /// 组合整段输入的视觉布局.
    /// 每行按各自提示符布局, 焦点行带括号标记.
    /// 超出终端高度时保留末尾窗口且焦点行可见.
    ///
    /// `editing` 为假时表示接受或结束后的终态: 焦点行显示位置提示符, 不带括号强调.
    fn layout_for(&self, round: usize, editing: bool) -> LineLayout {
        let buffer = self.buffer.buffer();
        let full = buffer.text();
        let focus = buffer.focus();
        let lines = buffer.lines();
        let mut ranges = Vec::with_capacity(lines.len());
        let mut offset = 0;
        for line in lines {
            let start = offset;
            let end = start + line.text().len();
            ranges.push((start, end));
            offset = end + 1;
        }
        // 括号标记只跟随编辑中焦点行的光标; 其余行按普通配色.
        let cursor = if editing {
            Some(ranges[focus].0 + lines[focus].cursor())
        } else {
            None
        };
        let all_spans = self.highlighter.spans(&full, &ranges, cursor);
        let mut parts = Vec::with_capacity(lines.len());
        for (index, line) in lines.iter().enumerate() {
            let prompt = if editing && index == focus {
                format!("{round}$ ")
            } else if index == 0 {
                format!("{round}> ")
            } else {
                format!("{round}| ")
            };
            let spans = &all_spans[index];
            parts.push(layout(&prompt, line.text(), line.cursor(), self.width, |at| {
                ink_at(spans, at)
            }));
        }
        compose(parts, focus, self.height)
    }

    fn redraw(
        &self,
        out: &mut impl Write,
        state: &mut RenderState,
        round: usize,
    ) -> io::Result<()> {
        let current = self.layout_for(round, true);
        draw(out, state, &current)
    }
}

/// 判定当前进程能否进入交互编辑.
///
/// REPL 要求标准输入与标准输出都是终端, 且终端支持 ANSI.
pub(super) fn terminal_ready() -> Result<(), &'static str> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return Err("REPL 需要标准输入与标准输出都是终端; 非交互执行用 ksl run");
    }
    if std::env::var("TERM").is_ok_and(|term| term == "dumb") {
        return Err("终端不支持 ANSI; REPL 无法启动");
    }
    Ok(())
}

fn ink_at(spans: &[Span], offset: usize) -> Ink {
    spans
        .iter()
        .find(|span| span.start <= offset && offset < span.end)
        .map_or(Ink::Plain, |span| span.ink)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn arrows_browse_history_in_multiline_buffer() {
        let mut editor = Editor::new();
        editor.history.add("older".to_string());
        editor.buffer.set_text("a\nb");
        let mut state = RenderState::new();
        let mut out = Vec::new();

        editor
            .handle_key(key(KeyCode::Up), 1, &mut state, &mut out)
            .expect("按键应成功");
        assert_eq!(editor.buffer.buffer().text(), "older");

        // 翻过历史末尾取回草稿.
        editor
            .handle_key(key(KeyCode::Down), 1, &mut state, &mut out)
            .expect("按键应成功");
        assert_eq!(editor.buffer.buffer().text(), "a\nb");
    }
}
