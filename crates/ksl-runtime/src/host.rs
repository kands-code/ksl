//! 宿主状态: 把源码加载, 检查与执行串成一条路径.
//!
//! 批处理与交互共用同一套状态.
//! 交互输入在会话副本上检查, 运行成功后才提交类型环境与模块缓存.
//! 宿主只返回结构化结果, 诊断渲染由调用方负责.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ksl_base::source::Sources;
use ksl_frontend::ast::{Expr, ExprKind, Program, Special};
use ksl_frontend::check::{self, Session, Type, TypeTable};
use ksl_frontend::error::{ErrorKind, KslError, RuntimeError};

use crate::io::{IoConfig, IoRuntime};
use crate::module::{self, LinkedGraph, ModuleCache, RootEnv};
use crate::runtime::scheduler::{RootGuard, Scheduler};
use crate::value::Value;

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
    modules: ModuleCache,
    /// 已提交模块导出值的根; 模块缓存替换时同步更新.
    module_roots: Option<RootGuard>,
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
            modules: ModuleCache::new(),
            module_roots: None,
            root_dir,
            repl_mode: false,
        }
    }

    /// 以当前工作目录建立空宿主.
    #[must_use]
    pub fn in_current_dir() -> Self {
        let root_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(root_dir)
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
    ) -> Result<crate::terminal::Session, RuntimeError> {
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

    /// 设置脚本命令行参数.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.scheduler.set_args(args);
    }

    /// 执行文件形式的脚本.
    ///
    /// 加载, 检查与执行都在宿主状态上进行.
    /// 成功后提交模块缓存与会话, 失败时保持原状.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn run_file(&mut self, file: &Path) -> Result<RunOutcome, KslError> {
        self.run_file_inner(file, true)
    }

    /// 加载初始化文件, 执行顶层语句但不运行 `@Main`.
    ///
    /// REPL 的初始化文件是模块加载而非直接运行, 只保留导出供交互会话使用.
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
        // 即使加载失败, 后续交互输入的行内 `@Use` 仍应以初始化文件所在目录为根.
        let root_dir = module::root_dir_of(file);
        if root_dir != self.root_dir {
            // 缓存按根相对路径索引, 更换根目录后旧条目不再有效.
            self.modules = ModuleCache::new();
            self.module_roots = None;
        }
        self.root_dir = root_dir;
        let mut trial_modules = self.modules.clone();
        let graph = module::load_and_check_file_with_cache(
            file,
            &mut self.sources,
            &mut trial_modules,
        )?;
        let checkpoint = self.scheduler.snapshot_globals();
        let execution = module::execute_with_cache(
            &mut self.scheduler,
            &graph,
            RootEnv::OnlyExports,
            &mut trial_modules,
        );
        // 模块导出值在提交前只由本次执行与缓存副本持有; 执行与根 Actor 运行期间都须作为根.
        let module_roots = self.scheduler.root_scope(trial_modules.root_values());
        let mut context = None;
        let mut main_handler = None;
        let mut main_mailboxes = None;
        let mut result = match execution {
            Ok(execution) => {
                context = Some(execution.context);
                main_handler = execution.main_handler;
                main_mailboxes = execution.main_mailboxes;
                Ok(execution.main_value)
            },
            Err(error) => Err(error),
        };
        if execute_main && let Some(handler) = main_handler {
            match main_mailboxes {
                Some(mailboxes) => result = self.scheduler.run_main(handler, mailboxes),
                None => result = Err(RuntimeError::internal("Main 缺少信箱数")),
            }
        }
        let actor_errors = self.scheduler.take_actor_errors();
        settle_output(&self.io, &mut result);
        self.io.restore_terminal();
        if result.is_ok() {
            if let Some(context) = context {
                self.modules = trial_modules;
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
        reject_export(&program)?;
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
    pub fn check_program(&self, program: &mut Program) -> Result<Option<Type>, KslError> {
        let mut trial = self.session.clone();
        check::apply_type_names(&mut trial, program);
        trial.check_with_tail(program)
    }

    /// 在输入边界回收不可达环.
    ///
    /// 模块缓存导出值由宿主持有根作用域, 不必作为外部根传入.
    /// 调用方在宿主之外持有的值不是根; 跨输入持有闭包或容器前,
    /// 应先避免在持有期间触发自动收集.
    pub fn collect_cycles(&mut self) {
        self.scheduler.collect_cycles(&[]);
    }

    /// 求值一段交互输入.
    ///
    /// 输入中的 `@Use` 按所在位置加载并执行.
    /// 类型检查在会话副本上进行, 运行成功后才提交类型环境与模块缓存.
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
        self.modules = ModuleCache::new();
        self.sources = Sources::default();
    }

    /// 终止调度器, 清理 Actor, 并等待已排队的输出写出.
    pub fn shutdown(&mut self) {
        self.scheduler.shutdown();
        let _ = self.io.flush_output();
    }

    /// 语法与类型检查阶段; 不修改会话与模块缓存.
    fn prepare_input(&mut self, name: &str, text: &str) -> Result<PreparedInput, KslError> {
        let program = parse(&mut self.sources, name, text)?;
        reject_export(&program)?;
        self.prepare_steps(name, split_steps(program))
    }

    /// 检查一段代码段; `@Use` 提升到顶部并合并为一次虚拟根加载.
    fn prepare_code(
        &mut self,
        name: &str,
        program: Program,
    ) -> Result<PreparedInput, KslError> {
        let (uses, mut body) = split_uses(program);
        let mut trial = self.session.clone();
        let mut trial_modules = self.modules.clone();
        let mut checked = Vec::new();
        let mut input_types = trial.types().clone();
        if !uses.is_empty() {
            let graph = self.load_imports(
                name,
                uses,
                &mut trial,
                &mut trial_modules,
                &mut input_types,
            )?;
            checked.push(CheckedStep::Import(graph));
        }
        let last_tail_ty = check_body(&mut trial, &mut body, &mut input_types)?;
        checked.push(CheckedStep::Body(body));
        Ok(PreparedInput {
            trial,
            modules: trial_modules,
            checked,
            last_tail_ty,
            input_types,
        })
    }

    /// 按顶层顺序检查导入与语句块.
    fn prepare_steps(
        &mut self,
        name: &str,
        steps: Vec<Step>,
    ) -> Result<PreparedInput, KslError> {
        let mut trial = self.session.clone();
        let mut trial_modules = self.modules.clone();
        let mut checked = Vec::new();
        let mut last_tail_ty = None;
        let mut input_types = trial.types().clone();
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
                    let graph = self.load_imports(
                        name,
                        vec![expr],
                        &mut trial,
                        &mut trial_modules,
                        &mut input_types,
                    )?;
                    checked.push(CheckedStep::Import(graph));
                    last_tail_ty = None;
                },
                Step::Body(mut body) => {
                    last_tail_ty = check_body(&mut trial, &mut body, &mut input_types)?;
                    checked.push(CheckedStep::Body(body));
                },
            }
        }
        Ok(PreparedInput {
            trial,
            modules: trial_modules,
            checked,
            last_tail_ty,
            input_types,
        })
    }

    /// 加载一组导入并并入会话; 类型表累积到 `input_types`.
    fn load_imports(
        &mut self,
        name: &str,
        uses: Vec<Expr>,
        trial: &mut Session,
        modules: &mut ModuleCache,
        input_types: &mut TypeTable,
    ) -> Result<LinkedGraph, KslError> {
        let root = Program {
            stmts: uses,
            tail: None,
        };
        let graph = module::load_and_check_virtual_with_cache(
            &self.root_dir,
            name,
            root,
            &mut self.sources,
            modules,
        )?;
        trial.merge_module_context(graph.import_context())?;
        input_types.extend(trial.types().iter().map(|(key, ty)| (*key, ty.clone())));
        Ok(graph)
    }

    /// 执行已检查的输入, 成功后才提交会话与模块缓存.
    fn execute_steps(&mut self, prepared: PreparedInput) -> InputOutcome {
        let PreparedInput {
            mut trial,
            mut modules,
            checked,
            last_tail_ty,
            input_types,
        } = prepared;
        let checkpoint = self.scheduler.snapshot_globals();
        let mut outcome = Ok(Value::Nil);
        for step in checked {
            // 逐步丢弃上一步结果, 避免下一步执行期间继续持有.
            drop(std::mem::replace(&mut outcome, Ok(Value::Nil)));
            let _module_scope = self.scheduler.root_scope(modules.root_values());
            let step_result = match step {
                CheckedStep::Import(graph) => module::execute_with_cache(
                    &mut self.scheduler,
                    &graph,
                    RootEnv::KeepBindings,
                    &mut modules,
                )
                .map(|_| Value::Nil),
                CheckedStep::Body(body) => {
                    self.scheduler.run_program_in_root(&body, &input_types)
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
                trial.replace_types(input_types);
                self.session = trial;
                self.modules = modules;
                self.module_roots = Some(self.scheduler.root_scope(self.modules.root_values()));
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

/// 检查一段语句并把本轮类型并入累积表.
fn check_body(
    trial: &mut Session,
    body: &mut Program,
    input_types: &mut TypeTable,
) -> Result<Option<Type>, KslError> {
    check::apply_type_names(trial, body);
    let tail = trial.check_with_tail(body)?;
    input_types.extend(trial.types().iter().map(|(key, ty)| (*key, ty.clone())));
    Ok(tail)
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
/// 导入声明只允许出现在顶层, 故只扫描顶层语句.
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

/// 报告模块之外的模块级声明.
///
/// 交互输入与命令行代码都不是模块, 不能声明导出, 链接或 `@Main`.
///
/// # Errors
///
/// 程序含 `@Export`, `@Link` 或 `@Main` 时返回对应错误.
pub fn reject_export(program: &Program) -> Result<(), KslError> {
    for stmt in program.exprs() {
        match &stmt.node {
            ExprKind::Special(Special::Export { .. }) => {
                return Err(KslError::new(ErrorKind::ExportOutsideModule, stmt.span));
            },
            ExprKind::Special(Special::Link(_)) => {
                return Err(KslError::new(ErrorKind::LinkOutsideModule, stmt.span));
            },
            ExprKind::Special(Special::Main { .. }) => {
                return Err(KslError::new(ErrorKind::MainOutsideModule, stmt.span));
            },
            _ => {},
        }
    }
    Ok(())
}

/// 已通过检查, 等待提交的输入.
struct PreparedInput {
    trial: Session,
    modules: ModuleCache,
    checked: Vec<CheckedStep>,
    last_tail_ty: Option<Type>,
    input_types: TypeTable,
}

/// 输入中的一个求值步骤: 行内导入或普通语句块.
enum Step {
    Import(Expr),
    Body(Program),
}

/// 已通过类型检查的步骤; 执行阶段按原顺序消费.
enum CheckedStep {
    Import(LinkedGraph),
    Body(Program),
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
/// 失败结果已有原因时保留原错误, 写出错误不覆盖更深层的主脚本错误.
fn settle_output<T>(io: &IoRuntime, result: &mut Result<T, RuntimeError>) {
    if let Err(error) = io.flush_output()
        && result.is_ok()
    {
        *result = Err(error);
    }
}

#[cfg(test)]
mod tests;
