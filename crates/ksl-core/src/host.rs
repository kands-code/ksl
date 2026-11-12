//! 宿主状态: 把源码加载, 检查与执行串成一条路径.
//!
//! 批处理与交互共用同一套状态. 交互输入在会话副本上检查, 运行成功后
//! 才提交类型环境与模块缓存. 宿主只返回结构化结果, 诊断渲染由调用方负责.

use std::path::{Path, PathBuf};

use crate::ast::{Expr, ExprKind, Program, Special};
use crate::check::{self, Session, Type, TypeTable};
use crate::cps::scheduler::Scheduler;
use crate::error::{ErrorKind, KslError, RuntimeError};
use crate::module::{self, LinkedGraph, ModuleCache, RootEnv};
use crate::source::Sources;
use crate::value::Value;

/// 一次执行的结局.
pub struct RunOutcome {
    /// 主脚本的结果; `@Exit` 与运行时错误在这里.
    pub result: Result<Value, RuntimeError>,
    /// Actor 错误; 不影响主脚本结局与退出码.
    pub actor_errors: Vec<RuntimeError>,
}

/// 一段交互输入的结局.
pub struct InputOutcome {
    /// 成功时给出尾表达式的值与类型; 失败, `@Exit` 与输出流关闭在错误侧.
    pub result: Result<(Value, Option<Type>), RuntimeError>,
    /// Actor 错误; 不影响主脚本结局与退出码.
    pub actor_errors: Vec<RuntimeError>,
}

/// 宿主: 持有一次进程内语言运行所需的全部状态.
pub struct Host {
    sources: Sources,
    session: Session,
    scheduler: Scheduler,
    modules: ModuleCache,
    root_dir: PathBuf,
}

impl Host {
    /// 以给定根目录建立空宿主.
    #[must_use]
    pub fn new(root_dir: PathBuf) -> Self {
        Self {
            sources: Sources::default(),
            session: Session::new(),
            scheduler: Scheduler::new(),
            modules: ModuleCache::new(),
            root_dir,
        }
    }

    /// 以当前工作目录建立空宿主.
    #[must_use]
    pub fn in_current_dir() -> Self {
        let root_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(root_dir)
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

    /// 设置脚本命令行参数.
    pub fn set_args(&mut self, args: Vec<String>) {
        self.scheduler.set_args(args);
    }

    /// 执行文件形式的脚本.
    ///
    /// 加载, 检查与执行都在宿主状态上进行; 成功后提交模块缓存与会话,
    /// 失败时保持原状.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn run_file(&mut self, file: &Path) -> Result<RunOutcome, KslError> {
        // 即使加载失败, 后续交互输入的行内 @Use 仍应以初始化文件所在目录为根.
        self.root_dir = module::root_dir_of(file);
        let mut trial_modules = self.modules.clone();
        let graph = module::load_and_check_file_with_cache(
            file,
            &mut self.sources,
            &mut trial_modules,
        )?;
        let execution = module::execute_with_cache(
            &mut self.scheduler,
            &graph,
            RootEnv::OnlyExports,
            &mut trial_modules,
        );
        let actor_errors = self.scheduler.take_actor_errors();
        match execution {
            Ok(execution) => {
                self.modules = trial_modules;
                self.session = Session::from_module_context(execution.context);
                Ok(RunOutcome {
                    result: Ok(execution.main_value),
                    actor_errors,
                })
            },
            Err(error) => Ok(RunOutcome {
                result: Err(error),
                actor_errors,
            }),
        }
    }

    /// 执行一段脚本代码.
    ///
    /// 脚本中的 `@Use` 逻辑上位于顶部; 与文件脚本不同, 代码段不是模块,
    /// 不能声明 `@Export`.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn run_code(&mut self, name: &str, code: &str) -> Result<RunOutcome, KslError> {
        let program = parse(&mut self.sources, name, code)?;
        reject_export(&program)?;
        let (uses, mut body) = split_uses(program);

        let mut trial_modules = self.modules.clone();
        let graph = if uses.is_empty() {
            None
        } else {
            let root = Program {
                stmts: uses,
                tail: None,
            };
            Some(module::load_and_check_virtual_with_cache(
                &self.root_dir,
                name,
                root,
                &mut self.sources,
                &mut trial_modules,
            )?)
        };

        let mut session = self.session.clone();
        if let Some(graph) = &graph {
            session.merge_module_context(graph.import_context())?;
        }
        check::apply_type_names(&mut session, &mut body);
        session.check(&body)?;

