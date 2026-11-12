//! 本机命令执行.
//!
//! 一次性启动子进程, 捕获 stdout 与 stderr, 并返回退出状态.
//! 命名的程序与参数直接交给子进程, 不经过 shell.
//! 读输出与写输入在独立线程中并发进行, 避免管道互相阻塞.
//! 任一路输出超限时终止子进程, 并报告 `OutputTooLarge`.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::process::{ChildStdin, Command, ExitStatus, Stdio};
use std::sync::Arc;

use arcstr::ArcStr;
use ksl_base::source::Span;
use ksl_base::symbol::{intern, resolve};
use ksl_frontend::error::RuntimeError as Error;
use parking_lot::Mutex;
use shared_child::SharedChild;

use crate::value::{List, Value, data_value, result_err, result_ok};

/// 单路输出的字节上限.
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

/// 子进程的最终状态.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CommandStatus {
    Exit(u64),
    Signal(u64),
}

/// 一次性命令执行的输出与状态.
pub struct CommandOutput {
    status: CommandStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl std::fmt::Debug for CommandOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<command-output>")
    }
}

/// 由 `RunCommand` 的选项解析出的执行参数.
#[derive(Default)]
struct CommandOptions {
    cwd: Option<String>,
    env: Option<Vec<(String, String)>>,
    input: Option<CommandInput>,
}

enum CommandInput {
    Text(String),
    Binary(Vec<u8>),
}

enum ParsedOptions {
    Ok(CommandOptions),
    Error(Value),
}

/// 一次命令执行的参数与取消控制.
pub(crate) struct ProcessTask {
    spec: ProcessSpec,
    control: Arc<ProcessControl>,
}

impl ProcessTask {
    fn new(spec: ProcessSpec) -> Self {
        Self {
            spec,
            control: Arc::new(ProcessControl::new()),
        }
    }

    pub(crate) fn control(&self) -> &Arc<ProcessControl> {
        &self.control
    }

    /// 在 I/O 阻塞池中执行命令; 取消时终止子进程.
    pub(crate) fn run(self) -> Value {
        run_command(self.spec, &self.control)
    }
}

struct ProcessSpec {
    program: String,
    args: Vec<String>,
    options: CommandOptions,
}

/// 子进程的共享控制; 等待与终止可以并发进行.
pub(crate) struct ProcessControl {
    state: Mutex<ProcessState>,
}

enum ProcessState {
    Pending,
    Running(Arc<SharedChild>),
    Cancelled,
}

impl ProcessControl {
    fn new() -> Self {
        Self {
            state: Mutex::new(ProcessState::Pending),
        }
    }

    /// 取消命令; 已启动的子进程会被终止.
    pub(crate) fn cancel(&self) {
        let mut state = self.state.lock();
        if matches!(&*state, ProcessState::Pending) {
            *state = ProcessState::Cancelled;
        } else if let ProcessState::Running(child) = &*state {
            let _ = child.kill();
        }
    }

    /// 在锁内完成启动, 避免取消与启动交错.
    fn spawn(&self, command: &mut Command) -> std::io::Result<Option<Arc<SharedChild>>> {
        let mut state = self.state.lock();
        if matches!(&*state, ProcessState::Cancelled) {
            return Ok(None);
        }
        let child = Arc::new(SharedChild::spawn(command)?);
        *state = ProcessState::Running(Arc::clone(&child));
        Ok(Some(child))
    }
}

/// 构造 `RunCommand` 的阻塞任务.
pub(crate) fn run_command_task(args: &[Value], _: Option<Span>) -> Result<super::Task, Error> {
    let program = super::string_arg(args, 0, "RunCommand 的程序名")?.to_string();
    let command_args = string_list_arg(args, 1, "RunCommand 的参数")?;
    match command_options_arg(args, 2)? {
        ParsedOptions::Ok(options) => Ok(super::Task::Process(ProcessTask::new(ProcessSpec {
            program,
            args: command_args,
            options,
        }))),
        ParsedOptions::Error(value) => Ok(super::Task::Ready(result_err(value))),
    }
}

/// 读取 `CommandOutput` 的退出状态.
pub(crate) fn command_output_status(
    _: &[String],
    args: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let output = command_output_arg(args)?;
    Ok(status_value(output.status))
}

