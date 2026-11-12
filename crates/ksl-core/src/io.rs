//! 标准 I/O 操作.
//!
//! 同步实现同时供内置函数表, 固定 I/O 线程池与专用输入线程使用; 走调度器时
//! 由 `cps::exec` 提交任务, 抽象机挂起, 完成后唤醒, 避免阻塞其他 Actor.

#![allow(clippy::unnecessary_wraps)] // 统一 Result 外形, 供工作线程与同步路径共用.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;

use crate::error::{ErrorKind, RuntimeError as Error};
use crate::source::Span;
use crate::value::{
    Value, data_value, data_value0, option_none, option_some, result_err, result_ok,
};

mod input;
mod pool;

/// 是否正在跟踪标准输出的行首状态.
///
/// 跟踪状态是进程级的: 提示一类写出发生在 I/O 工作线程上, 只按线程记录会漏掉.
/// 交互模式的输入串行执行, 不存在两个跟踪窗口重叠的情形.
static STDOUT_TRACKING: AtomicBool = AtomicBool::new(false);

/// 标准输出是否停在行首; 仅在跟踪开启时生效.
static STDOUT_AT_LINE_START: AtomicBool = AtomicBool::new(true);

/// 跟踪期间标准输出是否写过非空文本.
static STDOUT_WROTE: AtomicBool = AtomicBool::new(false);

/// 是否正在跟踪标准错误的写出.
static STDERR_TRACKING: AtomicBool = AtomicBool::new(false);

/// 跟踪期间标准错误是否写过非空文本.
static STDERR_WROTE: AtomicBool = AtomicBool::new(false);

