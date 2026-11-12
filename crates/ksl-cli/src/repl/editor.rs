//! 交互输入编辑器.
//!
//! 采用单行编辑加固定行续行: 当前行就地编辑, 括号或字符串未闭合时把当前行
//! 固定到终端, 以 `n| ` 继续输入; 完整后整段提交. 已固定的行不再移动光标,
//! 因此不需要多行缓冲与 viewport.
//!
//! 键位是 Emacs 子集: 行内移动, 杀/粘贴, 历史, Ctrl-C/D, Ctrl-L. 不处理
//! Alt 组合, 也不做补全. 括号匹配与语法着色由 `highlight` 模块按词法器计算.

use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};

use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ksl_core::symbol::Symbol;
use ksl_core::terminal::{
    Ink, LineAction, LineEditor, LineLayout, RenderState, clear, draw, finish, layout,
};

use self::history::History;
use self::render::print_fixed;
use super::complete;
use super::highlight::{Span, line_spans};

mod history;
mod render;

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
    line: LineEditor,
    builtins: HashSet<Symbol>,
    width: usize,
    plain: bool,
}

impl Editor {
    pub(super) fn new(builtins: HashSet<Symbol>) -> Self {
        let plain = !std::io::stdin().is_terminal()
            || !std::io::stdout().is_terminal()
            || std::env::var("TERM").is_ok_and(|term| term == "dumb")
            || !supports_ansi();
        let width = crossterm::terminal::size().map_or(80, |(width, _)| {
            if width == 0 { 80 } else { usize::from(width) }
        });
        Self {
            history: History::new(),
            line: LineEditor::new(),
            builtins,
            width,
            plain,
        }
    }

    pub(super) fn is_interactive(&self) -> bool {
        !self.plain
    }

    pub(super) fn read(&mut self, round: usize) -> ReadOutcome {
        if self.plain {
            self.read_plain()
        } else {
            self.read_interactive(round)
        }
    }

    /// 非终端输入: 不打印提示符, 按行累加直到输入完整.
    fn read_plain(&mut self) -> ReadOutcome {
        let mut block: Vec<String> = Vec::new();
        loop {
            let mut raw = String::new();
            match io::stdin().read_line(&mut raw) {
                Ok(0) => return ReadOutcome::Eof,
                Ok(_) => {
                    let line = strip_line_ending(raw);
                    let full = join_block(&block, &line);
                    if complete(&full) {
                        if !full.trim().is_empty() {
                            self.history.add(full.clone());
                        }
                        return ReadOutcome::Line(full);
                    }
                    block.push(line);
                },
                Err(err) => {
                    errln!("读取输入失败: {err}");
                    return ReadOutcome::Failed;
                },
            }
        }
    }