/// 读取 `CommandOutput` 的标准输出字节.
pub(crate) fn command_output_stdout(
    _: &[String],
    args: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let output = command_output_arg(args)?;
    Ok(Value::List(Arc::new(List::u8s(output.stdout.iter().copied()))))
}

/// 读取 `CommandOutput` 的标准错误字节.
pub(crate) fn command_output_stderr(
    _: &[String],
    args: &[Value],
    _: Option<Span>,
) -> Result<Value, Error> {
    let output = command_output_arg(args)?;
    Ok(Value::List(Arc::new(List::u8s(output.stderr.iter().copied()))))
}

fn command_output_arg(args: &[Value]) -> Result<&CommandOutput, Error> {
    let Some(Value::CommandOutput(output)) = args.first() else {
        return Err(Error::internal("命令输出访问函数的实参不是 CommandOutput"));
    };
    Ok(output)
}

fn string_list_arg(args: &[Value], index: usize, what: &str) -> Result<Vec<String>, Error> {
    let Some(Value::List(items)) = args.get(index) else {
        return Err(Error::internal(format!("{what}不是字符串列表")));
    };
    let mut result = Vec::with_capacity(items.len());
    for position in 0..items.len() {
        let Some(Value::Str(text)) = items.get_value(position) else {
            return Err(Error::internal(format!("{what}的元素不是字符串")));
        };
        result.push(text.to_string());
    }
    Ok(result)
}

fn command_options_arg(args: &[Value], index: usize) -> Result<ParsedOptions, Error> {
    let Some(Value::List(items)) = args.get(index) else {
        return Err(Error::internal("RunCommand 的选项不是列表"));
    };
    let mut options = CommandOptions::default();
    let mut seen_options = HashSet::new();
    for position in 0..items.len() {
        let Some(Value::Data(data)) = items.get_value(position) else {
            return Err(Error::internal("RunCommand 的选项元素不是 Data"));
        };
        if data.ty != intern("CommandOption") {
            return Err(Error::internal("RunCommand 的选项不是 CommandOption"));
        }
        let name = resolve(data.variant);
        if !seen_options.insert(data.variant) {
            return Ok(ParsedOptions::Error(command_error(
                "DuplicateOption",
                [Value::Str(ArcStr::from(name.as_ref()))],
            )));
        }
        match &*name {
            "Cwd" => {
                let Some(Value::Str(path)) = data.fields.first() else {
                    return Err(Error::internal("CommandOption/Cwd 的字段不是字符串"));
                };
                options.cwd = Some(path.to_string());
            },
            "Env" => {
                let Some(Value::List(entries)) = data.fields.first() else {
                    return Err(Error::internal("CommandOption/Env 的字段不是列表"));
                };
                let mut env = Vec::with_capacity(entries.len());
                let mut names = HashSet::new();
                for entry in 0..entries.len() {
                    let Some(Value::Pair(pair)) = entries.get_value(entry) else {
                        return Err(Error::internal("CommandOption/Env 的元素不是配对"));
                    };
                    let (Value::Str(name), Value::Str(value)) = (&pair.0, &pair.1) else {
                        return Err(Error::internal("环境变量的名字或值不是字符串"));
                    };
                    if !names.insert(name.to_string()) {
                        return Ok(ParsedOptions::Error(command_error(
                            "DuplicateEnv",
                            [Value::Str(name.clone())],
                        )));
                    }
                    env.push((name.to_string(), value.to_string()));
                }
                options.env = Some(env);
            },
            "Input" => {
                let Some(Value::Data(input)) = data.fields.first() else {
                    return Err(Error::internal("CommandOption/Input 的字段不是 CommandInput"));
                };
                match &*resolve(input.variant) {
                    "Text" => {
                        let Some(Value::Str(text)) = input.fields.first() else {
                            return Err(Error::internal("CommandInput/Text 的字段不是字符串"));
                        };
                        options.input = Some(CommandInput::Text(text.to_string()));
                    },
                    "Binary" => {
                        let Some(Value::List(bytes)) = input.fields.first() else {
                            return Err(Error::internal("CommandInput/Binary 的字段不是列表"));
                        };
                        options.input = Some(CommandInput::Binary(list_bytes(bytes)?));
                    },
                    other => {
                        return Err(Error::internal(format!(
                            "未知的 CommandInput 变体: {other}"
                        )));
                    },
                }
            },
            other => {
                return Err(Error::internal(format!("未知的 CommandOption 变体: {other}")));
            },
        }
    }
    Ok(ParsedOptions::Ok(options))
}