        let checkpoint = self.scheduler.snapshot_globals();
        self.scheduler.begin_root();
        let result = match &graph {
            Some(graph) => module::execute_with_cache_in_root(
                &mut self.scheduler,
                graph,
                RootEnv::KeepBindings,
                &mut trial_modules,
            )
            .and_then(|_| self.scheduler.run_program_in_root(&body, session.types())),
            None => self.scheduler.run_program_in_root(&body, session.types()),
        };
        let actor_errors = self.scheduler.take_actor_errors();
        self.scheduler.finish_root(result.is_ok());
        if result.is_err() {
            self.scheduler.restore_globals(checkpoint);
        } else {
            self.session = session;
            self.modules = trial_modules;
        }
        Ok(RunOutcome {
            result,
            actor_errors,
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

    /// 在输入边界回收不可达环; 模块缓存导出值作为外部根.
    pub fn collect_cycles(&mut self) {
        let roots = self.modules.root_values();
        self.scheduler.collect_cycles(&roots);
    }

    /// 是否仍有任务在等待标准输入.
    #[must_use]
    pub fn pending_input(&self) -> bool {
        self.scheduler.pending_input() > 0
    }

    /// 推进 Actor, 直到标准输入请求清空; 返回其间产生的 Actor 错误.
    pub fn pump_program(&mut self) -> Vec<RuntimeError> {
        // 模块导出值只由宿主持有, 推进期间自动回收时须作为额外根.
        self.scheduler.push_extra_roots(self.modules.root_values());
        self.scheduler.pump_program();
        self.scheduler.pop_extra_roots();
        self.scheduler.take_actor_errors()
    }

    /// 求值一段交互输入.
    ///
    /// 输入中的 `@Use` 按所在位置加载并执行; 类型检查在会话副本上进行,
    /// 运行成功后才提交类型环境与模块缓存. 运行时失败回滚全局绑定.
    ///
    /// # Errors
    ///
    /// 词法, 语法, 类型检查或模块加载错误.
    pub fn eval_input(&mut self, name: &str, text: &str) -> Result<InputOutcome, KslError> {
        let prepared = self.prepare_input(name, text)?;
        Ok(self.execute_input(prepared))
    }

    /// 清空会话状态; 保留根目录.
    pub fn reset(&mut self) {
        self.scheduler.shutdown();
        self.scheduler = Scheduler::new();
        self.session = Session::new();
        self.modules = ModuleCache::new();
        self.sources = Sources::default();
    }

    /// 终止调度器并清理 Actor.
    pub fn shutdown(&mut self) {
        self.scheduler.shutdown();
    }

    /// 语法与类型检查阶段; 不修改会话与模块缓存.
    fn prepare_input(&mut self, name: &str, text: &str) -> Result<PreparedInput, KslError> {
        let mut trial = self.session.clone();
        let program = parse(&mut self.sources, name, text)?;
        reject_export(&program)?;

        let mut trial_modules = self.modules.clone();
        let mut checked = Vec::new();
        let mut last_tail_ty = None;
        let mut input_types = trial.types().clone();
        for step in split_steps(program) {
            match step {
                Step::Import(expr) => {
                    let empty_use = matches!(
                        &expr.node,
                        ExprKind::Special(Special::Use { items, .. }) if items.is_empty()
                    );
                    if empty_use {
                        continue;
                    }
                    let root = Program {
                        stmts: vec![expr],
                        tail: None,
                    };
                    let graph = module::load_and_check_virtual_with_cache(
                        &self.root_dir,
                        name,
                        root,
                        &mut self.sources,
                        &mut trial_modules,
                    )?;
                    trial.merge_module_context(graph.import_context())?;
                    input_types
                        .extend(trial.types().iter().map(|(key, ty)| (*key, ty.clone())));
                    checked.push(CheckedStep::Import(graph));
                    last_tail_ty = None;
                },
                Step::Body(mut body) => {
                    check::apply_type_names(&mut trial, &mut body);
                    let tail_ty = trial.check_with_tail(&body)?;
                    last_tail_ty = tail_ty;
                    input_types
                        .extend(trial.types().iter().map(|(key, ty)| (*key, ty.clone())));
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

    /// 执行已检查的输入, 成功后才提交会话与模块缓存.
    fn execute_input(&mut self, prepared: PreparedInput) -> InputOutcome {
        let PreparedInput {
            mut trial,
            mut modules,
            checked,
            last_tail_ty,
            input_types,
        } = prepared;
        let checkpoint = self.scheduler.snapshot_globals();
        let began_root = !checked.is_empty();
        if began_root {
            self.scheduler.begin_root();
        }
        // 执行中途收集时, 回滚快照只存在于本栈帧.
        self.scheduler.push_extra_env(&checkpoint);
        let mut outcome = Ok(Value::Nil);
        for step in checked {
            // 上一步结果不再需要, 先丢弃再进入下一步的执行.
            drop(std::mem::replace(&mut outcome, Ok(Value::Nil)));
            self.scheduler.push_extra_roots(modules.root_values());
            let step_result = match step {
                CheckedStep::Import(graph) => module::execute_with_cache_in_root(
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
            self.scheduler.pop_extra_roots();
            match step_result {
                Ok(value) => outcome = Ok(value),
                Err(error) => {
                    outcome = Err(error);
                    break;
                },
            }
        }
        self.scheduler.pop_extra_roots();
        let actor_errors = self.scheduler.take_actor_errors();
        if began_root {
            // 多步输入中任一步失败时, 终止本轮全部步骤启动的 Actor.
            self.scheduler.finish_root(outcome.is_ok());
        }
        match outcome {
            Ok(value) => {
                trial.replace_types(input_types);
                self.session = trial;
                self.modules = modules;
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

/// 解析一段源码, 返回语法树.
///
/// # Errors
///
/// 词法或语法失败.
pub fn parse(sources: &mut Sources, name: &str, text: &str) -> Result<Program, KslError> {
    let id = sources.add(name, text);
    crate::parser::parse_text(sources.text(id), id)
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
        if matches!(stmt.node, ExprKind::Special(Special::Use { .. })) {
            uses.push(stmt);
        } else {
            rest.push(stmt);
        }
    }
    if tail
        .as_ref()
        .is_some_and(|expr| matches!(&expr.node, ExprKind::Special(Special::Use { .. })))
    {
        uses.extend(tail.take());
    }
    (uses, Program { stmts: rest, tail })
}

/// 报告模块之外的导出声明.
///
/// 交互输入与命令行代码都不是模块, 不能声明导出.
///
/// # Errors
///
/// 程序含导出声明时返回对应错误.
pub fn reject_export(program: &Program) -> Result<(), KslError> {
    for stmt in program.exprs() {
        let ExprKind::Special(Special::Export { .. }) = &stmt.node else {
            continue;
        };
        return Err(KslError::new(ErrorKind::ExportOutsideModule, stmt.span));
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
    let mut steps = Vec::new();
    let mut body = Vec::new();
    for stmt in program.stmts {
        if matches!(stmt.node, ExprKind::Special(Special::Use { .. })) {
            if !body.is_empty() {
                steps.push(Step::Body(Program {
                    stmts: std::mem::take(&mut body),
                    tail: None,
                }));
            }
            steps.push(Step::Import(stmt));
        } else {
            body.push(stmt);
        }
    }
    if let Some(tail) = program.tail {
        if matches!(tail.node, ExprKind::Special(Special::Use { .. })) {
            if !body.is_empty() {
                steps.push(Step::Body(Program {
                    stmts: std::mem::take(&mut body),
                    tail: None,
                }));
            }
            steps.push(Step::Import(tail));
        } else {
            steps.push(Step::Body(Program {
                stmts: body,
                tail: Some(tail),
            }));
            return steps;
        }
    }
    if !body.is_empty() {
        steps.push(Step::Body(Program {
            stmts: body,
            tail: None,
        }));
    }
    steps
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testutil::TempDir;

    /// 多步输入中任一步失败时, 本轮更早步骤启动的 Actor 也必须终止.
    ///
    /// 模块导入会执行依赖并可能启动 Actor; 随后语句失败时, 若只清理失败
    /// 步骤自己的 Spawn, 这些 Actor 将变成无法再通过 Pid 访问的孤立任务.
    #[test]
    fn failed_input_kills_actors_from_earlier_steps() {
        let root = TempDir::new("ksl_host_rollback_");
        fs::create_dir_all(root.path().join("lib")).expect("模块目录应可创建");
        fs::write(
            root.path().join("lib/spawn.ksl"),
            r"
                @Let[worker, @Handler[[self : Pid[i64, i64]] : i64,
                    @Match[@Receive[0u, Option/None[]], {
                        (/Some[x], x),
                        (/None[], 0)
                    }]]];
                @Let[p, @SpawnDetached[1u, worker]];
                @Export[p];
            ",
        )
        .expect("模块文件应可写入");

        let mut host = Host::new(root.path().to_path_buf());
        let outcome = host
            .eval_input("<交互>", "@Use[{p}, \"lib/spawn\"]; DivideI64[1, 0]")
            .expect("输入应可解析与检查");
        assert!(outcome.result.is_err(), "第二步应失败");
        assert_eq!(
            host.scheduler.actor_count(),
            0,
            "失败输入应终止本轮启动的全部 Actor"
        );
        host.shutdown();
    }
}
