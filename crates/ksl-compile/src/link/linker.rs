//! 模块图链接与检查.

use std::collections::HashMap;
use std::path::PathBuf;

use ksl_base::name::Name;
use ksl_base::types::Type;
use ksl_diagnostics::error::KslError;
use ksl_frontend::ast::{ExportItem, Expr, ExprKind, Program, Special};
use ksl_frontend::check::{self, DataTable, Session, TypeDeclCollector, TypeEnv, TypeTarget};

use super::exports::{check_export_type, visible_types};
use super::imports::ModuleLink;
use super::main::{normalize_main, take_main};
use super::{CheckedGraph, CheckedModule, ModuleInterface, PluginType, ValueTypes};
use crate::cache::CompileCache;
use crate::graph::{ModuleGraph, ModuleId, ModuleUnit};

/// 模块检查结果: 导出值类型, typed 程序与可见 Data 声明.
type ModuleCheck = (ValueTypes, ksl_typed::CheckedProgram, DataTable);

struct Linker {
    interfaces: imbl::HashMap<ModuleId, ModuleInterface>,
    /// 模块根目录; 插件身份与路径解析使用.
    root_dir: PathBuf,
}

/// 链接并检查模块图, 复用会话编译缓存中的接口.
pub(crate) fn link_and_check_cached(
    graph: ModuleGraph,
    cache: &CompileCache,
) -> Result<CheckedGraph, KslError> {
    let (root, root_dir, order, mut units) = graph.into_modules();
    let mut linker = Linker {
        interfaces: cache.interfaces.clone(),
        root_dir,
    };
    let mut linked_modules = HashMap::new();
    for id in &order {
        let unit = units.remove(id).ok_or_else(|| {
            KslError::internal(format!("模块顺序包含未载入的模块: {id}"), None)
        })?;
        let module = linker.link_module(&unit)?;
        linked_modules.insert(id.clone(), module);
    }
    Ok(CheckedGraph {
        root,
        order,
        modules: linked_modules,
    })
}

impl Linker {
    // 模块链接步骤对应 `docs/topics/language/modules.md`.
    fn link_module(&mut self, unit: &ModuleUnit) -> Result<CheckedModule, KslError> {
        let mut link = ModuleLink::new(unit, &self.root_dir)?;
        link.collect_imports(unit, &self.interfaces)?;
        let value_refs = link.collect_value_refs(unit)?;
        let mut interface = link.collect_exports(unit, &value_refs)?;
        let plugin_types: HashMap<Name, PluginType> = link
            .type_refs
            .values()
            .filter_map(|target| {
                target.plugin.as_ref().map(|source| {
                    (
                        target.full.clone(),
                        PluginType {
                            source: source.clone(),
                            name: target.name.clone(),
                        },
                    )
                })
            })
            .collect();

        let mut program = unit.program.clone();
        normalize_main(&mut program, unit.id.as_str())?;
        strip_declarations(&mut program);
        let main = take_main(&mut program);

        let (value_types, checked, data) =
            Self::check_module(unit, &program, main.as_ref(), &interface, &link)?;
        interface.value_types = value_types;
        for target in interface.types.values() {
            let TypeTarget::Data(_) = target.target() else {
                continue;
            };
            let info = data.get(&target.full).ok_or_else(|| {
                KslError::internal(
                    format!("导出类型缺少声明: {}", target.full),
                    Some(target.span),
                )
            })?;
            interface.type_decls.insert(target.full.clone(), info.clone());
        }

        self.interfaces.insert(unit.id.clone(), interface.clone());
        let links = program
            .exprs()
            .filter_map(|stmt| match &stmt.node {
                ExprKind::Special(Special::Link(link)) => Some(link.clone()),
                _ => None,
            })
            .collect();
        let ModuleLink {
            link_sources,
            imports,
            imported_value_types,
            imported_bindings,
            imported_data,
            ..
        } = link;
        Ok(CheckedModule {
            id: unit.id.clone(),
            path: unit.path.clone(),
            program: checked,
            links,
            link_sources,
            plugin_types,
            imports,
            imported_value_types,
            imported_bindings,
            imported_data,
            interface,
        })
    }