/// 写出标准输出.
///
/// 写出后立即冲刷, 终端上不滞留. 跟踪期间记录输出是否停在行首与是否写过
/// 非空文本, 供宿主决定结果标签前的空行与补行的时机.
///
/// # Errors
///
/// 标准输出不可写时返回底层错误.
pub fn write_stdout(text: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(text.as_bytes())?;
    stdout.flush()?;
    // 空文本不改变行首状态, 也不算写过输出.
    if !text.is_empty() && STDOUT_TRACKING.load(Ordering::Relaxed) {
        STDOUT_AT_LINE_START.store(text.ends_with('\n'), Ordering::Relaxed);
        STDOUT_WROTE.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// 写出标准错误.
///
/// 只用于程序自身的 `PrintError`; 诊断输出走宿主路径, 不计入这里的跟踪.
///
/// # Errors
///
/// 标准错误不可写时返回底层错误.
pub fn write_stderr(text: &str) -> std::io::Result<()> {
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(text.as_bytes())?;
    stderr.flush()?;
    if !text.is_empty() && STDERR_TRACKING.load(Ordering::Relaxed) {
        STDERR_WROTE.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// 开启标准输出的行首与写出跟踪.
pub fn begin_stdout_tracking() {
    STDOUT_AT_LINE_START.store(true, Ordering::Relaxed);
    STDOUT_WROTE.store(false, Ordering::Relaxed);
    STDOUT_TRACKING.store(true, Ordering::Relaxed);
}

/// 结束行首跟踪, 返回输出当前是否停在行首.
///
/// 未开启跟踪时返回真, 即视为停在行首.
#[must_use]
pub fn take_stdout_line_start() -> bool {
    STDOUT_TRACKING.store(false, Ordering::Relaxed);
    STDOUT_AT_LINE_START.load(Ordering::Relaxed)
}

/// 取本次跟踪期间标准输出是否写过非空文本.
#[must_use]
pub fn take_stdout_wrote() -> bool {
    let wrote = STDOUT_WROTE.load(Ordering::Relaxed);
    STDOUT_WROTE.store(false, Ordering::Relaxed);
    wrote
}

/// 开启标准错误的写出跟踪.
pub fn begin_stderr_tracking() {
    STDERR_WROTE.store(false, Ordering::Relaxed);
    STDERR_TRACKING.store(true, Ordering::Relaxed);
}

/// 结束标准错误跟踪, 返回本次是否写过非空文本.
#[must_use]
pub fn take_stderr_wrote() -> bool {
    STDERR_TRACKING.store(false, Ordering::Relaxed);
    let wrote = STDERR_WROTE.load(Ordering::Relaxed);
    STDERR_WROTE.store(false, Ordering::Relaxed);
    wrote
}

/// 打开的文件句柄; 读写方向在打开时确定, 关闭后不可再用.
pub struct FileHandle {
    inner: Mutex<Option<FileInner>>,
    path: String,
}

impl std::fmt::Debug for FileHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<file>")
    }
}

enum FileInner {
    Read(BufReader<File>),
    Write(BufWriter<File>),
}

/// I/O 任务类别; 调度器据此区分终端输入与普通阻塞操作.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoKind {
    /// 标准输入读取.
    Input,
    /// 文件或其他阻塞操作.
    Blocking,
}

/// 待执行的 I/O 任务; 终端读取由专用输入线程处理, 其余进入阻塞线程池.
pub(crate) enum Task {
    Blocking(Blocking),
    Line(String),
    Key,
}

type Blocking = Box<dyn FnOnce() -> Result<Value, Error> + Send + 'static>;

impl Task {
    fn blocking(job: impl FnOnce() -> Result<Value, Error> + Send + 'static) -> Self {
        Self::Blocking(Box::new(job))
    }

    #[must_use]
    pub(crate) fn kind(&self) -> IoKind {
        match self {
            Self::Line(_) | Self::Key => IoKind::Input,
            Self::Blocking(_) => IoKind::Blocking,
        }
    }
}

/// 提交 I/O 任务; 完成回调把结果交给调度器.
///
/// `cancel` 只对终端读取生效; 文件操作无法中断, 完成后结果由调度器丢弃.
pub(crate) fn submit_task(
    task: Task,
    cancel: Arc<AtomicBool>,
    done: impl FnOnce(Result<Value, Error>) + Send + 'static,
) {
    match task {
        Task::Blocking(job) => {
            drop(cancel);
            pool::submit(Box::new(move || done(job())));
        },
        Task::Line(prompt) => input::submit_line(prompt, cancel, done),
        Task::Key => input::submit_key(cancel, done),
    }
}

/// 同步执行 I/O 任务; 没有调度器时走此路径.
pub(crate) fn run_task(task: Task) -> Result<Value, Error> {
    match task {
        Task::Blocking(job) => job(),
        Task::Line(prompt) => input::read_line(&prompt, &AtomicBool::new(false)),
        Task::Key => input::read_key(&AtomicBool::new(false)),
    }
}

/// I/O 任务构造器; 注册表直接持有, 不再按名字二次分派.
pub(crate) type Builder = fn(&[Value], Option<Span>) -> Result<Task, Error>;

pub(crate) fn read_line_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let prompt = string_arg(args, 0, "ReadLine 的提示")?;
    Ok(Task::Line(prompt.to_string()))
}

pub(crate) fn read_key_task(_: &[Value], _: Option<Span>) -> Result<Task, Error> {
    Ok(Task::Key)
}

pub(crate) fn read_file_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "ReadFile 的路径")?;
    Ok(Task::blocking(move || read_file(&path)))
}

pub(crate) fn read_file_bytes_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "ReadFileBytes 的路径")?;
    Ok(Task::blocking(move || read_file_bytes(&path)))
}

pub(crate) fn write_file_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "WriteFile 的路径")?;
    let contents = string_arg(args, 1, "WriteFile 的内容")?;
    Ok(Task::blocking(move || write_file(&path, &contents)))
}

pub(crate) fn write_file_bytes_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "WriteFileBytes 的路径")?;
    let bytes = bytes_arg(args, 1)?;
    Ok(Task::blocking(move || write_file_bytes(&path, &bytes)))
}

