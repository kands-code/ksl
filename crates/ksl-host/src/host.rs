//! 宿主状态.
//!
//! 交互输入在会话副本上检查, 运行成功后才提交类型环境与两份缓存.
//! 宿主只返回结构化结果, 诊断渲染由调用方负责.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ksl_base::source::Sources;
use ksl_base::types::Type;
use ksl_compile::{
    CachedImports, CheckedGraph, CompileCache, RootSource, compile, root_dir_of,
};
use ksl_diagnostics::error::{KslError, RuntimeError};
use ksl_frontend::ast::{Expr, ExprKind, Program, Special};
use ksl_frontend::check::Session;
use ksl_frontend::validate::{self, SourceKind};
use ksl_io::io::{IoConfig, IoRuntime};
use ksl_runtime::{RootGuard, Scheduler};
use ksl_value::value::Value;

use crate::module::{InstanceCache, RootEnv, execute_with_cache};
use crate::plugin::LibraryKeeper;

/// 一次执行的结局.
pub struct RunOutcome {
    /// 主脚本的结果, 含 `@Exit` 与运行时错误.
    pub result: Result<Value, RuntimeError>,
    /// Actor 错误; 不影响主脚本结局与退出码.
    pub actor_errors: Vec<RuntimeError>,
}

/// 一段交互输入的结局.
pub struct InputOutcome {
    /// 成功时给出尾表达式的值与类型; `@Exit`, 运行时错误与输出流关闭都走错误侧.
    pub result: Result<(Value, Option<Type>), RuntimeError>,
    /// Actor 错误; 不影响主脚本结局与退出码.
    pub actor_errors: Vec<RuntimeError>,
}

/// 宿主: 持有一次进程内语言运行所需的全部状态.
pub struct Host {
    sources: Sources,
    session: Session,
    scheduler: Scheduler,
    /// 宿主拥有的 I/O 服务; 调度器句柄持有同一实例的引用.
    io: Arc<IoRuntime>,
    /// 编译缓存: 模块接口与依赖文件状态.
    compile: CompileCache,
    /// 运行缓存: 模块导出值与插件实例.
    runtime: InstanceCache,
    /// 已提交模块导出值的根; 运行缓存替换时同步更新.
    module_roots: Option<RootGuard>,
    /// 已加载插件库; 随宿主释放, 不随会话与根目录重置.
    libraries: LibraryKeeper,
    root_dir: PathBuf,
    /// 是否交互会话模式; reset 时保留.
    repl_mode: bool,
}

impl Host {
    /// 以给定根目录建立空宿主.
    #[must_use]
    pub fn new(root_dir: PathBuf) -> Self {
        Self::with_config(root_dir, IoConfig::default())
    }

    /// 以给定 I/O 配置建立空宿主.
    #[must_use]
    pub(crate) fn with_config(root_dir: PathBuf, config: IoConfig) -> Self {
        let io = Arc::new(IoRuntime::new(config));
        Self {
            sources: Sources::default(),
            session: Session::new(),
            scheduler: Scheduler::with_io(Arc::clone(&io)),
            io,
            compile: CompileCache::new(),
            runtime: InstanceCache::new(),
            module_roots: None,
            libraries: LibraryKeeper::default(),
            root_dir,
            repl_mode: false,
        }
    }

    /// 以当前工作目录与指定 I/O, CPU 工作线程数建立空宿主.
    #[must_use]
    pub fn in_current_dir_with_workers(io: Option<usize>, cpu: Option<usize>) -> Self {
        let root_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::with_config(
            root_dir,
            IoConfig {
                io_workers: io,
                cpu_workers: cpu,
            },
        )
    }

    /// 开启标准输出的行首与写出跟踪.
    pub fn begin_stdout_tracking(&self) {
        self.io.begin_stdout_tracking();
    }

    /// 结束标准输出跟踪, 返回输出当前是否停在行首.
    #[must_use]
    pub fn take_stdout_line_start(&self) -> bool {
        self.io.take_stdout_line_start()
    }

