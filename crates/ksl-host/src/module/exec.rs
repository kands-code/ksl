//! 模块执行: 按依赖顺序在独立环境中执行模块, 再绑定导出值.

use std::collections::HashMap;
use std::sync::Arc;

use ksl_base::name::Name;
use ksl_compile::{CheckedGraph, CheckedModule, ModuleId};
use ksl_diagnostics::error::{ErrorKind, RuntimeError};
use ksl_frontend::check::ModuleContext;
use ksl_runtime::{PumpRegistry, Scheduler};
use ksl_value::value::{Env, Value};

use super::{InstanceCache, ModuleInstance, value_roots};
use crate::plugin::{LibraryKeeper, LinkCache};

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
}

/// 执行已检查的模块图; 根模块执行后只保留导出值.
///
/// # Errors
///
/// 任一模块的运行时错误或 `@Exit`.
#[cfg(test)]
pub fn execute(
    scheduler: &mut Scheduler,
    graph: &CheckedGraph,
    libraries: &mut LibraryKeeper,
) -> Result<Execution, RuntimeError> {
    execute_with_cache(
        scheduler,
        graph,
        RootEnv::OnlyExports,
        &mut InstanceCache::new(),
        libraries,
    )
}

/// 执行已检查的模块图, 复用会话运行缓存中的导出值.
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
    graph: &CheckedGraph,
    root_env: RootEnv,
    cache: &mut InstanceCache,
    libraries: &mut LibraryKeeper,
) -> Result<Execution, RuntimeError> {
    let base = match root_env {
        RootEnv::OnlyExports => Env::new(),
        RootEnv::KeepBindings => scheduler.globals().clone(),
    };
    let values = cache.instances.clone();
    let mut run = ModuleRun {
        graph,
        base,
        links: &mut cache.links,
        libraries,
        values,
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
        main_globals,
        ..
    } = run;
    // 保留全部绑定时沿用根模块执行后的全局环境.
    if root_env == RootEnv::OnlyExports {
        scheduler.set_globals(main_globals);
    }
    let context = match root_env {
        RootEnv::OnlyExports => graph.main_context(),
        RootEnv::KeepBindings => graph.import_context(),
    };
    // 根模块不是可被导入的模块, 不进入会话缓存.
    values.remove(&graph.root);
    cache.update_values(values);
    Ok(Execution {
        main_value: main_value.unwrap_or(Value::Nil),
        main_handler,
        main_mailboxes,
        context,
    })
}

/// 一次模块图执行的可变状态.
struct ModuleRun<'a> {
    graph: &'a CheckedGraph,
    base: Env,
    links: &'a mut LinkCache,
    libraries: &'a mut LibraryKeeper,
    values: imbl::HashMap<ModuleId, ModuleInstance>,
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
                self.libraries,
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
            let value = scheduler.run_linear(ksl_ir::compile(&module.program))?;
            if *id == self.graph.root {
                if module.program.main.is_some() {
                    // 入口声明不是程序值; 单独求值得到根 Actor 蓝图.
                    let entry = module
                        .program
                        .main_entry()
                        .ok_or_else(|| RuntimeError::internal("@Main 缺少有效入口"))?;
                    let handler = scheduler.run_linear(ksl_ir::compile(&entry.program))?;
                    self.main_mailboxes = Some(entry.mailboxes);
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
            self.values.insert(id.clone(), ModuleInstance { exports });
        }
        Ok(())
    }
}

/// 合并两个全局环境; 后者覆盖同名项.
fn merge_envs(base: &Env, extra: &Env) -> Env {
    let mut env = base.clone();
    for (name, value) in extra {
        env.insert(name.clone(), value.clone());
    }
    env
}

fn module_imports(
    module: &CheckedModule,
    values: &imbl::HashMap<ModuleId, ModuleInstance>,
) -> Result<Env, RuntimeError> {
    let mut env = Env::new();
    for (local, target) in &module.imports {
        let value = values
            .get(&target.module)
            .and_then(|instance| instance.exports.get(&target.name))
            .ok_or_else(|| RuntimeError::internal("导入值尚未执行"))?;
        env.insert(local.clone(), value.clone());
    }
    Ok(env)
}

fn load_links(
    module: &CheckedModule,
    links: &mut LinkCache,
    libraries: &mut LibraryKeeper,
    pumps: &Arc<PumpRegistry>,
) -> Result<HashMap<Name, Value>, RuntimeError> {
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
        let plugin = links.load(source, &link.plugin, span, pumps, libraries)?;
        for ty in &link.types {
            if plugin.type_index(ty.node.as_str()).is_none() {
                return Err(RuntimeError::runtime(
                    ErrorKind::PluginMissingExport {
                        name: ty.node.to_string(),
                    },
                    Some(ty.span),
                ));
            }
        }
        for (full, ty) in &module.plugin_types {
            if ty.source.path == source.path && plugin.type_index(ty.name.as_str()).is_none() {
                return Err(RuntimeError::runtime(
                    ErrorKind::PluginMissingExport {
                        name: full.to_string(),
                    },
                    None,
                ));
            }
        }
        for value in &link.values {
            let name = value.name.node.clone();
            let declared = module
                .interface
                .value_types
                .get(&name)
                .ok_or_else(|| RuntimeError::internal("Link 值缺少类型"))?;
            let value = plugin.make_value(name.as_str(), declared, value.name.span)?;
            values.insert(name, value);
        }
    }
    Ok(values)
}

fn module_exports(
    module: &CheckedModule,
    scheduler: &Scheduler,
    values: &imbl::HashMap<ModuleId, ModuleInstance>,
) -> Result<HashMap<Name, Value>, RuntimeError> {
    let mut exports = HashMap::new();
    for (name, target) in module
        .interface
        .values
        .iter()
        .chain(&module.interface.link_values)
    {
        let value = if target.module == module.id {
            scheduler
                .globals()
                .get(&target.name)
                .cloned()
                .ok_or_else(|| RuntimeError::internal("本地导出值尚未绑定"))?
        } else {
            values
                .get(&target.module)
                .and_then(|instance| instance.exports.get(&target.name))
                .cloned()
                .ok_or_else(|| RuntimeError::internal("依赖模块导出值缺失"))?
        };
        exports.insert(name.clone(), value);
    }
    Ok(exports)
}

fn env_from_values(values: &HashMap<Name, Value>) -> Env {
    values
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}