pub(crate) fn append_file_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "AppendFile 的路径")?;
    let contents = string_arg(args, 1, "AppendFile 的内容")?;
    Ok(Task::blocking(move || append_file(&path, contents.as_bytes())))
}

pub(crate) fn append_file_bytes_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "AppendFileBytes 的路径")?;
    let bytes = bytes_arg(args, 1)?;
    Ok(Task::blocking(move || append_file(&path, &bytes)))
}

pub(crate) fn list_directory_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "ListDirectory 的路径")?;
    Ok(Task::blocking(move || list_directory(&path)))
}

pub(crate) fn create_directory_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "CreateDirectory 的路径")?;
    Ok(Task::blocking(move || create_directory(&path, false)))
}

pub(crate) fn create_directory_all_task(
    args: &[Value],
    _: Option<Span>,
) -> Result<Task, Error> {
    let path = string_arg(args, 0, "CreateDirectoryAll 的路径")?;
    Ok(Task::blocking(move || create_directory(&path, true)))
}

pub(crate) fn remove_directory_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "RemoveDirectory 的路径")?;
    Ok(Task::blocking(move || remove_directory(&path)))
}

pub(crate) fn remove_file_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "RemoveFile 的路径")?;
    Ok(Task::blocking(move || remove_file(&path)))
}

pub(crate) fn rename_path_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let from = string_arg(args, 0, "RenamePath 的原路径")?;
    let to = string_arg(args, 1, "RenamePath 的新路径")?;
    Ok(Task::blocking(move || rename_path(&from, &to)))
}

pub(crate) fn copy_file_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let from = string_arg(args, 0, "CopyFile 的原路径")?;
    let to = string_arg(args, 1, "CopyFile 的新路径")?;
    Ok(Task::blocking(move || copy_file(&from, &to)))
}

pub(crate) fn is_path_exists_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "IsPathExists 的路径")?;
    Ok(Task::blocking(move || {
        Ok(Value::Bool(std::fs::metadata(path.as_ref()).is_ok()))
    }))
}

pub(crate) fn is_file_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "IsFile 的路径")?;
    Ok(Task::blocking(move || {
        Ok(Value::Bool(
            std::fs::metadata(path.as_ref()).is_ok_and(|metadata| metadata.is_file()),
        ))
    }))
}

pub(crate) fn is_directory_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "IsDirectory 的路径")?;
    Ok(Task::blocking(move || {
        Ok(Value::Bool(
            std::fs::metadata(path.as_ref()).is_ok_and(|metadata| metadata.is_dir()),
        ))
    }))
}

pub(crate) fn get_current_directory_task(_: &[Value], _: Option<Span>) -> Result<Task, Error> {
    Ok(Task::blocking(get_current_directory))
}

pub(crate) fn open_read_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "OpenRead 的路径")?;
    Ok(Task::blocking(move || open_read(&path)))
}

pub(crate) fn open_write_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "OpenWrite 的路径")?;
    Ok(Task::blocking(move || open_write(&path, false)))
}

pub(crate) fn open_append_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "OpenAppend 的路径")?;
    Ok(Task::blocking(move || open_write(&path, true)))
}

pub(crate) fn close_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let handle = file_arg(args, 0, "Close 的句柄")?;
    Ok(Task::blocking(move || close(&handle)))
}

pub(crate) fn flush_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let handle = file_arg(args, 0, "Flush 的句柄")?;
    Ok(Task::blocking(move || flush(&handle)))
}

pub(crate) fn read_line_from_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let handle = file_arg(args, 0, "ReadLineFrom 的句柄")?;
    Ok(Task::blocking(move || read_line_from(&handle)))
}