fn list_bytes(items: &List) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::with_capacity(items.len());
    for position in 0..items.len() {
        let Some(Value::U8(byte)) = items.get_value(position) else {
            return Err(Error::internal("字节列表的元素不是 u8"));
        };
        bytes.push(byte);
    }
    Ok(bytes)
}

fn run_command(spec: ProcessSpec, control: &ProcessControl) -> Value {
    let ProcessSpec {
        program,
        args,
        options,
    } = spec;
    if let Some(cwd) = options.cwd.as_deref()
        && !is_directory(cwd)
    {
        return result_err(invalid_cwd(cwd));
    }
    let input = options.input.map(|input| match input {
        CommandInput::Text(text) => text.into_bytes(),
        CommandInput::Binary(bytes) => bytes,
    });
    let mut command = Command::new(&program);
    command.args(&args);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    if let Some(cwd) = options.cwd.as_deref() {
        command.current_dir(cwd);
    }
    if let Some(env) = options.env.as_deref() {
        command.envs(env.iter().map(|(name, value)| (name, value)));
    }
    let child = match control.spawn(&mut command) {
        Ok(Some(child)) => child,
        Ok(None) => return result_err(other_error("命令已取消")),
        Err(error) => {
            return result_err(spawn_error(&error, &program, options.cwd.as_deref()));
        },
    };
    let stdout = child.take_stdout().expect("stdout 已设置为管道");
    let stderr = child.take_stderr().expect("stderr 已设置为管道");
    let stdin = child.take_stdin();
    let overflow = Arc::new(Overflow::default());
    let stdout_thread =
        spawn_reader(stdout, "stdout", Arc::clone(&child), Arc::clone(&overflow));
    let stderr_thread =
        spawn_reader(stderr, "stderr", Arc::clone(&child), Arc::clone(&overflow));
    let input_thread = input.zip(stdin).map(|(bytes, pipe)| {
        std::thread::Builder::new()
            .name("ksl-command-stdin".to_owned())
            .spawn(move || write_input(pipe, &bytes))
            .expect("无法启动命令输入线程")
    });
    let status = child.wait();
    let stdout = stdout_thread.join().expect("stdout 读取线程 panic");
    let stderr = stderr_thread.join().expect("stderr 读取线程 panic");
    let input = input_thread.map(|thread| thread.join().expect("stdin 写入线程 panic"));
    if let Some(stream) = overflow.take() {
        return result_err(output_too_large(stream));
    }
    let stdout = match stdout {
        Ok(bytes) => bytes,
        Err(error) => return result_err(other_error(&error.to_string())),
    };
    let stderr = match stderr {
        Ok(bytes) => bytes,
        Err(error) => return result_err(other_error(&error.to_string())),
    };
    match input {
        None | Some(Ok(())) => {},
        Some(Err(error)) => return result_err(other_error(&error.to_string())),
    }
    let status = match status {
        Ok(status) => status_of(status),
        Err(error) => return result_err(other_error(&error.to_string())),
    };
    result_ok(Value::CommandOutput(Arc::new(CommandOutput {
        status,
        stdout,
        stderr,
    })))
}

/// 已超限的输出流; 只记录第一路.
#[derive(Default)]
struct Overflow {
    stream: Mutex<Option<&'static str>>,
}

impl Overflow {
    fn set(&self, stream: &'static str) {
        let mut slot = self.stream.lock();
        if slot.is_none() {
            *slot = Some(stream);
        }
    }

    fn take(&self) -> Option<&'static str> {
        self.stream.lock().take()
    }
}

fn spawn_reader(
    mut reader: impl Read + Send + 'static,
    stream: &'static str,
    child: Arc<SharedChild>,
    overflow: Arc<Overflow>,
) -> std::thread::JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::Builder::new()
        .name(format!("ksl-command-{stream}"))
        .spawn(move || read_limited(&mut reader, stream, &child, &overflow))
        .expect("无法启动命令输出线程")
}

fn read_limited(
    reader: &mut impl Read,
    stream: &'static str,
    child: &SharedChild,
    overflow: &Overflow,
) -> std::io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(output);
        }
        if output.len() + count > MAX_OUTPUT {
            overflow.set(stream);
            let _ = child.kill();
            return Ok(output);
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

fn write_input(mut input: ChildStdin, bytes: &[u8]) -> std::io::Result<()> {
    match input.write_all(bytes) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(error) => Err(error),
    }
}