    /// 取本次跟踪期间标准输出是否写过非空文本.
    #[must_use]
    pub fn take_stdout_wrote(&self) -> bool {
        self.io.take_stdout_wrote()
    }

    /// 开启标准错误的写出跟踪.
    pub fn begin_stderr_tracking(&self) {
        self.io.begin_stderr_tracking();
    }

    /// 结束标准错误跟踪, 返回本次是否写过非空文本.
    #[must_use]
    pub fn take_stderr_wrote(&self) -> bool {
        self.io.take_stderr_wrote()
    }

    /// 请求输入线程开启终端会话.
    ///
    /// # Errors
    ///
    /// 终端不可用或输入线程已停止.
    pub fn open_terminal_session(
        &self,
        bracketed_paste: bool,
    ) -> Result<ksl_io::terminal::Session, RuntimeError> {
        self.io.open_session(bracketed_paste)
    }

    /// 取走终端模式中已消费但未交付的一行.
    #[must_use]
    pub fn take_buffered_line(&self) -> Option<String> {
        self.io.take_buffered_line()
    }

    /// 取走终端模式中已编辑但未交付的文本.
    #[must_use]
    pub fn take_buffered_text(&self) -> Option<String> {
        self.io.take_buffered_text()
    }

    /// 源码集合; 诊断渲染与调试输出使用.
    #[must_use]
    pub fn sources(&self) -> &Sources {
        &self.sources
    }

    /// 当前类型检查会话.
    #[must_use]
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// 设置交互会话模式; 交互会话不注册平台事件泵.
    pub fn set_repl_mode(&mut self, repl: bool) {
        self.scheduler.set_pumps_enabled(!repl);
        self.repl_mode = repl;
    }

    pub fn set_args(&mut self, args: Vec<String>) {
        self.scheduler.set_args(args);
    }