pub(crate) fn read_chunk_task(args: &[Value], span: Option<Span>) -> Result<Task, Error> {
    let handle = file_arg(args, 0, "ReadChunk 的句柄")?;
    let max = count_arg(args, 1, span)?;
    Ok(Task::blocking(move || read_chunk(&handle, max)))
}

pub(crate) fn write_string_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let handle = file_arg(args, 0, "WriteString 的句柄")?;
    let text = string_arg(args, 1, "WriteString 的内容")?;
    Ok(Task::blocking(move || write_string(&handle, &text)))
}

pub(crate) fn write_bytes_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let handle = file_arg(args, 0, "WriteBytes 的句柄")?;
    let bytes = bytes_arg(args, 1)?;
    Ok(Task::blocking(move || write_bytes(&handle, &bytes)))
}

/// 把按键转成 `KeyEvent` 值; 未建模的按键返回 None.
///
/// Ctrl 与 Alt 同时按下时按 Ctrl 处理: 变体表不区分组合修饰键.
fn key_value(key: crossterm::event::KeyEvent) -> Option<Value> {
    use crossterm::event::{KeyCode, KeyModifiers};

    let ty = crate::symbol::intern("KeyEvent");
    let empty = |variant| data_value0(ty, crate::symbol::intern(variant));
    let value = match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            data_value(ty, crate::symbol::intern("Ctrl"), [Value::Char(c)])
        },
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::ALT) => {
            data_value(ty, crate::symbol::intern("Alt"), [Value::Char(c)])
        },
        KeyCode::Char(c) => data_value(ty, crate::symbol::intern("Char"), [Value::Char(c)]),
        KeyCode::Enter => empty("Enter"),
        KeyCode::Esc => empty("Escape"),
        KeyCode::Backspace => empty("Backspace"),
        KeyCode::Tab => empty("Tab"),
        KeyCode::BackTab => empty("BackTab"),
        KeyCode::Up => empty("Up"),
        KeyCode::Down => empty("Down"),
        KeyCode::Left => empty("Left"),
        KeyCode::Right => empty("Right"),
        KeyCode::Home => empty("Home"),
        KeyCode::End => empty("End"),
        KeyCode::PageUp => empty("PageUp"),
        KeyCode::PageDown => empty("PageDown"),
        KeyCode::Delete => empty("Delete"),
        KeyCode::Insert => empty("Insert"),
        KeyCode::F(number) => data_value(ty, crate::symbol::intern("F"), [Value::U8(number)]),
        _ => return None,
    };
    Some(value)
}

/// 把输出流错误转为运行时错误; 下游关闭管道时由宿主静默结束.
pub(super) fn output_error(error: &std::io::Error, action: &str) -> Error {
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        Error::BrokenPipe
    } else {
        Error::internal(format!("{action}失败: {error}"))
    }
}

pub(super) fn strip_line_ending(mut line: String) -> String {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    line
}