    /// 规范后的模块程序与导入接口一起检查, 返回类型会话与值类型.
    fn check_module(
        unit: &ModuleUnit,
        program: &ksl_frontend::ast::Program,
        main: Option<&Expr>,
        interface: &ModuleInterface,
        link: &ModuleLink,
    ) -> Result<ModuleCheck, KslError> {
        // 检查环境只装内置声明, 导入声明与本模块声明.
        let mut type_env = TypeEnv::with_data(DataTable::default());
        if let Some((name, binding)) = type_env.bindings_mut().merge(&link.bindings) {
            return Err(KslError::duplicate_import(&name, binding.span));
        }
        type_env.data_mut().merge(&link.imported_data);
        let collected = TypeDeclCollector::new(&mut type_env).collect(program, |name| {
            link.type_refs
                .get(&name)
                .expect("本地 Data 声明已登记")
                .full
                .clone()
        });
        if let Err(error) = collected {
            return Err(error.with_type_names(type_env.type_names()));
        }
        let module_dir = unit
            .path
            .parent()
            .map_or_else(String::new, |dir| dir.to_string_lossy().into_owned());
        let mut session =
            Session::for_module(type_env, link.imported_value_types.clone(), module_dir);
        let mut link_value_types = HashMap::new();
        for stmt in program.exprs() {
            let ExprKind::Special(Special::Link(link)) = &stmt.node else {
                continue;
            };
            for value in &link.values {
                let ty = session.declare_link_value(value.name.node.clone(), &value.ty)?;
                check::validate_link_return(&ty, session.type_bindings()).map_err(
                    |detail| {
                        KslError::plugin_unsupported_signature(
                            &value.name.node,
                            detail,
                            value.name.span,
                        )
                    },
                )?;
                link_value_types.insert(value.name.node.clone(), ty);
            }
        }
        let checked = session.check_loaded_typed_in_place(program, main)?;
        let data = session.type_env().data().clone();
        let value_types =
            collect_value_types(unit, interface, link, &session, &link_value_types)?;

        let type_names = link.type_names();
        let visible = visible_types(interface);
        for item in &unit.exports {
            match item {
                ExportItem::Value(name) => {
                    let span = name.span;
                    let ty = value_types.get(&name.node).ok_or_else(|| {
                        KslError::internal(
                            format!("导出值缺少类型: {}", name.node.as_str()),
                            Some(span),
                        )
                    })?;
                    check_export_type(ty, &visible, &type_names, span)?;
                },
                ExportItem::Type(name) => {
                    let target = interface.types.get(&name.node).ok_or_else(|| {
                        KslError::internal(
                            format!("导出类型缺少接口: {}", name.node.as_str()),
                            Some(name.span),
                        )
                    })?;
                    let TypeTarget::Data(_) = target.target() else {
                        continue;
                    };
                    let info = data.get(&target.full).ok_or_else(|| {
                        KslError::internal(
                            format!("导出类型缺少声明: {}", target.full),
                            Some(name.span),
                        )
                    })?;
                    for fields in info.variants.values() {
                        for field in fields {
                            check_export_type(field, &visible, &type_names, name.span)?;
                        }
                    }
                },
            }
        }
        for link in &unit.links {
            for value in &link.values {
                if let Some(ty) = link_value_types.get(&value.name.node) {
                    check_export_type(ty, &visible, &type_names, value.name.span)?;
                }
            }
        }
        Ok((value_types, checked, data))
    }
}

/// 移除只参与链接的顶层声明; 类型名保留原样, 由检查器按绑定解析.
fn strip_declarations(program: &mut Program) {
    program.stmts.retain(|stmt| {
        !matches!(
            &stmt.node,
            ExprKind::Special(Special::Export { .. } | Special::Use { .. })
        )
    });
    if program.tail.as_ref().is_some_and(|tail| {
        matches!(
            &tail.node,
            ExprKind::Special(Special::Export { .. } | Special::Use { .. })
        )
    }) {
        program.tail = None;
    }
}

/// 汇总接口值的类型与 `@Link` 值的类型.
fn collect_value_types(
    unit: &ModuleUnit,
    interface: &ModuleInterface,
    link: &ModuleLink,
    session: &Session,
    link_value_types: &HashMap<Name, Type>,
) -> Result<ValueTypes, KslError> {
    let mut value_types = HashMap::new();
    for (name, target) in &interface.values {
        let ty = if target.module == unit.id {
            session.value_type(&target.name)
        } else {
            link.imported_value_types.get(name).cloned()
        };
        let ty = ty.ok_or_else(|| {
            KslError::internal(format!("接口值缺少类型: {}", name.as_str()), None)
        })?;
        value_types.insert(name.clone(), ty);
    }
    value_types.extend(
        link_value_types
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone())),
    );
    Ok(value_types)
}