    /// 执行文件形式的脚本.
    ///
    /// 加载, 检查与执行都在宿主状态上进行.
    /// 成功后提交编译与运行缓存以及会话, 失败时保持原状.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn run_file(&mut self, file: &Path) -> Result<RunOutcome, KslError> {
        self.run_file_inner(file, true)
    }

    /// 按模块加载初始化文件, 执行顶层语句但不运行 `@Main`.
    ///
    /// 只保留导出供交互会话使用.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn load_file(&mut self, file: &Path) -> Result<RunOutcome, KslError> {
        self.run_file_inner(file, false)
    }

    fn run_file_inner(
        &mut self,
        file: &Path,
        execute_main: bool,
    ) -> Result<RunOutcome, KslError> {
        // 加载失败时仍设置根目录, 供后续行内 `@Use` 使用.
        let root_dir = root_dir_of(file);
        if root_dir != self.root_dir {
            // 缓存按根相对路径索引, 根目录变化时清空.
            self.compile = CompileCache::new();
            self.runtime = InstanceCache::new();
            self.module_roots = None;
        }
        self.root_dir = root_dir;
        let mut trial_compile = self.compile.clone();
        let mut trial_runtime = self.runtime.clone();
        let graph = compile(RootSource::File(file), &mut self.sources, &mut trial_compile)?;
        let checkpoint = self.scheduler.snapshot_globals();
        let execution = execute_with_cache(
            &mut self.scheduler,
            &graph,
            RootEnv::OnlyExports,
            &mut trial_runtime,
            &mut self.libraries,
        );
        // 模块导出值在提交前只由本次执行与缓存副本持有; 执行与根 Actor 运行期间都须作为根.
        let module_roots = self.scheduler.root_scope(trial_runtime.root_values());
        let (mut result, context) = match execution {
            Ok(execution) => {
                let mut result = Ok(execution.main_value);
                if execute_main && let Some(handler) = execution.main_handler {
                    result = match execution.main_mailboxes {
                        Some(mailboxes) => self.scheduler.run_main(handler, mailboxes),
                        None => Err(RuntimeError::internal("Main 缺少信箱数")),
                    };
                }
                (result, Some(execution.context))
            },
            Err(error) => (Err(error), None),
        };
        let actor_errors = self.scheduler.take_actor_errors();
        settle_output(&self.io, &mut result);
        self.io.restore_terminal();
        if result.is_ok() {
            if let Some(context) = context {
                self.compile = trial_compile;
                self.runtime = trial_runtime;
                self.module_roots = Some(module_roots);
                self.session = Session::from_module_context(context);
            }
        } else {
            // 根 Actor 在模块执行之后才启动, 失败时模块执行已写入的全局绑定也要撤销.
            self.scheduler.restore_globals(checkpoint);
        }
        Ok(RunOutcome {
            result,
            actor_errors,
        })
    }

    /// 执行一段脚本代码.
    ///
    /// 脚本中的 `@Use` 逻辑上位于顶部.
    /// 与文件脚本不同, 代码段不是模块, 不能声明 `@Export`.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn run_code(&mut self, name: &str, code: &str) -> Result<RunOutcome, KslError> {
        let program = parse(&mut self.sources, name, code)?;
        validate::validate(&program, SourceKind::Code)?;
        let prepared = self.prepare_code(name, program)?;
        let outcome = self.execute_steps(prepared);
        Ok(RunOutcome {
            result: outcome.result.map(|(value, _)| value),
            actor_errors: outcome.actor_errors,
        })
    }

    /// 解析一段源码; 追加到宿主源码集合.
    ///
    /// # Errors
    ///
    /// 词法或语法失败.
    pub fn parse(&mut self, name: &str, text: &str) -> Result<Program, KslError> {
        parse(&mut self.sources, name, text)
    }

    /// 在会话副本上检查一段程序, 返回尾表达式类型; 不提交绑定.
    ///
    /// # Errors
    ///
    /// 返回首个类型错误.
    pub fn check_program(&self, program: &Program) -> Result<Option<Type>, KslError> {
        let mut trial = self.session.fork();
        let checked = trial.check_typed_in_place(program, None)?;
        Ok(checked.tail.as_ref().map(|tail| tail.ty.clone()))
    }

    /// 在输入边界回收不可达环.
    ///
    /// 模块运行缓存导出值由宿主持有根作用域.
    /// 宿主之外持有的值不参与回收; 跨输入持有闭包或容器时,
    /// 应避免在此期间触发自动收集.
    pub fn collect_cycles(&mut self) {
        self.scheduler.collect_cycles(&[]);
    }

    /// 求值一段交互输入.
    ///
    /// 输入中的 `@Use` 按所在位置加载并执行.
    /// 类型检查在会话副本上进行, 运行成功后才提交类型环境与两份缓存.
    /// 运行时失败回滚全局绑定.
    ///
    /// 返回的值不属于宿主根. 跨输入持有返回的闭包或容器时,
    /// 后续输入期间的自动收集可能清空其不可达捕获; 调用方应避免这种持有,
    /// 或改用带外部根的调度器入口.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn eval_input(&mut self, name: &str, text: &str) -> Result<InputOutcome, KslError> {
        let prepared = self.prepare_input(name, text)?;
        Ok(self.execute_steps(prepared))
    }

    /// 清空会话状态; 保留根目录.
    pub fn reset(&mut self) {
        self.module_roots = None;
        self.scheduler.shutdown();
        let _ = self.io.flush_output();
        let mut scheduler = Scheduler::with_io(Arc::clone(&self.io));
        scheduler.set_pumps_enabled(!self.repl_mode);
        self.scheduler = scheduler;
        self.session = Session::new();
        self.compile = CompileCache::new();
        self.runtime = InstanceCache::new();
        self.sources = Sources::default();
    }

    /// 终止调度器, 清理 Actor, 等待已排队的输出写出, 并恢复终端状态.
    pub fn shutdown(&mut self) {
        self.scheduler.shutdown();
        let _ = self.io.flush_output();
        self.io.restore_terminal();
    }

    /// 语法与类型检查阶段; 不修改会话与两份缓存.
    fn prepare_input(&mut self, name: &str, text: &str) -> Result<PreparedInput, KslError> {
        let program = parse(&mut self.sources, name, text)?;
        validate::validate(&program, SourceKind::Repl)?;
        self.prepare_steps(name, split_steps(program))
    }

    /// 检查一段代码段; `@Use` 提升到顶部并合并为一次虚拟根加载.
    fn prepare_code(
        &mut self,
        name: &str,
        program: Program,
    ) -> Result<PreparedInput, KslError> {
        let (uses, body) = split_uses(program);
        let mut trial = self.session.fork();
        let mut trial_compile = self.compile.clone();
        let mut checked = Vec::new();
        if !uses.is_empty() {
            let graph = self.load_imports(name, uses, &mut trial, &mut trial_compile)?;
            checked.push(CheckedStep::Import(graph));
        }
        let (last_tail_ty, body) = check_body(&mut trial, &body)?;
        checked.push(CheckedStep::Body(Box::new(body)));
        Ok(PreparedInput {
            trial,
            compile: trial_compile,
            checked,
            last_tail_ty,
        })
    }

    /// 按顶层顺序检查导入与语句块.
    fn prepare_steps(
        &mut self,
        name: &str,
        steps: Vec<Step>,
    ) -> Result<PreparedInput, KslError> {
        let mut trial = self.session.fork();
        let mut trial_compile = self.compile.clone();
        let mut checked = Vec::new();
        let mut last_tail_ty = None;
        for step in steps {
            match step {
                Step::Import(expr) => {
                    let empty_use = matches!(
                        &expr.node,
                        ExprKind::Special(Special::Use { items, .. }) if items.is_empty()
                    );
                    if empty_use {
                        continue;
                    }
                    let graph =
                        self.load_imports(name, vec![expr], &mut trial, &mut trial_compile)?;
                    checked.push(CheckedStep::Import(graph));
                    last_tail_ty = None;
                },
                Step::Body(body) => {
                    let (tail_ty, body) = check_body(&mut trial, &body)?;
                    last_tail_ty = tail_ty;
                    checked.push(CheckedStep::Body(Box::new(body)));
                },
            }
        }
        Ok(PreparedInput {
            trial,
            compile: trial_compile,
            checked,
            last_tail_ty,
        })
    }

    /// 加载一组导入并并入会话.
    fn load_imports(
        &mut self,
        name: &str,
        uses: Vec<Expr>,
        trial: &mut Session,
        cache: &mut CompileCache,
    ) -> Result<CheckedGraph, KslError> {
        let root = Program {
            stmts: uses,
            tail: None,
        };
        let graph = compile(
            RootSource::Virtual {
                dir: &self.root_dir,
                name,
                program: root,
                cached_imports: CachedImports::Reject,
            },
            &mut self.sources,
            cache,
        )?;
        trial.merge_module_context(graph.import_context())?;
        Ok(graph)
    }

    /// 执行已检查的输入, 成功后才提交会话与两份缓存.
    fn execute_steps(&mut self, prepared: PreparedInput) -> InputOutcome {
        let PreparedInput {
            trial,
            compile,
            checked,
            last_tail_ty,
        } = prepared;
        let mut runtime = self.runtime.clone();
        let checkpoint = self.scheduler.snapshot_globals();
        let mut outcome = Ok(Value::Nil);
        for step in checked {
            // 逐步丢弃上一步结果, 不让下一步继续持有.
            drop(std::mem::replace(&mut outcome, Ok(Value::Nil)));
            let _module_scope = self.scheduler.root_scope(runtime.root_values());
            let step_result = match step {
                CheckedStep::Import(graph) => execute_with_cache(
                    &mut self.scheduler,
                    &graph,
                    RootEnv::KeepBindings,
                    &mut runtime,
                    &mut self.libraries,
                )
                .map(|_| Value::Nil),
                CheckedStep::Body(body) => {
                    let linear = ksl_ir::compile(&body);
                    self.scheduler.run_linear(linear)
                },
            };
            match step_result {
                Ok(value) => outcome = Ok(value),
                Err(error) => {
                    outcome = Err(error);
                    break;
                },
            }
        }
        settle_output(&self.io, &mut outcome);
        self.io.restore_terminal();
        let actor_errors = self.scheduler.take_actor_errors();
        match outcome {
            Ok(value) => {
                self.session = trial;
                self.compile = compile;
                self.runtime = runtime;
                self.module_roots = Some(self.scheduler.root_scope(self.runtime.root_values()));
                InputOutcome {
                    result: Ok((value, last_tail_ty)),
                    actor_errors,
                }
            },
            Err(error) => {
                if !matches!(error, RuntimeError::Exit(_) | RuntimeError::BrokenPipe) {
                    self.scheduler.restore_globals(checkpoint);
                }
                InputOutcome {
                    result: Err(error),
                    actor_errors,
                }
            },
        }
    }
}