fn read_file(path: &str) -> Result<Value, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(result_ok(Value::Str(Arc::from(text)))),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            Ok(result_err(io_error("InvalidUtf8", path)))
        },
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn write_file(path: &str, contents: &str) -> Result<Value, Error> {
    match std::fs::write(path, contents) {
        Ok(()) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn read_file_bytes(path: &str) -> Result<Value, Error> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(result_ok(Value::List(Arc::new(crate::value::List::u8s(bytes))))),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn write_file_bytes(path: &str, bytes: &[u8]) -> Result<Value, Error> {
    match std::fs::write(path, bytes) {
        Ok(()) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn append_file(path: &str, bytes: &[u8]) -> Result<Value, Error> {
    let mut file = match OpenOptions::new().append(true).create(true).open(path) {
        Ok(file) => file,
        Err(error) => return Ok(result_err(io_error_for(&error, path))),
    };
    match file.write_all(bytes) {
        Ok(()) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn list_directory(path: &str) -> Result<Value, Error> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return Ok(result_err(io_error_for(&error, path))),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return Ok(result_err(io_error_for(&error, path))),
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Ok(result_err(io_error("Other", "目录条目名不是有效 UTF-8")));
        };
        names.push(name.to_string());
    }
    names.sort();
    Ok(result_ok(crate::value::list(
        names.into_iter().map(|name| Value::Str(Arc::from(name))),
    )))
}

fn create_directory(path: &str, recursive: bool) -> Result<Value, Error> {
    let result = if recursive {
        std::fs::create_dir_all(path)
    } else {
        std::fs::create_dir(path)
    };
    match result {
        Ok(()) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn remove_directory(path: &str) -> Result<Value, Error> {
    match std::fs::remove_dir(path) {
        Ok(()) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn remove_file(path: &str) -> Result<Value, Error> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn rename_path(from: &str, to: &str) -> Result<Value, Error> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for_pair(&error, from, to))),
    }
}

fn copy_file(from: &str, to: &str) -> Result<Value, Error> {
    match std::fs::copy(from, to) {
        Ok(_) => Ok(result_ok(Value::Nil)),
        Err(error) => Ok(result_err(io_error_for_pair(&error, from, to))),
    }
}

fn get_current_directory() -> Result<Value, Error> {
    match std::env::current_dir() {
        Ok(path) => Ok(result_ok(Value::Str(Arc::from(
            path.to_string_lossy().as_ref(),
        )))),
        Err(error) => Ok(result_err(io_error("Other", &error.to_string()))),
    }
}

fn open_read(path: &str) -> Result<Value, Error> {
    match File::open(path) {
        Ok(file) => Ok(result_ok(file_value(
            FileInner::Read(BufReader::new(file)),
            path,
        ))),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn open_write(path: &str, append: bool) -> Result<Value, Error> {
    let result = if append {
        OpenOptions::new().append(true).create(true).open(path)
    } else {
        File::create(path)
    };
    match result {
        Ok(file) => Ok(result_ok(file_value(
            FileInner::Write(BufWriter::new(file)),
            path,
        ))),
        Err(error) => Ok(result_err(io_error_for(&error, path))),
    }
}

fn close(handle: &FileHandle) -> Result<Value, Error> {
    let mut inner = handle.inner.lock();
    match inner.take() {
        None => Ok(result_err(io_error("InvalidHandle", ""))),
        Some(FileInner::Read(_)) => Ok(result_ok(Value::Nil)),
        Some(FileInner::Write(mut writer)) => match writer.flush() {
            Ok(()) => Ok(result_ok(Value::Nil)),
            Err(error) => Ok(result_err(io_error("Other", &error.to_string()))),
        },
    }
}

fn flush(handle: &FileHandle) -> Result<Value, Error> {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None => Ok(result_err(io_error("InvalidHandle", ""))),
        Some(FileInner::Read(_)) => Ok(result_ok(Value::Nil)),
        Some(FileInner::Write(writer)) => match writer.flush() {
            Ok(()) => Ok(result_ok(Value::Nil)),
            Err(error) => Ok(result_err(io_error("Other", &error.to_string()))),
        },
    }
}

fn read_line_from(handle: &FileHandle) -> Result<Value, Error> {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None | Some(FileInner::Write(_)) => Ok(result_err(io_error("InvalidHandle", ""))),
        Some(FileInner::Read(reader)) => {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => Ok(result_ok(option_none())),
                Ok(_) => Ok(result_ok(option_some(Value::Str(Arc::from(
                    strip_line_ending(line),
                ))))),
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    Ok(result_err(io_error("InvalidUtf8", &handle.path)))
                },
                Err(error) => Ok(result_err(io_error("Other", &error.to_string()))),
            }
        },
    }
}

fn read_chunk(handle: &FileHandle, max: usize) -> Result<Value, Error> {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None | Some(FileInner::Write(_)) => Ok(result_err(io_error("InvalidHandle", ""))),
        Some(FileInner::Read(reader)) => {
            // 按块上限读取而非预分配: 请求块再大, 实际内存只随文件内容增长.
            let limit = u64::try_from(max).unwrap_or(u64::MAX);
            let mut buffer = Vec::new();
            match reader.take(limit).read_to_end(&mut buffer) {
                Ok(0) => Ok(result_ok(option_none())),
                Ok(_) => {
                    let bytes = crate::value::List::u8s(buffer);
                    Ok(result_ok(option_some(Value::List(Arc::new(bytes)))))
                },
                Err(error) => Ok(result_err(io_error("Other", &error.to_string()))),
            }
        },
    }
}

