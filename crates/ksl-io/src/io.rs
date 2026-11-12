//! 标准 I/O 操作.
//!
//! 文件, 子进程与 I/O 类插件调用共用一个阻塞线程池, 网络使用 mio 事件轮询.
//! CPU 类插件调用使用独立线程池, 不占用文件与子进程线程.
//! 终端与标准流保留专用服务线程.
//! 经调度器执行时由抽象机提交任务, 任务挂起, 完成后唤醒, 不阻塞其他 Actor.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::args::string_arg;
use ksl_value::root::RootSet;
use ksl_value::value::{Value, data_value, data_value0};
use parking_lot::Mutex;

mod command;
mod display;
mod file;
mod input;
pub mod net;
mod pool;
mod service;
mod stdio;

pub use command::{
    CommandOutput, command_output_status, command_output_stderr, command_output_stdout,
    run_command_task,
};
pub use file::{
    FileHandle, append_file_bytes_task, append_file_task, close_task, closed_file,
    copy_file_task, create_directory_all_task, create_directory_task, flush_task,
    get_current_directory_task, is_directory_task, is_file_task, is_path_exists_task,
    list_directory_task, open_append_task, open_read_task, open_write_task, read_chunk_task,
    read_file_bytes_task, read_file_task, read_line_from_task, remove_directory_task,
    remove_file_task, rename_path_task, write_bytes_task, write_file_bytes_task,
    write_file_task, write_string_task,
};
pub use net::{SocketHandle, TcpListenerHandle};

/// I/O 完成回调; 服务, 工作线程与网络引擎只调用一次.
pub type Done = Box<dyn FnOnce(Result<Value, Error>) + Send + 'static>;

/// 在途操作守卫; 回调被调用或丢弃时解除取消登记.
struct IoOpGuard {
    cancellations: Arc<Mutex<HashMap<u64, CancelHandle>>>,
    id: u64,
}

impl Drop for IoOpGuard {
    fn drop(&mut self) {
        self.cancellations.lock().remove(&self.id);
    }
}

/// 一个可取消的 I/O 操作.
enum CancelHandle {
    /// 终端读取与线程池任务的协作式取消.
    Flag(Arc<AtomicBool>),
    /// 网络操作; 取消时置位标记并通知网络引擎.
    Reactor(net::engine::EngineHandle, u64, Arc<AtomicBool>),
    /// 子进程任务; 取消时置位标记并终止已启动的子进程.
    Process(Arc<command::ProcessControl>, Arc<AtomicBool>),
}

/// 可阻塞的计算或外来调用; 交给阻塞线程池执行.
pub type ComputeJob = Box<dyn FnOnce() -> Result<Value, Error> + Send + 'static>;

/// 待执行的 I/O 任务.
/// 终端读取走输入线程, 文件与子进程走 I/O 阻塞池, CPU 调用走独立线程池, 网络走网络引擎.
pub enum Task {
    /// 标准输入行读取.
    Line {
        prompt: String,
    },
    Key,
    /// 进入或退出终端原始模式.
    RawMode {
        enable: bool,
    },
    /// 网络操作; 交给网络引擎.
    ///
    /// `roots` 保存操作完成前必须存活的目标句柄.
    Net {
        op: net::engine::Op,
        roots: RootSet,
    },
    /// 一次性子进程; 交给 I/O 线程池, 取消时终止子进程.
    Process(command::ProcessTask),
    /// 阻塞在外部事件上的工作, 交给 I/O 线程池.
    Blocking(ComputeJob),
    /// CPU 密集工作, 交给独立线程池.
    Compute(ComputeJob),
    /// 立即完成的 I/O 结果; 参数校验失败等路径直接返回.
    Ready(Value),
}

impl Task {
    /// 把阻塞操作交给阻塞线程池.
    fn blocking(job: impl FnOnce() -> Value + Send + 'static) -> Self {
        Self::Blocking(Box::new(move || Ok(job())))
    }

    /// 取走操作期间需要保持存活的值.
    pub fn take_roots(&mut self) -> RootSet {
        match self {
            Self::Net { roots, .. } => std::mem::take(roots),
            _ => RootSet::new(),
        }
    }
}

/// I/O 运行时配置.
#[derive(Debug, Clone, Copy, Default)]
pub struct IoConfig {
    /// 阻塞 I/O 工作线程数; None 时按可用并行度取定并限制在 4 到 16.
    pub io_workers: Option<usize>,
    /// CPU 工作线程数; None 时等于可用并行度.
    pub cpu_workers: Option<usize>,
}