/// 检查一段语句, 返回尾表达式类型与 typed 程序.
fn check_body(
    trial: &mut Session,
    body: &Program,
) -> Result<(Option<Type>, ksl_typed::CheckedProgram), KslError> {
    let checked = trial.check_typed_in_place(body, None)?;
    let tail_ty = checked.tail.as_ref().map(|tail| tail.ty.clone());
    Ok((tail_ty, checked))
}

/// 解析一段源码, 返回语法树.
///
/// # Errors
///
/// 词法或语法失败.
pub fn parse(sources: &mut Sources, name: &str, text: &str) -> Result<Program, KslError> {
    let id = sources.add(name, text);
    ksl_frontend::parser::parse_text(sources.text(id), id)
}

/// 分离顶层导入声明与其余程序.
///
/// 导入声明只在顶层, 只扫描顶层语句.
#[must_use]
pub fn split_uses(program: Program) -> (Vec<Expr>, Program) {
    let Program { stmts, mut tail } = program;
    let mut uses = Vec::new();
    let mut rest = Vec::new();
    for stmt in stmts {
        if is_use(&stmt.node) {
            uses.push(stmt);
        } else {
            rest.push(stmt);
        }
    }
    if tail.as_ref().is_some_and(|expr| is_use(&expr.node)) {
        uses.extend(tail.take());
    }
    (uses, Program { stmts: rest, tail })
}