fn write_string(handle: &FileHandle, text: &str) -> Result<Value, Error> {
    write_all(handle, text.as_bytes())
}

fn write_bytes(handle: &FileHandle, bytes: &[u8]) -> Result<Value, Error> {
    write_all(handle, bytes)
}

fn write_all(handle: &FileHandle, bytes: &[u8]) -> Result<Value, Error> {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None | Some(FileInner::Read(_)) => Ok(result_err(io_error("InvalidHandle", ""))),
        Some(FileInner::Write(writer)) => match writer.write_all(bytes) {
            Ok(()) => Ok(result_ok(Value::Nil)),
            Err(error) => Ok(result_err(io_error("Other", &error.to_string()))),
        },
    }
}

fn file_value(inner: FileInner, path: &str) -> Value {
    Value::File(Arc::new(FileHandle {
        inner: Mutex::new(Some(inner)),
        path: path.to_string(),
    }))
}

fn string_arg(args: &[Value], index: usize, what: &str) -> Result<Arc<str>, Error> {
    let Some(Value::Str(text)) = args.get(index) else {
        return Err(Error::internal(format!("{what}不是字符串")));
    };
    Ok(Arc::clone(text))
}

fn file_arg(args: &[Value], index: usize, what: &str) -> Result<Arc<FileHandle>, Error> {
    let Some(Value::File(handle)) = args.get(index) else {
        return Err(Error::internal(format!("{what}不是 File")));
    };
    Ok(Arc::clone(handle))
}

fn count_arg(args: &[Value], index: usize, span: Option<Span>) -> Result<usize, Error> {
    let Some(Value::U64(value)) = args.get(index) else {
        return Err(Error::internal("读取块大小不是 u64"));
    };
    if *value == 0 {
        return Err(Error::runtime(ErrorKind::CountOutOfRange, span));
    }
    usize::try_from(*value).map_err(|_| Error::runtime(ErrorKind::CountOutOfRange, span))
}

fn bytes_arg(args: &[Value], index: usize) -> Result<Vec<u8>, Error> {
    let Some(Value::List(list)) = args.get(index) else {
        return Err(Error::internal("WriteBytes 的内容不是字节列表"));
    };
    match &**list {
        crate::value::List::U8(items) => Ok(items.iter().copied().collect()),
        crate::value::List::Values(items) => {
            let mut bytes = Vec::with_capacity(items.len());
            for item in items {
                let Value::U8(byte) = item else {
                    return Err(Error::internal("WriteBytes 的列表元素不是 u8"));
                };
                bytes.push(*byte);
            }
            Ok(bytes)
        },
        crate::value::List::I64(_)
        | crate::value::List::U64(_)
        | crate::value::List::F64(_)
        | crate::value::List::Char(_)
        | crate::value::List::Bool(_) => Err(Error::internal("WriteBytes 的内容不是字节列表")),
    }
}

fn io_error_for(error: &std::io::Error, path: &str) -> Value {
    let variant = match error.kind() {
        std::io::ErrorKind::NotFound => "NotFound",
        std::io::ErrorKind::PermissionDenied => "PermissionDenied",
        std::io::ErrorKind::AlreadyExists => "AlreadyExists",
        std::io::ErrorKind::NotADirectory => "NotADirectory",
        std::io::ErrorKind::IsADirectory => "IsADirectory",
        std::io::ErrorKind::DirectoryNotEmpty => "DirectoryNotEmpty",
        _ => "Other",
    };
    let field = if variant == "Other" {
        error.to_string()
    } else {
        path.to_string()
    };
    io_error(variant, &field)
}

