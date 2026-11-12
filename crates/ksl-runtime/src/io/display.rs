//! 行编辑画面与异步输出的协调.
//!
//! 行编辑期间, 提示符与编辑行由本模块持有.
//! `Print` 与 `PrintError` 先清除编辑区, 写出文本, 再重绘提示符.
//! 异步文本须以行首结束; 本模块不补换行控制序列.
//! 所有终端写入都在同一把显示锁内完成, 锁顺序为显示锁, 然后标准输出或标准错误.
//! 持有显示锁时不得等待输出服务或输入线程.

use std::io::IsTerminal as _;
use std::sync::Arc;

use crossterm::event::KeyEvent;
use ksl_frontend::error::RuntimeError as Error;
use parking_lot::Mutex as PlMutex;

use super::output_error;
use super::stdio::Stdio;
use crate::terminal::state::TerminalFlags;
use crate::terminal::{
    Ink, LineAction, LineEditor, RenderState, clear, draw, finish as finish_layout, layout,
};

/// 一次运行时的终端共享状态.
pub(super) struct TerminalShared {
    stdio: Stdio,
    flags: Arc<TerminalFlags>,
    display: PlMutex<Option<EditDisplay>>,
}

impl TerminalShared {
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            stdio: Stdio::new(),
            flags: Arc::new(TerminalFlags::new()),
            display: PlMutex::new(None),
        }
    }

    #[must_use]
    pub(super) fn stdio(&self) -> &Stdio {
        &self.stdio
    }

    #[must_use]
    pub(super) fn flags(&self) -> &TerminalFlags {
        &self.flags
    }

    #[must_use]
    pub(super) fn flags_handle(&self) -> Arc<TerminalFlags> {
        Arc::clone(&self.flags)
    }
}

/// 一次行编辑的显示状态.
struct EditDisplay {
    prompt: String,
    editor: LineEditor,
    state: RenderState,
    width: usize,
}

impl EditDisplay {
    fn new(prompt: String, text: Option<String>, width: usize) -> Self {
        let mut editor = LineEditor::new();
        if let Some(text) = text {
            editor.set_text(text);
        }
        Self {
            prompt,
            editor,
            state: RenderState::new(),
            width,
        }
    }

    fn render_to(&mut self, out: &mut Vec<u8>) -> std::io::Result<()> {
        let layout = layout(
            &self.prompt,
            self.editor.text(),
            self.editor.cursor(),
            self.width,
            |_| Ink::Plain,
        );
        draw(out, &mut self.state, &layout)
    }

    fn finish_to(&mut self, out: &mut Vec<u8>) -> std::io::Result<()> {
        let layout = layout(
            &self.prompt,
            self.editor.text(),
            self.editor.cursor(),
            self.width,
            |_| Ink::Plain,
        );
        finish_layout(out, &mut self.state, &layout)
    }

    fn render(&mut self, shared: &TerminalShared) -> Result<(), Error> {
        let mut buffer = Vec::new();
        self.render_to(&mut buffer)
            .map_err(|error| output_error(&error, "绘制输入"))?;
        write_buffer(shared, buffer)
    }

    /// `print_message` 的缓冲版本, 输出写入 `out`.
    fn message_to(&mut self, out: &mut Vec<u8>, text: &str) -> std::io::Result<()> {
        clear(out, &mut self.state)?;
        out.extend_from_slice(text.as_bytes());
        self.render_to(out)
    }

    /// 在编辑区上方写出文本; 文本须自行以行首结束.
    fn print_message(&mut self, shared: &TerminalShared, text: &str) -> Result<(), Error> {
        let mut buffer = Vec::new();
        self.message_to(&mut buffer, text)
            .map_err(|error| output_error(&error, "绘制输入"))?;
        write_buffer(shared, buffer)
    }

    /// 清除编辑区, 写出标准错误, 再重绘.
    fn print_error(&mut self, shared: &TerminalShared, text: &str) -> Result<(), Error> {
        let mut buffer = Vec::new();
        clear(&mut buffer, &mut self.state)
            .map_err(|error| output_error(&error, "清除输入"))?;
        write_buffer(shared, buffer)?;
        shared
            .stdio()
            .write_stderr(text)
            .map_err(|error| output_error(&error, "写出标准错误"))?;
        self.render(shared)
    }
}

/// 开启一次行编辑并绘制初始画面.
pub(super) fn begin(
    shared: &TerminalShared,
    prompt: String,
    text: Option<String>,
    width: usize,
) -> Result<(), Error> {
    let mut slot = shared.display.lock();
    if let Some(mut old) = slot.take() {
        // 上一次编辑未正常结束; 先清掉旧画面.
        let mut buffer = Vec::new();
        let _ = clear(&mut buffer, &mut old.state);
        let _ = write_buffer(shared, buffer);
    }
    let mut display = EditDisplay::new(prompt, text, width);
    display.render(shared)?;
    *slot = Some(display);
    Ok(())
}

