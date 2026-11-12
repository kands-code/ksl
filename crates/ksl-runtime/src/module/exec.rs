//! 模块执行: 按依赖顺序在独立环境中执行模块, 再绑定导出值.

use std::collections::HashMap;
use std::sync::Arc;

use ksl_base::symbol::{Symbol, resolve};
use ksl_frontend::ast::Program;
use ksl_frontend::check::ModuleContext;
use ksl_frontend::error::{ErrorKind, RuntimeError};

use super::link::{LinkedGraph, LinkedModule};
use super::{ModuleCache, ModuleId, value_roots};
use crate::ffi::LinkCache;
use crate::runtime::scheduler::Scheduler;
use crate::value::{Env, Value};

/// 根模块全局环境的处置方式.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootEnv {
    /// 只保留导出值. 根模块只看到导入值, 执行后全局环境只余导出值.
    OnlyExports,
    /// 保留全部绑定. 根模块看到既有绑定与导入值, 执行后全局环境叠加新绑定.
    KeepBindings,
}

/// 一次根加载的执行结果.
pub struct Execution {
    pub main_value: Value,
    /// 根模块声明了 `@Main` 时的 Handler 蓝图; 由宿主作为根 Actor 启动.
    pub main_handler: Option<Value>,
    /// `@Main` 的 self 信箱数; 仅 `main_handler` 为 Some 时有值.
    pub main_mailboxes: Option<usize>,
    pub context: ModuleContext,
    /// 主模块导出值组成的环境. 只保留导出值时为导出值, 保留全部绑定时为执行后的全局环境.
    pub globals: Env,
    /// 本次执行中的模块导出值, 包含复用的缓存模块.
    pub module_values: imbl::HashMap<ModuleId, HashMap<Symbol, Value>>,
}

/// 执行已链接的模块图; 根模块执行后只保留导出值.
///
/// # Errors
///
/// 任一模块的运行时错误或 `@Exit`.
pub fn execute(
    scheduler: &mut Scheduler,
    graph: &LinkedGraph,
) -> Result<Execution, RuntimeError> {
    execute_with_cache(scheduler, graph, RootEnv::OnlyExports, &mut ModuleCache::new())
}

/// 执行已链接的模块图; 根模块的全局环境按 `root_env` 处置.
///
/// 保留全部绑定时根模块沿用执行前的全局绑定, 导入值并入其中.
/// 执行失败时全局绑定回滚到执行前的状态, 导入值不留痕迹.
///
/// # Errors
///
/// 任一模块的运行时错误或 `@Exit`.
pub fn execute_with(
    scheduler: &mut Scheduler,
    graph: &LinkedGraph,
    root_env: RootEnv,
) -> Result<Execution, RuntimeError> {
    let mut cache = ModuleCache::new();
    execute_with_cache(scheduler, graph, root_env, &mut cache)
}

/// 执行已链接的模块图, 复用会话模块缓存中的导出值.
///
/// 新模块执行成功后把导出值写回缓存; 失败时缓存保持不变.
///
/// 函数返回后继续执行时, 调用方须把缓存导出值登记为环回收根.
///
/// # Errors
///
/// 任一模块的运行时错误或 `@Exit`.
pub fn execute_with_cache(
    scheduler: &mut Scheduler,
    graph: &LinkedGraph,
    root_env: RootEnv,
    cache: &mut ModuleCache,
) -> Result<Execution, RuntimeError> {
    let base = match root_env {
        RootEnv::OnlyExports => Env::new(),
        RootEnv::KeepBindings => scheduler.globals().clone(),
    };
    let mut run = ModuleRun {
        graph,
        base,
        links: &mut cache.links,
        values: cache.values.clone(),
        main_value: None,
        main_handler: None,
        main_mailboxes: None,
        main_globals: Env::new(),
    };
    if let Err(err) = run.run(scheduler) {
        // 失败时回滚全局绑定, 导入值与模块副作用留下的绑定一并撤销.
        scheduler.set_globals(run.base);
        return Err(err);
    }
    let ModuleRun {
        mut values,
        main_value,
        main_handler,
        main_mailboxes,
        mut main_globals,
        ..
    } = run;
    match root_env {
        RootEnv::OnlyExports => scheduler.set_globals(main_globals.clone()),
        // 保留全部绑定时沿用根模块执行后的全局环境, 既有绑定与导入值都在其中.
        RootEnv::KeepBindings => main_globals = scheduler.globals().clone(),
    }
    let context = match root_env {
        RootEnv::OnlyExports => graph.main_context(),
        RootEnv::KeepBindings => graph.import_context(),
    };
    let module_values = values.clone();
    // 根模块不是可被导入的模块, 不进入会话缓存.
    values.remove(&graph.root);
    cache.update_values(values);
    Ok(Execution {
        main_value: main_value.unwrap_or(Value::Nil),
        main_handler,
        main_mailboxes,
        context,
        globals: main_globals,
        module_values,
    })
}

/// 一次模块图执行的可变状态.
struct ModuleRun<'a> {
    graph: &'a LinkedGraph,
    base: Env,
    links: &'a mut LinkCache,
    values: imbl::HashMap<ModuleId, HashMap<Symbol, Value>>,
    main_value: Option<Value>,
    main_handler: Option<Value>,
    main_mailboxes: Option<usize>,
    main_globals: Env,
}