/// 双路径操作的错误归属: 目标已存在时问题在新路径, 其余按原路径报告.
fn io_error_for_pair(error: &std::io::Error, from: &str, to: &str) -> Value {
    let path = if error.kind() == std::io::ErrorKind::AlreadyExists {
        to
    } else {
        from
    };
    io_error_for(error, path)
}

/// 无载荷的 `IOError` 变体; 这些变体没有路径或消息可报.
const IO_ERROR_PLAIN: &[&str] = &["InvalidHandle", "NotATerminal", "Interrupted"];

fn io_error(variant: &str, field: &str) -> Value {
    let ty = crate::symbol::intern("IOError");
    let name = crate::symbol::intern(variant);
    if IO_ERROR_PLAIN.contains(&variant) {
        data_value0(ty, name)
    } else {
        data_value(ty, name, [Value::Str(Arc::from(field))])
    }
}

/// 空文件句柄: 处于关闭态, 任何读写都报 `IOError/InvalidHandle`.
///
/// 供必须给出 `File` 值而手上没有可用句柄的场合使用, 例如 `ResultGet`
/// 的默认值.
pub(crate) fn closed_file() -> Value {
    Value::File(Arc::new(FileHandle {
        inner: Mutex::new(None),
        path: String::new(),
    }))
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{io_error, key_value};
    use crate::symbol::resolve;
    use crate::value::Value;

    /// 取变体名与字段; 未建模的按键返回 None.
    fn mapped(code: KeyCode, modifiers: KeyModifiers) -> Option<(String, Vec<Value>)> {
        let value = key_value(KeyEvent::new(code, modifiers))?;
        let Value::Data(data) = value else {
            panic!("KeyEvent 必须是 Data");
        };
        Some((resolve(data.variant).to_string(), data.fields.to_vec()))
    }

    #[test]
    fn key_events_map_to_values() {
        // 修饰键折叠进变体; Ctrl 优先于 Alt.
        let (variant, fields) = mapped(KeyCode::Char('a'), KeyModifiers::CONTROL).unwrap();
        assert_eq!(variant, "Ctrl");
        assert!(matches!(fields.as_slice(), [Value::Char('a')]));

        let (variant, fields) = mapped(KeyCode::Char('x'), KeyModifiers::ALT).unwrap();
        assert_eq!(variant, "Alt");
        assert!(matches!(fields.as_slice(), [Value::Char('x')]));

        let (variant, _) =
            mapped(KeyCode::Char('c'), KeyModifiers::CONTROL | KeyModifiers::ALT).unwrap();
        assert_eq!(variant, "Ctrl");

        // 功能键与无载荷按键.
        let (variant, fields) = mapped(KeyCode::F(12), KeyModifiers::NONE).unwrap();
        assert_eq!(variant, "F");
        assert!(matches!(fields.as_slice(), [Value::U8(12)]));

        let (variant, fields) = mapped(KeyCode::BackTab, KeyModifiers::SHIFT).unwrap();
        assert_eq!(variant, "BackTab");
        assert!(fields.is_empty());

        // 未建模的按键不进入语言层.
        assert!(mapped(KeyCode::CapsLock, KeyModifiers::NONE).is_none());
    }

    #[test]
    fn plain_io_errors_have_no_payload() {
        for variant in ["NotATerminal", "Interrupted"] {
            let Value::Data(data) = io_error(variant, "") else {
                panic!("IOError 必须是 Data");
            };
            assert!(data.fields.is_empty(), "{variant} 不应携带载荷");
        }
    }
}