    fn read_interactive(&mut self, round: usize) -> ReadOutcome {
        let mut out = io::stdout();
        let _guard = match RawGuard::new() {
            Ok(guard) => guard,
            Err(err) => {
                errln!("无法进入交互编辑, 退回逐行读取: {err}");
                self.plain = true;
                return self.read_plain();
            },
        };
        let mut block: Vec<String> = Vec::new();
        self.line.clear();
        let mut state = RenderState::new();
        if let Err(err) = self.redraw(&mut out, &mut state, round, &block) {
            errln!("绘制输入失败: {err}");
            return ReadOutcome::Failed;
        }
        loop {
            let event = match event::read() {
                Ok(event) => event,
                Err(err) => {
                    errln!("读取输入失败: {err}");
                    return ReadOutcome::Failed;
                },
            };
            match event {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    let result =
                        match self.handle_key(key, round, &mut block, &mut state, &mut out) {
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
                    if let Err(err) =
                        self.handle_paste(&text, round, &mut block, &mut state, &mut out)
                    {
                        errln!("绘制输入失败: {err}");
                        return ReadOutcome::Failed;
                    }
                },
                Event::Resize(cols, _) => {
                    self.width = if cols == 0 { 80 } else { usize::from(cols) };
                    if let Err(err) = self.redraw(&mut out, &mut state, round, &block) {
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
        block: &mut Vec<String>,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<KeyResult> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Up => {
                self.history_previous(round, block, state, out)?;
                return Ok(KeyResult::Continue);
            },
            KeyCode::Down => {
                self.history_next(round, block, state, out)?;
                return Ok(KeyResult::Continue);
            },
            KeyCode::Char('p') if ctrl => {
                self.history_previous(round, block, state, out)?;
                return Ok(KeyResult::Continue);
            },
            KeyCode::Char('n') if ctrl => {
                self.history_next(round, block, state, out)?;
                return Ok(KeyResult::Continue);
            },
            KeyCode::Char('d') if ctrl && self.line.is_empty() && !block.is_empty() => {
                clear(out, state)?;
                block.clear();
                self.history.reset();
                self.redraw(out, state, round, block)?;
                return Ok(KeyResult::Continue);
            },
            _ => {},
        }
        match self.line.handle_key(key) {
            LineAction::Continue => {
                self.redraw(out, state, round, block)?;
                Ok(KeyResult::Continue)
            },
            LineAction::Accept => self.accept_or_continue(round, block, state, out),
            LineAction::Interrupt => {
                clear(out, state)?;
                write!(out, "^C\r\n")?;
                out.flush()?;
                self.line.clear();
                self.history.reset();
                Ok(KeyResult::Interrupt)
            },
            LineAction::Eof => {
                if block.is_empty() {
                    let current = self.layout_for(round, block);
                    finish(out, state, &current)?;
                    self.line.clear();
                    Ok(KeyResult::Eof)
                } else {
                    clear(out, state)?;
                    block.clear();
                    self.history.reset();
                    self.redraw(out, state, round, block)?;
                    Ok(KeyResult::Continue)
                }
            },
            LineAction::Clear => {
                clear(out, state)?;
                self.redraw(out, state, round, block)?;
                Ok(KeyResult::Continue)
            },
        }
    }

    fn accept_or_continue(
        &mut self,
        round: usize,
        block: &mut Vec<String>,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<KeyResult> {
        let full = join_block(block, self.line.text());
        if complete(&full) {
            let current = self.layout_for(round, block);
            finish(out, state, &current)?;
            if !full.trim().is_empty() {
                self.history.add(full.clone());
            }
            self.history.reset();
            self.line.clear();
            return Ok(KeyResult::Accept(full));
        }
        // 固定行不再显示括号标记, 避免第一行留下的未配对红色在后续行补齐后仍然存在.
        self.commit_line(round, block, state, out)?;
        block.push(self.line.text().to_owned());
        self.line.clear();
        self.history.reset();
        self.redraw(out, state, round, block)?;
        Ok(KeyResult::Continue)
    }

    /// 把当前行作为固定行打印; 不检查完整性.
    fn commit_line(
        &self,
        round: usize,
        block: &[String],
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let line = self.line.line();
        let prompt = if block.is_empty() {
            format!("{round}> ")
        } else {
            format!("{round}| ")
        };
        let prefix = if block.is_empty() {
            0
        } else {
            block.join("\n").len() + 1
        };
        let line_start = prefix;
        let line_end = prefix + line.text().len();
        let full = join_block(block, line.text());
        let spans = line_spans(&full, line_start, line_end, None, &self.builtins);
        clear(out, state)?;
        print_fixed(out, &prompt, line.text(), &spans)
    }

    fn history_previous(
        &mut self,
        round: usize,
        block: &mut Vec<String>,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if !block.is_empty() {
            return Ok(());
        }
        let current = self.line.text().to_owned();
        let Some(entry) = self.history.previous(&current) else {
            return Ok(());
        };
        self.recall(&entry, round, block, state, out)
    }

    fn history_next(
        &mut self,
        round: usize,
        block: &mut Vec<String>,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if !block.is_empty() {
            return Ok(());
        }
        let Some(entry) = self.history.next() else {
            return Ok(());
        };
        self.recall(&entry, round, block, state, out)
    }

    /// 把历史条目放回输入区: 除最后一行外都作为固定行打印.
    fn recall(
        &mut self,
        entry: &str,
        round: usize,
        block: &mut Vec<String>,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        clear(out, state)?;
        let mut parts: Vec<&str> = entry.split('\n').collect();
        let last = parts.pop().unwrap_or("");
        if parts.is_empty() {
            self.line.set_text(last.to_owned());
            self.redraw(out, state, round, block)?;
            return Ok(());
        }
        let mut offset = 0;
        for (index, part) in parts.iter().enumerate() {
            let prompt = if index == 0 {
                format!("{round}> ")
            } else {
                format!("{round}| ")
            };
            let start = offset;
            let end = start + part.len();
            let spans = line_spans(entry, start, end, None, &self.builtins);
            print_fixed(out, &prompt, part, &spans)?;
            offset = end + 1;
        }
        *block = parts.iter().map(|part| (*part).to_owned()).collect();
        self.line.set_text(last.to_owned());
        state.reset();
        self.redraw(out, state, round, block)
    }

    /// 粘贴按行提交: 除最后一段外都固定到终端, 最后一段留在当前行.
    fn handle_paste(
        &mut self,
        text: &str,
        round: usize,
        block: &mut Vec<String>,
        state: &mut RenderState,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            self.line.line_mut().insert_str(part);
            if parts.peek().is_some() {
                self.commit_line(round, block, state, out)?;
                block.push(self.line.text().to_owned());
                self.line.clear();
                self.history.reset();
            }
        }
        self.redraw(out, state, round, block)
    }

    fn layout_for(&self, round: usize, block: &[String]) -> LineLayout {
        let line = self.line.line();
        let prompt = if block.is_empty() {
            format!("{round}> ")
        } else {
            format!("{round}| ")
        };
        let prefix = if block.is_empty() {
            0
        } else {
            block.join("\n").len() + 1
        };
        let line_start = prefix;
        let line_end = prefix + line.text().len();
        let full = join_block(block, line.text());
        let cursor = line_start + line.cursor();
        let spans = line_spans(&full, line_start, line_end, Some(cursor), &self.builtins);
        layout(&prompt, line.text(), line.cursor(), self.width, |offset| {
            ink_at(&spans, offset)
        })
    }

    fn redraw(
        &self,
        out: &mut impl Write,
        state: &mut RenderState,
        round: usize,
        block: &[String],
    ) -> io::Result<()> {
        let current = self.layout_for(round, block);
        draw(out, state, &current)
    }
}

/// Windows 控制台需要显式检查 ANSI 支持; Unix 终端按支持 ANSI 处理.
fn supports_ansi() -> bool {
    #[cfg(windows)]
    {
        crossterm::ansi_support::supports_ansi()
    }
    #[cfg(not(windows))]
    {
        true
    }
}

fn ink_at(spans: &[Span], offset: usize) -> Ink {
    spans
        .iter()
        .find(|span| span.start <= offset && offset < span.end)
        .map_or(Ink::Plain, |span| span.ink)
}

fn join_block(block: &[String], line: &str) -> String {
    if block.is_empty() {
        line.to_owned()
    } else {
        format!("{}\n{}", block.join("\n"), line)
    }
}

fn strip_line_ending(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
    }
    if line.ends_with('\r') {
        line.pop();
    }
    line
}

/// 离开交互读取时恢复终端状态.
struct RawGuard {
    raw: bool,
    paste: bool,
}

impl RawGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut guard = Self {
            raw: true,
            paste: false,
        };
        execute!(io::stdout(), EnableBracketedPaste)?;
        guard.paste = true;
        Ok(guard)
    }
}

impl Drop for RawGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        if self.paste {
            let _ = execute!(out, DisableBracketedPaste);
        }
        if self.raw {
            let _ = disable_raw_mode();
        }
        let _ = out.flush();
    }
}
