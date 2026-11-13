//! 交互输入编辑器.
//!
//! 采用多行缓冲: 段内任意行可编辑, `n$ ` 提示正在编辑的焦点行, 其余行
//! 按位置显示 `n> ` 与 `n| `; 超出终端高度时按视口窗口显示, 焦点行始终
//! 可见. Enter 在输入完整时整段提交, 否则在光标处拆行续输; Backspace
//! 在行首合并上一行, Ctrl-D 在行尾合并下一行, 末尾空行上的 Ctrl-D 删除
//! 该空行.
//!
//! 键位是 Emacs 子集: 行内移动与杀/粘贴由 `BufferEditor` 处理,
//! Ctrl-P/N 在行间转移编辑焦点, 方向键上下选择历史 (仅单行输入时),
//! 括号匹配与语法着色由 `highlight` 模块按词法器计算.

use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ksl_core::symbol::Symbol;
use ksl_core::terminal::{
    BufferEditor, Ink, LineAction, LineLayout, RenderState, Session, clear, compose, draw,
    finish, layout,
};

use self::history::History;
use super::complete;
use super::highlight::{Span, line_spans};

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
    builtins: HashSet<Symbol>,
    width: usize,
    height: usize,
    plain: bool,
}

impl Editor {
    pub(super) fn new(builtins: HashSet<Symbol>) -> Self {
        let plain = !std::io::stdin().is_terminal()
            || !std::io::stdout().is_terminal()
            || std::env::var("TERM").is_ok_and(|term| term == "dumb")
            || !supports_ansi();
        let (width, height) = crossterm::terminal::size()
            .map_or((80, 24), |(width, height)| {
                (defaulted(width, 80), defaulted(height, 24))
            });
        Self {
            history: History::new(),
            buffer: BufferEditor::new(),
            builtins,
            width,
            height,
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
            // 逐行输入统一经由输入线程; 取消缓冲由它先行消费.
            let line = match ksl_core::io::read_repl_line() {
                Ok(Some(line)) => line,
                Ok(None) => return ReadOutcome::Eof,
                Err(err) => {
                    errln!("读取输入失败: {err:?}");
                    return ReadOutcome::Failed;
                },
            };
            let full = join_block(&block, &line);
            if complete(&full) {
                if !full.trim().is_empty() {
                    self.history.add(full.clone());
                }
                return ReadOutcome::Line(full);
            }
            block.push(line);
        }
    }

    fn read_interactive(&mut self, round: usize) -> ReadOutcome {
        // 会话开启后输入线程被会话占用, 缓冲请求必须在此之前取走.
        let initial =
            ksl_core::io::take_buffered_text().or_else(ksl_core::io::take_buffered_line);
        let mut out = io::stdout();
        let session = match Session::open(true) {
            Ok(session) => session,
            Err(err) => {
                errln!("无法进入交互编辑, 退回逐行读取: {err:?}");
                self.plain = true;
                return self.read_plain();
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
                Event::Resize(cols, rows) => {
                    self.width = defaulted(cols, 80);
                    self.height = defaulted(rows, 24);
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
            // 历史选择只用方向键, 且仅单行输入时可用.
            KeyCode::Up if self.buffer.buffer().line_count() == 1 => {
                self.history_previous(round, state, out)?;
                return Ok(KeyResult::Continue);
            },
            KeyCode::Down if self.buffer.buffer().line_count() == 1 => {
                self.history_next(round, state, out)?;
                return Ok(KeyResult::Continue);
            },
            // Ctrl-P/N 在行间转移编辑焦点, Emacs 语义的行上移与行下移.
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
                clear(out, state)?;
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
            // 接受前先按终态重绘: 焦点行已完成编辑, `$` 标记与括号
            // 强调固定回位置提示符, 留下的回显不再带编辑状态.
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
        // 输入未完: 在光标处拆行续输.
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

    /// 组合整段输入的视觉布局: 每行按各自提示符布局, 焦点行带括号标记,
    /// 超出终端高度时保留末尾窗口且焦点行可见.
    ///
    /// `editing` 为假时表示接受或结束后的终态: 焦点行已完成编辑,
    /// 统一显示位置提示符, 且不带括号强调.
    fn layout_for(&self, round: usize, editing: bool) -> LineLayout {
        let buffer = self.buffer.buffer();
        let full = buffer.text();
        let focus = buffer.focus();
        let mut parts = Vec::with_capacity(buffer.line_count());
        let mut offset = 0;
        for (index, line) in buffer.lines().iter().enumerate() {
            let prompt = if editing && index == focus {
                format!("{round}$ ")
            } else if index == 0 {
                format!("{round}> ")
            } else {
                format!("{round}| ")
            };
            let line_start = offset;
            let line_end = line_start + line.text().len();
            // 括号标记只跟随编辑中焦点行的光标; 其余行按普通配色.
            let cursor = if editing && index == focus {
                Some(line_start + line.cursor())
            } else {
                None
            };
            let spans = line_spans(&full, line_start, line_end, cursor, &self.builtins);
            parts.push(layout(&prompt, line.text(), line.cursor(), self.width, |at| {
                ink_at(&spans, at)
            }));
            offset = line_end + 1;
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

/// 终端报告 0 列或 0 行时退回默认值; 无尺寸报告的伪终端按 80x24 处理.
fn defaulted(value: u16, default: usize) -> usize {
    if value == 0 {
        default
    } else {
        usize::from(value)
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