/// 一次语言运行时拥有的 I/O 服务.
///
/// 线程池与网络事件线程按需启动, 生命周期与 `IoRuntime` 绑定.
pub struct IoRuntime {
    config: IoConfig,
    /// 终端与标准流共享状态.
    terminal: Arc<display::TerminalShared>,
    output: OnceLock<Arc<service::OutputService>>,
    input: OnceLock<Arc<input::InputService>>,
    /// 网络引擎; 持有 TCP 监听器与套接字. 启动失败时保留错误文本.
    net: OnceLock<Result<net::engine::EngineHandle, String>>,
    /// 阻塞 I/O 线程池; 执行文件, 子进程与 I/O 类外来调用.
    blocking: OnceLock<pool::ThreadPool>,
    /// CPU 线程池; 只执行 CPU 类外来调用.
    compute: OnceLock<pool::ThreadPool>,
    /// 可取消的 I/O 操作.
    cancellations: Arc<Mutex<HashMap<u64, CancelHandle>>>,
    /// 下一个 I/O 操作编号; 与取消表共享生命周期, Host reset 不重置.
    next_op: AtomicU64,
}

impl IoRuntime {
    /// 以给定配置建立空 I/O 运行时.
    #[must_use]
    pub fn new(config: IoConfig) -> Self {
        Self {
            config,
            terminal: Arc::new(display::TerminalShared::new()),
            output: OnceLock::new(),
            input: OnceLock::new(),
            net: OnceLock::new(),
            blocking: OnceLock::new(),
            compute: OnceLock::new(),
            cancellations: Arc::new(Mutex::new(HashMap::new())),
            next_op: AtomicU64::new(1),
        }
    }

    /// 分配 I/O 操作编号; 编号在 `IoRuntime` 生命周期内唯一.
    pub fn allocate_op_id(&self) -> u64 {
        self.next_op.fetch_add(1, Ordering::Relaxed)
    }

    fn engine(&self) -> Result<&net::engine::EngineHandle, Error> {
        self.net
            .get_or_init(|| {
                net::engine::EngineHandle::start()
                    .map_err(|error| format!("无法启动网络引擎: {error}"))
            })
            .as_ref()
            .map_err(|message| Error::internal(message.clone()))
    }

    fn blocking_pool(&self) -> &pool::ThreadPool {
        self.blocking.get_or_init(|| {
            let workers = self.config.io_workers.unwrap_or_else(io_workers_default);
            pool::ThreadPool::start(workers, "ksl-io")
        })
    }

    fn compute_pool(&self) -> &pool::ThreadPool {
        self.compute.get_or_init(|| {
            let workers = self.config.cpu_workers.unwrap_or_else(|| {
                std::thread::available_parallelism().map_or(1, std::num::NonZero::get)
            });
            pool::ThreadPool::start(workers, "ksl-cpu")
        })
    }

    fn output(&self) -> &Arc<service::OutputService> {
        self.output
            .get_or_init(|| Arc::new(service::OutputService::start(Arc::clone(&self.terminal))))
    }

    fn input(&self) -> &Arc<input::InputService> {
        self.input
            .get_or_init(|| Arc::new(input::InputService::start(&self.terminal, self.output())))
    }

    /// 写出标准输出; 输出服务按需启动.
    ///
    /// # Errors
    ///
    /// 输出服务已停止或写出失败时返回运行时错误.
    pub fn print(&self, text: &str) -> Result<(), Error> {
        self.output().print(text)
    }

    /// 写出标准错误.
    ///
    /// # Errors
    ///
    /// 输出服务已停止或写出失败时返回运行时错误.
    pub fn print_error(&self, text: &str) -> Result<(), Error> {
        self.output().print_error(text)
    }

    /// 等待已排队的输出全部完成.
    ///
    /// # Errors
    ///
    /// 输出服务返回写出错误时透传.
    pub fn flush_output(&self) -> Result<(), Error> {
        self.output.get().map_or(Ok(()), |output| output.flush())
    }

    /// 释放某个终端持有者; 持有者终止时调用.
    pub fn release_terminal_holder(&self, holder: crate::terminal::state::RawModeHolder) {
        if self.terminal.flags().held()
            && let Some(input) = self.input.get()
        {
            input.submit_release_holder(holder);
        }
    }