impl ModuleRun<'_> {
    /// 按依赖顺序执行模块, 并把导出值交给后继模块.
    fn run(&mut self, scheduler: &mut Scheduler) -> Result<(), RuntimeError> {
        for id in &self.graph.order {
            let module = self
                .graph
                .get(id)
                .ok_or_else(|| RuntimeError::internal("模块图缺少已链接模块"))?;
            let mut imports = module_imports(module, &self.values)?;
            imports.extend(load_links(
                module,
                self.links,
                scheduler.releases(),
                scheduler.pumps(),
            )?);
            let env = if *id == self.graph.root {
                // 交互模式下根模块沿用既有绑定, 导入值并入其中.
                merge_envs(&self.base, &imports)
            } else {
                imports
            };
            scheduler.set_globals(env);
            // 执行中途收集时, 已执行模块的导出值可能只存在于 values 与 base 中.
            let _base_scope = scheduler.root_env_scope(&self.base);
            let _values_scope = scheduler.root_scope(value_roots(&self.values));
            let value = scheduler.run_program_in_root(&module.program, &module.types)?;
            if *id == self.graph.root {
                if let Some(main) = &module.main {
                    // 入口声明不是程序值; 单独求值得到根 Actor 蓝图.
                    let entry = Program {
                        stmts: Vec::new(),
                        tail: Some(main.expr.clone()),
                    };
                    let handler = scheduler.run_program_in_root(&entry, &module.types)?;
                    let Some((mailboxes, _)) = main.handler.pid.as_pid() else {
                        return Err(RuntimeError::internal("@Main 的 self 不是 Pid"));
                    };
                    self.main_mailboxes = Some(mailboxes.len());
                    self.main_handler = Some(handler);
                    // 顶层语句的值不作为程序结果; 根 Actor 的值才是.
                    self.main_value = Some(Value::Nil);
                } else {
                    self.main_value = Some(value);
                }
            }
            let exports = module_exports(module, scheduler, &self.values)?;
            if *id == self.graph.root {
                self.main_globals = env_from_values(&exports);
            }
            self.values.insert(id.clone(), exports);
        }
        Ok(())
    }
}

/// 合并两个全局环境; 后者覆盖同名项.
fn merge_envs(base: &Env, extra: &Env) -> Env {
    let mut env = base.clone();
    for (name, value) in extra {
        env.insert(*name, value.clone());
    }
    env
}

fn module_imports(
    module: &LinkedModule,
    values: &imbl::HashMap<ModuleId, HashMap<Symbol, Value>>,
) -> Result<Env, RuntimeError> {
    let mut env = Env::new();
    for (local, target) in &module.imports {
        let value = values
            .get(&target.module)
            .and_then(|exports| exports.get(&target.name))
            .ok_or_else(|| RuntimeError::internal("导入值尚未执行"))?;
        env.insert(*local, value.clone());
    }
    Ok(env)
}

fn load_links(
    module: &LinkedModule,
    links: &mut LinkCache,
    releases: &Arc<crate::ffi::ReleaseQueue>,
    pumps: &Arc<crate::runtime::pump::PumpRegistry>,
) -> Result<HashMap<Symbol, Value>, RuntimeError> {
    let mut values = HashMap::new();
    for link in &module.links {
        if link.values.is_empty() {
            continue;
        }
        let span = link.values[0].name.span;
        let source = module
            .link_sources
            .get(&link.plugin)
            .ok_or_else(|| RuntimeError::internal("Link 缺少插件来源"))?;
        let plugin = links.load(source, &link.plugin, span, releases, pumps)?;
        let mut type_map = HashMap::new();
        for ty in &link.types {
            let name = resolve(ty.node);
            let index = plugin.type_index(&name).ok_or_else(|| {
                RuntimeError::runtime(
                    ErrorKind::PluginMissingExport {
                        name: name.to_string(),
                    },
                    Some(ty.span),
                )
            })?;
            type_map.insert(ty.node, index);
            if let Some(target) = module.exports.link_types.get(&ty.node) {
                type_map.insert(target.full, index);
            }
        }
        // 导入的 Link 类型按插件来源共享; 只映射同一份动态库的类型.
        for (full, ty) in &module.plugin_types {
            if ty.source.path == source.path
                && let Some(index) = plugin.type_index(&resolve(ty.name))
            {
                type_map.insert(*full, index);
            }
        }
        for value in &link.values {
            let name = value.name.node;
            let declared = module
                .value_types
                .get(&name)
                .ok_or_else(|| RuntimeError::internal("Link 值缺少类型"))?;
            let value = plugin.make_value(
                &resolve(name),
                declared,
                &type_map,
                value.name.span,
                releases,
            )?;
            values.insert(name, value);
        }
    }
    Ok(values)
}

fn module_exports(
    module: &LinkedModule,
    scheduler: &Scheduler,
    values: &imbl::HashMap<ModuleId, HashMap<Symbol, Value>>,
) -> Result<HashMap<Symbol, Value>, RuntimeError> {
    let mut exports = HashMap::new();
    for (name, target) in module.exports.values.iter().chain(&module.exports.link_values) {
        let value = if target.module == module.id {
            scheduler
                .globals()
                .get(&target.name)
                .cloned()
                .ok_or_else(|| RuntimeError::internal("本地导出值尚未绑定"))?
        } else {
            values
                .get(&target.module)
                .and_then(|module| module.get(&target.name))
                .cloned()
                .ok_or_else(|| RuntimeError::internal("依赖模块导出值缺失"))?
        };
        exports.insert(*name, value);
    }
    Ok(exports)
}

fn env_from_values(values: &HashMap<Symbol, Value>) -> Env {
    values
        .iter()
        .map(|(name, value)| (*name, value.clone()))
        .collect()
}