/// 处理一次按键; Continue 时立即重绘.
pub(super) fn handle_key(
    shared: &TerminalShared,
    key: KeyEvent,
) -> Result<Option<LineAction>, Error> {
    let mut slot = shared.display.lock();
    let Some(display) = slot.as_mut() else {
        return Ok(None);
    };
    let action = display.editor.handle_key(key);
    if action == LineAction::Continue {
        display.render(shared)?;
    }
    Ok(Some(action))
}

/// 提交或取消当前行, 返回已编辑的文本.
pub(super) fn finish(shared: &TerminalShared) -> Result<Option<String>, Error> {
    let mut slot = shared.display.lock();
    let Some(mut display) = slot.take() else {
        return Ok(None);
    };
    let mut buffer = Vec::new();
    display
        .finish_to(&mut buffer)
        .map_err(|error| output_error(&error, "写完输入"))?;
    write_buffer(shared, buffer)?;
    Ok(Some(display.editor.text().to_owned()))
}

/// 清除编辑区并写出中断提示.
pub(super) fn interrupt(shared: &TerminalShared) -> Result<bool, Error> {
    let mut slot = shared.display.lock();
    let Some(mut display) = slot.take() else {
        return Ok(false);
    };
    let mut buffer = Vec::new();
    clear(&mut buffer, &mut display.state).map_err(|error| output_error(&error, "清除输入"))?;
    buffer.extend_from_slice(b"^C\r\n");
    write_buffer(shared, buffer)?;
    Ok(true)
}

/// 清屏并重绘当前编辑行.
pub(super) fn clear_screen(shared: &TerminalShared) -> Result<bool, Error> {
    let mut slot = shared.display.lock();
    let Some(display) = slot.as_mut() else {
        return Ok(false);
    };
    let mut buffer = Vec::new();
    buffer.extend_from_slice(b"\x1b[H\x1b[2J");
    display.state.reset();
    display
        .render_to(&mut buffer)
        .map_err(|error| output_error(&error, "绘制输入"))?;
    write_buffer(shared, buffer)?;
    Ok(true)
}

/// 按新的终端宽度重绘.
pub(super) fn resize(shared: &TerminalShared, width: usize) -> Result<(), Error> {
    let mut slot = shared.display.lock();
    let Some(display) = slot.as_mut() else {
        return Ok(());
    };
    display.width = width;
    display.render(shared)
}

/// 丢弃编辑状态并清除画面; 用于读取失败等异常路径.
pub(super) fn abandon(shared: &TerminalShared) {
    let mut slot = shared.display.lock();
    if let Some(mut display) = slot.take() {
        let mut buffer = Vec::new();
        let _ = clear(&mut buffer, &mut display.state);
        let _ = write_buffer(shared, buffer);
    }
}

/// 写出标准输出; 行编辑期间先清行再重绘.
pub(super) fn print(shared: &TerminalShared, text: &str) -> Result<(), Error> {
    let mut slot = shared.display.lock();
    match slot.as_mut() {
        Some(display) => display.print_message(shared, text),
        None => shared
            .stdio()
            .write_stdout(text)
            .map_err(|error| output_error(&error, "写出标准输出")),
    }
}

/// 写出标准错误; 标准错误是终端且正在行编辑时先清行再重绘.
pub(super) fn print_error(shared: &TerminalShared, text: &str) -> Result<(), Error> {
    if !std::io::stderr().is_terminal() {
        return shared
            .stdio()
            .write_stderr(text)
            .map_err(|error| output_error(&error, "写出标准错误"));
    }
    let mut slot = shared.display.lock();
    match slot.as_mut() {
        Some(display) => display.print_error(shared, text),
        None => shared
            .stdio()
            .write_stderr(text)
            .map_err(|error| output_error(&error, "写出标准错误")),
    }
}

fn write_buffer(shared: &TerminalShared, buffer: Vec<u8>) -> Result<(), Error> {
    let text = String::from_utf8(buffer).map_err(|_| Error::internal("终端输出不是 UTF-8"))?;
    shared
        .stdio()
        .write_stdout(&text)
        .map_err(|error| output_error(&error, "写出终端画面"))
}

#[cfg(test)]
mod tests {
    use super::EditDisplay;

    #[test]
    fn message_is_written_before_redraw() {
        let mut display = EditDisplay::new("> ".to_owned(), Some("abc".to_owned()), 80);
        let mut initial = Vec::new();
        display.render_to(&mut initial).expect("初始绘制应成功");

        let mut out = Vec::new();
        display.message_to(&mut out, "msg\r\n").expect("消息绘制应成功");
        let text = String::from_utf8(out).expect("输出应为 UTF-8");
        let message = text.find("msg\r\n").expect("应写出消息");
        let prompt = text.rfind("> abc").expect("应重绘提示符");
        assert!(message < prompt);
    }
}