fn is_directory(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

fn spawn_error(error: &std::io::Error, program: &str, cwd: Option<&str>) -> Value {
    if let Some(cwd) = cwd
        && !is_directory(cwd)
    {
        return invalid_cwd(cwd);
    }
    match error.kind() {
        std::io::ErrorKind::NotFound => {
            command_error("NotFound", [Value::Str(ArcStr::from(program))])
        },
        std::io::ErrorKind::PermissionDenied => {
            command_error("PermissionDenied", [Value::Str(ArcStr::from(program))])
        },
        _ => other_error(&error.to_string()),
    }
}

fn status_of(status: ExitStatus) -> CommandStatus {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return CommandStatus::Signal(u64::from(signal.cast_unsigned()));
        }
    }
    CommandStatus::Exit(status.code().map_or(0, |code| u64::from(code.cast_unsigned())))
}

fn status_value(status: CommandStatus) -> Value {
    let ty = intern("CommandStatus");
    match status {
        CommandStatus::Exit(code) => data_value(ty, intern("Exit"), [Value::U64(code)]),
        CommandStatus::Signal(signal) => data_value(ty, intern("Signal"), [Value::U64(signal)]),
    }
}

fn command_error(variant: &str, fields: impl Into<Box<[Value]>>) -> Value {
    data_value(intern("CommandError"), intern(variant), fields)
}

fn other_error(message: &str) -> Value {
    command_error("Other", [Value::Str(ArcStr::from(message))])
}

fn invalid_cwd(path: &str) -> Value {
    command_error("InvalidCwd", [Value::Str(ArcStr::from(path))])
}

fn output_too_large(stream: &str) -> Value {
    command_error("OutputTooLarge", [Value::Str(ArcStr::from(stream))])
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn run(program: &str, args: &[&str], options: CommandOptions) -> Value {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        run_command(
            ProcessSpec {
                program: program.to_string(),
                args,
                options,
            },
            &ProcessControl::new(),
        )
    }

    fn output(value: &Value) -> &CommandOutput {
        let Value::Data(result) = value else {
            panic!("结果不是 Result");
        };
        let Value::CommandOutput(output) = &result.fields[0] else {
            panic!("结果不是 Ok[CommandOutput]");
        };
        output
    }

    fn error_variant(value: &Value) -> String {
        let Value::Data(result) = value else {
            panic!("结果不是 Result");
        };
        let Value::Data(error) = &result.fields[0] else {
            panic!("错误不是 Data");
        };
        resolve(error.variant).to_string()
    }

    #[test]
    fn captures_output_and_status() {
        let value = run(
            "sh",
            &["-c", "printf hello; printf oops >&2; exit 3"],
            CommandOptions::default(),
        );
        let output = output(&value);
        assert!(matches!(output.status, CommandStatus::Exit(3)));
        assert_eq!(output.stdout, b"hello");
        assert_eq!(output.stderr, b"oops");
    }

    #[test]
    fn feeds_text_input() {
        let value = run(
            "sh",
            &["-c", "cat"],
            CommandOptions {
                input: Some(CommandInput::Text("hello\n".to_string())),
                ..CommandOptions::default()
            },
        );
        assert_eq!(output(&value).stdout, b"hello\n");
    }

    #[test]
    fn stops_child_when_output_exceeds_limit() {
        let value = run("cat", &["/dev/zero"], CommandOptions::default());
        assert_eq!(error_variant(&value), "OutputTooLarge");
    }

    #[test]
    fn cancelled_command_does_not_start() {
        // 取消发生在启动前: 任务直接失败, 不执行程序.
        let control = ProcessControl::new();
        control.cancel();
        let value = run_command(
            ProcessSpec {
                program: "true".to_string(),
                args: Vec::new(),
                options: CommandOptions::default(),
            },
            &control,
        );
        assert_eq!(error_variant(&value), "Other");
    }

    #[test]
    fn reports_invalid_cwd() {
        let value = run(
            "sh",
            &["-c", "true"],
            CommandOptions {
                cwd: Some("/ksl-command-missing-cwd".to_string()),
                ..CommandOptions::default()
            },
        );
        assert_eq!(error_variant(&value), "InvalidCwd");
    }
}