    /// 恢复终端状态; 程序执行边界调用.
    pub fn restore_terminal(&self) {
        if !self.terminal.flags().raw_active() && !self.terminal.flags().held() {
            return;
        }
        if let Some(input) = self.input.get() {
            input.restore_terminal(&self.terminal);
            return;
        }
        if self.terminal.terminal().disable_raw_mode().is_ok() {
            self.terminal.flags().clear_raw();
            self.terminal.flags().set_held(false);
        }
    }

    /// 请求输入线程开启终端会话.
    ///
    /// # Errors
    ///
    /// 输出冲刷失败或输入线程已停止.
    pub fn open_session(
        &self,
        bracketed_paste: bool,
    ) -> Result<crate::terminal::Session, Error> {
        self.flush_output()?;
        self.input()
            .open_session(self.terminal.terminal().clone(), bracketed_paste)
    }

    /// 取走终端模式中已消费但未交付的一行.
    #[must_use]
    pub fn take_buffered_line(&self) -> Option<String> {
        self.input.get()?.take_buffered_line()
    }

    /// 取走终端模式中已编辑但未交付的文本.
    #[must_use]
    pub fn take_buffered_text(&self) -> Option<String> {
        self.input.get()?.take_buffered_text()
    }

    pub fn begin_stdout_tracking(&self) {
        let _ = self.flush_output();
        self.terminal.stdio().begin_stdout_tracking();
    }

    #[must_use]
    pub fn take_stdout_line_start(&self) -> bool {
        let _ = self.flush_output();
        self.terminal.stdio().take_stdout_line_start()
    }

    #[must_use]
    pub fn take_stdout_wrote(&self) -> bool {
        let _ = self.flush_output();
        self.terminal.stdio().take_stdout_wrote()
    }

    pub fn begin_stderr_tracking(&self) {
        let _ = self.flush_output();
        self.terminal.stdio().begin_stderr_tracking();
    }

    #[must_use]
    pub fn take_stderr_wrote(&self) -> bool {
        let _ = self.flush_output();
        self.terminal.stdio().take_stderr_wrote()
    }

    /// 提交 I/O 任务; 完成回调把结果交给调度器.
    ///
    /// 终端读取与网络操作可经 `cancel_task` 取消. 文件操作无法中断, 完成后结果由调度器丢弃.
    pub fn submit_task(
        &self,
        task: Task,
        op_id: u64,
        holder: crate::terminal::state::RawModeHolder,
        done: impl FnOnce(Result<Value, Error>) + Send + 'static,
    ) {
        let guard = IoOpGuard {
            cancellations: Arc::clone(&self.cancellations),
            id: op_id,
        };
        let done: Done = Box::new(move |result| {
            done(result);
            drop(guard);
        });
        match task {
            Task::Net { op, .. } => {
                // 先登记取消入口, 再提交; 引擎按标记跳过已取消的操作.
                let cancel = Arc::new(AtomicBool::new(false));
                let engine = match self.engine() {
                    Ok(engine) => engine.clone(),
                    Err(error) => {
                        done(Err(error));
                        return;
                    },
                };
                self.cancellations.lock().insert(
                    op_id,
                    CancelHandle::Reactor(engine.clone(), op_id, Arc::clone(&cancel)),
                );
                engine.submit(op_id, op, cancel, done);
            },
            Task::Process(task) => {
                // 取消置位标记并终止子进程; 已读入的输出由协调器丢弃.
                let cancel = Arc::new(AtomicBool::new(false));
                let control = Arc::clone(task.control());
                self.cancellations
                    .lock()
                    .insert(op_id, CancelHandle::Process(control, Arc::clone(&cancel)));
                let job: ComputeJob = Box::new(move || Ok(task.run()));
                self.blocking_pool().submit(job, cancel, done);
            },
            Task::Blocking(job) => {
                // 中止只阻止任务开始或丢弃结果; 已开始的阻塞工作自行结束.
                let cancel = self.cancellation_flag(op_id);
                self.blocking_pool().submit(job, cancel, done);
            },
            Task::Compute(job) => {
                let cancel = self.cancellation_flag(op_id);
                self.compute_pool().submit(job, cancel, done);
            },
            // 输入请求与输出共用服务邮箱.
            // 服务把阻塞读取交给输入线程, 自身继续处理后续输出请求.
            Task::Line { prompt } => {
                let cancel = self.cancellation_flag(op_id);
                self.input().submit_line(prompt, cancel, done);
            },
            Task::Key => {
                let cancel = self.cancellation_flag(op_id);
                self.input().submit_key(cancel, done);
            },
            Task::RawMode { enable } => {
                let cancel = self.cancellation_flag(op_id);
                self.input().submit_raw_mode(enable, holder, cancel, done);
            },
            Task::Ready(value) => done(Ok(value)),
        }
    }