/// 已通过检查, 等待提交的输入.
struct PreparedInput {
    trial: Session,
    compile: CompileCache,
    checked: Vec<CheckedStep>,
    last_tail_ty: Option<Type>,
}

/// 输入中的一个求值步骤: 行内导入或普通语句块.
enum Step {
    Import(Expr),
    Body(Program),
}

/// 已通过类型检查的步骤; 执行阶段按原顺序消费.
///
/// 只在本次输入内使用.
enum CheckedStep {
    Import(CheckedGraph),
    Body(Box<ksl_typed::CheckedProgram>),
}

/// 按顶层顺序把输入拆成导入与语句块.
fn split_steps(program: Program) -> Vec<Step> {
    fn flush(steps: &mut Vec<Step>, body: &mut Vec<Expr>) {
        if !body.is_empty() {
            steps.push(Step::Body(Program {
                stmts: std::mem::take(body),
                tail: None,
            }));
        }
    }

    let mut steps = Vec::new();
    let mut body = Vec::new();
    for stmt in program.stmts {
        if is_use(&stmt.node) {
            flush(&mut steps, &mut body);
            steps.push(Step::Import(stmt));
        } else {
            body.push(stmt);
        }
    }
    if let Some(tail) = program.tail {
        if is_use(&tail.node) {
            flush(&mut steps, &mut body);
            steps.push(Step::Import(tail));
        } else {
            steps.push(Step::Body(Program {
                stmts: body,
                tail: Some(tail),
            }));
            return steps;
        }
    }
    flush(&mut steps, &mut body);
    steps
}

/// 是否为顶层 `@Use` 声明.
fn is_use(node: &ExprKind) -> bool {
    matches!(node, ExprKind::Special(Special::Use { .. }))
}

/// 输出请求是异步的. 在执行边界等待队列清空, 并把首次写出错误并入结果.
///
/// 失败结果保留原错误; 写出错误不覆盖主脚本错误.
fn settle_output<T>(io: &IoRuntime, result: &mut Result<T, RuntimeError>) {
    if let Err(error) = io.flush_output()
        && result.is_ok()
    {
        *result = Err(error);
    }
}

#[cfg(test)]
mod tests;
