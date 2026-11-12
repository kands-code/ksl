//! 文件与目录操作.
//!
//! 任务在阻塞线程池执行, 调用方挂起等待结果.
//! 可恢复失败以 `FileError` 值报告; 句柄关闭后任何读写都报 `InvalidHandle`.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_frontend::error::{ErrorKind, RuntimeError as Error};
use parking_lot::Mutex;

use super::{Task, string_arg, strip_line_ending};
use crate::value::{
    Value, data_value, data_value0, option_none, option_some, result_err, result_ok,
};

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
        Value::Bool(std::fs::metadata(path.as_str()).is_ok())
    }))
}

pub(crate) fn is_file_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "IsFile 的路径")?;
    Ok(Task::blocking(move || {
        Value::Bool(std::fs::metadata(path.as_str()).is_ok_and(|metadata| metadata.is_file()))
    }))
}

pub(crate) fn is_directory_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let path = string_arg(args, 0, "IsDirectory 的路径")?;
    Ok(Task::blocking(move || {
        Value::Bool(std::fs::metadata(path.as_str()).is_ok_and(|metadata| metadata.is_dir()))
    }))
}

/// 构造器签名由注册表统一为 `Result`, 本函数不会失败.
#[allow(clippy::unnecessary_wraps)]
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

fn read_file(path: &str) -> Value {
    match std::fs::read_to_string(path) {
        Ok(text) => result_ok(Value::Str(ArcStr::from(text))),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            result_err(file_error("InvalidUtf8", path))
        },
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn write_file(path: &str, contents: &str) -> Value {
    match std::fs::write(path, contents) {
        Ok(()) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn read_file_bytes(path: &str) -> Value {
    match std::fs::read(path) {
        Ok(bytes) => result_ok(Value::List(Arc::new(crate::value::List::u8s(bytes)))),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn write_file_bytes(path: &str, bytes: &[u8]) -> Value {
    match std::fs::write(path, bytes) {
        Ok(()) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn append_file(path: &str, bytes: &[u8]) -> Value {
    let mut file = match OpenOptions::new().append(true).create(true).open(path) {
        Ok(file) => file,
        Err(error) => return result_err(file_error_for(&error, path)),
    };
    match file.write_all(bytes) {
        Ok(()) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn list_directory(path: &str) -> Value {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => return result_err(file_error_for(&error, path)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => return result_err(file_error_for(&error, path)),
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return result_err(file_error("Other", "目录条目名不是 UTF-8"));
        };
        names.push(name.to_string());
    }
    names.sort();
    result_ok(crate::value::list(
        names.into_iter().map(|name| Value::Str(ArcStr::from(name))),
    ))
}

fn create_directory(path: &str, recursive: bool) -> Value {
    let result = if recursive {
        std::fs::create_dir_all(path)
    } else {
        std::fs::create_dir(path)
    };
    match result {
        Ok(()) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn remove_directory(path: &str) -> Value {
    match std::fs::remove_dir(path) {
        Ok(()) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn remove_file(path: &str) -> Value {
    match std::fs::remove_file(path) {
        Ok(()) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

fn rename_path(from: &str, to: &str) -> Value {
    match std::fs::rename(from, to) {
        Ok(()) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for_pair(&error, from, to)),
    }
}

fn copy_file(from: &str, to: &str) -> Value {
    match std::fs::copy(from, to) {
        Ok(_) => result_ok(Value::Nil),
        Err(error) => result_err(file_error_for_pair(&error, from, to)),
    }
}

fn get_current_directory() -> Value {
    match std::env::current_dir() {
        Ok(path) => result_ok(Value::Str(ArcStr::from(path.to_string_lossy().as_ref()))),
        Err(error) => result_err(file_error("Other", &error.to_string())),
    }
}

pub(super) fn open_read(path: &str) -> Value {
    match File::open(path) {
        Ok(file) => result_ok(file_value(FileInner::Read(BufReader::new(file)), path)),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

pub(super) fn open_write(path: &str, append: bool) -> Value {
    let result = if append {
        OpenOptions::new().append(true).create(true).open(path)
    } else {
        File::create(path)
    };
    match result {
        Ok(file) => result_ok(file_value(FileInner::Write(BufWriter::new(file)), path)),
        Err(error) => result_err(file_error_for(&error, path)),
    }
}

pub(super) fn close(handle: &FileHandle) -> Value {
    let mut inner = handle.inner.lock();
    match inner.take() {
        None => result_err(file_error("InvalidHandle", "")),
        Some(FileInner::Read(_)) => result_ok(Value::Nil),
        Some(FileInner::Write(mut writer)) => match writer.flush() {
            Ok(()) => result_ok(Value::Nil),
            Err(error) => result_err(file_error("Other", &error.to_string())),
        },
    }
}

fn flush(handle: &FileHandle) -> Value {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None => result_err(file_error("InvalidHandle", "")),
        Some(FileInner::Read(_)) => result_ok(Value::Nil),
        Some(FileInner::Write(writer)) => match writer.flush() {
            Ok(()) => result_ok(Value::Nil),
            Err(error) => result_err(file_error("Other", &error.to_string())),
        },
    }
}

pub(super) fn read_line_from(handle: &FileHandle) -> Value {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None => result_err(file_error("InvalidHandle", "")),
        Some(FileInner::Write(_)) => result_err(file_error("NotReadable", "")),
        Some(FileInner::Read(reader)) => {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => result_ok(option_none()),
                Ok(_) => {
                    result_ok(option_some(Value::Str(ArcStr::from(strip_line_ending(line)))))
                },
                Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                    result_err(file_error("InvalidUtf8", &handle.path))
                },
                Err(error) => result_err(file_error("Other", &error.to_string())),
            }
        },
    }
}

fn read_chunk(handle: &FileHandle, max: usize) -> Value {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None => result_err(file_error("InvalidHandle", "")),
        Some(FileInner::Write(_)) => result_err(file_error("NotReadable", "")),
        Some(FileInner::Read(reader)) => {
            // 按块上限读取而非预分配: 请求块再大, 实际内存只随文件内容增长.
            let limit = u64::try_from(max).unwrap_or(u64::MAX);
            let mut buffer = Vec::new();
            match reader.take(limit).read_to_end(&mut buffer) {
                Ok(0) => result_ok(option_none()),
                Ok(_) => {
                    let bytes = crate::value::List::u8s(buffer);
                    result_ok(option_some(Value::List(Arc::new(bytes))))
                },
                Err(error) => result_err(file_error("Other", &error.to_string())),
            }
        },
    }
}

fn write_string(handle: &FileHandle, text: &str) -> Value {
    write_all(handle, text.as_bytes())
}

fn write_bytes(handle: &FileHandle, bytes: &[u8]) -> Value {
    write_all(handle, bytes)
}

pub(super) fn write_all(handle: &FileHandle, bytes: &[u8]) -> Value {
    let mut inner = handle.inner.lock();
    match inner.as_mut() {
        None => result_err(file_error("InvalidHandle", "")),
        Some(FileInner::Read(_)) => result_err(file_error("NotWritable", "")),
        Some(FileInner::Write(writer)) => match writer.write_all(bytes) {
            Ok(()) => result_ok(Value::Nil),
            Err(error) => result_err(file_error("Other", &error.to_string())),
        },
    }
}

fn file_value(inner: FileInner, path: &str) -> Value {
    Value::File(Arc::new(FileHandle {
        inner: Mutex::new(Some(inner)),
        path: path.to_string(),
    }))
}

fn file_arg(args: &[Value], index: usize, what: &str) -> Result<Arc<FileHandle>, Error> {
    let Some(Value::File(handle)) = args.get(index) else {
        return Err(Error::internal(format!("{what} 不是 File")));
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

fn file_error_for(error: &std::io::Error, path: &str) -> Value {
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
    file_error(variant, &field)
}

/// 双路径操作的错误归属: 目标已存在时问题在新路径, 其余按原路径报告.
fn file_error_for_pair(error: &std::io::Error, from: &str, to: &str) -> Value {
    let path = if error.kind() == std::io::ErrorKind::AlreadyExists {
        to
    } else {
        from
    };
    file_error_for(error, path)
}

/// 无载荷的 `FileError` 变体.
const FILE_ERROR_PLAIN: &[&str] = &["InvalidHandle", "NotReadable", "NotWritable"];

pub(super) fn file_error(variant: &str, field: &str) -> Value {
    let ty = ksl_base::symbol::intern("FileError");
    let name = ksl_base::symbol::intern(variant);
    if FILE_ERROR_PLAIN.contains(&variant) {
        data_value0(ty, name)
    } else {
        data_value(ty, name, [Value::Str(ArcStr::from(field))])
    }
}

/// 空文件句柄: 处于关闭态, 任何读写都报 `FileError/InvalidHandle`.
///
/// 供必须给出 `File` 值而手上没有可用句柄的场合使用, 例如 `ResultGet` 的默认值.
pub(crate) fn closed_file() -> Value {
    Value::File(Arc::new(FileHandle {
        inner: Mutex::new(None),
        path: String::new(),
    }))
}