    fn cancellation_flag(&self, op_id: u64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.cancellations
            .lock()
            .insert(op_id, CancelHandle::Flag(Arc::clone(&flag)));
        flag
    }

    /// 取消一个 I/O 操作; 协作式任务置位标记, 网络任务直接中止.
    pub fn cancel_task(&self, op_id: u64) {
        let cancel = self.cancellations.lock().remove(&op_id);
        if let Some(cancel) = cancel {
            match cancel {
                CancelHandle::Flag(flag) => flag.store(true, Ordering::Relaxed),
                CancelHandle::Reactor(engine, op_id, flag) => {
                    flag.store(true, Ordering::Relaxed);
                    engine.cancel(op_id);
                },
                CancelHandle::Process(control, flag) => {
                    flag.store(true, Ordering::Relaxed);
                    control.cancel();
                },
            }
        }
    }
}

impl Drop for IoRuntime {
    fn drop(&mut self) {
        if let Some(Ok(engine)) = self.net.get() {
            engine.shutdown();
        }
    }
}

/// I/O 任务构造器; 注册表直接持有, 不按名字二次分派.
///
/// 可恢复的语言层失败以 `Task::Ready` 携带 `Result` 值返回.
/// `Err` 只用于内部形态错误.
pub type Builder = fn(&[Value], Option<Span>) -> Result<Task, Error>;

/// 构造标准输入行读取任务.
///
/// # Errors
///
/// 实参缺失, 或实参不是 string 时返回内部错误.
pub fn read_line_task(args: &[Value], _: Option<Span>) -> Result<Task, Error> {
    let prompt = string_arg(args, 0, "ReadLine 的提示")?;
    Ok(Task::Line {
        prompt: prompt.to_string(),
    })
}

/// 构造单键读取任务.
///
/// # Errors
///
/// 本函数不会失败.
pub fn read_key_task(_: &[Value], _: Option<Span>) -> Result<Task, Error> {
    Ok(Task::Key)
}

/// 进入终端原始模式.
///
/// 标准输入不是终端时返回 `InputError/NotATerminal`.
///
/// # Errors
///
/// 本函数不会失败; 终端状态错误由任务以 `Result` 值报告.
pub fn enter_raw_mode_task(_: &[Value], _: Option<Span>) -> Result<Task, Error> {
    Ok(Task::RawMode { enable: true })
}

/// 退出终端原始模式.
///
/// 不在原始模式时成功, 即空操作.
///
/// # Errors
///
/// 本函数不会失败.
pub fn exit_raw_mode_task(_: &[Value], _: Option<Span>) -> Result<Task, Error> {
    Ok(Task::RawMode { enable: false })
}

/// 把按键转成 `KeyEvent` 值; 未建模的按键返回 None.
///
/// Ctrl 与 Alt 同时按下时按 Ctrl 处理: 变体表不区分组合修饰键.
fn key_value(key: crate::terminal::KeyEvent) -> Option<Value> {
    use crate::terminal::{KeyCode, KeyModifiers};

    let ty = ksl_base::name::Name::new("KeyEvent");
    let empty = |variant| data_value0(ty.clone(), ksl_base::name::Name::new(variant));
    let value = match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
            data_value(ty, ksl_base::name::Name::new("Ctrl"), [Value::Char(c)])
        },
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::ALT) => {
            data_value(ty, ksl_base::name::Name::new("Alt"), [Value::Char(c)])
        },
        KeyCode::Char(c) => data_value(ty, ksl_base::name::Name::new("Char"), [Value::Char(c)]),
        KeyCode::Enter => empty("Enter"),
        KeyCode::Escape => empty("Escape"),
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
        KeyCode::Function(number) => {
            data_value(ty, ksl_base::name::Name::new("F"), [Value::U8(number)])
        },
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

/// 无载荷的 `InputError` 变体.
const INPUT_ERROR_PLAIN: &[&str] = &[
    "NotATerminal",
    "Interrupted",
    "RawModeActive",
    "RawModeHeld",
    "NotRawModeHolder",
    "RawModeReleased",
];

fn input_error(variant: &str, field: &str) -> Value {
    let ty = ksl_base::name::Name::new("InputError");
    let name = ksl_base::name::Name::new(variant);
    if INPUT_ERROR_PLAIN.contains(&variant) {
        data_value0(ty, name)
    } else {
        data_value(ty, name, [Value::Str(ArcStr::from(field))])
    }
}

/// 阻塞 I/O 工作线程数的默认范围: 4 到 16.
fn io_workers_default() -> usize {
    std::thread::available_parallelism().map_or(4, |cpus| cpus.get().clamp(4, 16))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crossbeam_channel::bounded;
    use ksl_value::value::Value;

    use super::file::{close, open_read, open_write, read_line_from, write_all};
    use super::{IoConfig, IoRuntime, Task, closed_file, key_value};
    use crate::host;
    use crate::terminal::state::RawModeHolder;
    use crate::terminal::{KeyCode, KeyEvent, KeyModifiers};

    /// 取错误变体名; 输入必须是 `Result/Err[FileError]`.
    fn error_variant(value: Value) -> String {
        let Value::Data(result) = value else {
            panic!("I/O 结果应为 Result");
        };
        assert_eq!(result.variant.as_str(), "Err");
        let Value::Data(error) = &result.fields[0] else {
            panic!("错误负载应为 FileError");
        };
        error.variant.as_str().to_string()
    }

    /// 取变体名与字段; 未建模的按键返回 None.
    fn mapped(code: KeyCode, modifiers: KeyModifiers) -> Option<(String, Vec<Value>)> {
        let value = key_value(KeyEvent::new(code, modifiers))?;
        let Value::Data(data) = value else {
            panic!("KeyEvent 必须是 Data");
        };
        Some((data.variant.as_str().to_string(), data.fields.to_vec()))
    }

    #[test]
    fn key_events_map_to_values() {
        // 修饰键折叠进变体, Ctrl 优先于 Alt.
        let (variant, fields) =
            mapped(KeyCode::Char('c'), KeyModifiers::CONTROL | KeyModifiers::ALT).unwrap();
        assert_eq!(variant, "Ctrl");
        assert!(matches!(fields.as_slice(), [Value::Char('c')]));

        // 功能键带载荷; 未建模的按键不进入语言层.
        let (variant, fields) = mapped(KeyCode::Function(12), KeyModifiers::NONE).unwrap();
        assert_eq!(variant, "F");
        assert!(matches!(fields.as_slice(), [Value::U8(12)]));
        assert!(mapped(KeyCode::CapsLock, KeyModifiers::NONE).is_none());
    }

    #[test]
    fn file_handle_direction_errors_are_distinct() {
        let dir = ksl_base::testutil::TempDir::new("ksl_io_direction_");
        let path = dir.path().join("data.txt");
        let path = path.to_str().expect("路径应为 UTF-8");

        let Value::Data(written) = open_write(path, false) else {
            panic!("打开操作应返回 Result");
        };
        let Some(writer) = host::as_file(&written.fields[0]) else {
            panic!("打开结果应为 File");
        };
        assert_eq!(error_variant(read_line_from(writer)), "NotReadable");
        let _written = write_all(writer, b"x");
        let _closed = close(writer);

        let Value::Data(opened) = open_read(path) else {
            panic!("打开操作应返回 Result");
        };
        let Some(reader) = host::as_file(&opened.fields[0]) else {
            panic!("打开结果应为 File");
        };
        assert_eq!(error_variant(write_all(reader, b"x")), "NotWritable");
        let _read = read_line_from(reader);
        let _closed = close(reader);

        let Some(closed) = host::as_file(&closed_file()).cloned() else {
            panic!("空句柄应为 File");
        };
        assert_eq!(error_variant(read_line_from(&closed)), "InvalidHandle");
        assert_eq!(error_variant(write_all(&closed, b"x")), "InvalidHandle");
    }

    #[test]
    fn cpu_pool_does_not_block_io_pool() {
        let io = IoRuntime::new(IoConfig {
            io_workers: Some(1),
            cpu_workers: Some(1),
        });
        let holder = || RawModeHolder::from_owner(None, None);
        let (started_tx, started_rx) = bounded(1);
        let (release_tx, release_rx) = bounded(1);
        io.submit_task(
            Task::Compute(Box::new(move || {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                Ok(Value::Nil)
            })),
            1,
            holder(),
            |_| {},
        );
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("CPU 任务应开始执行");

        let (done_tx, done_rx) = bounded(1);
        io.submit_task(
            Task::Blocking(Box::new(|| Ok(Value::Nil))),
            2,
            holder(),
            move |_| {
                let _ = done_tx.send(());
            },
        );
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("I/O 任务不应被 CPU 任务阻塞");
        let _ = release_tx.send(());
    }
}
