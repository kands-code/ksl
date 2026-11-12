//! 模块图链接与检查.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use ksl_base::symbol::{Symbol, resolve};
use ksl_frontend::ast::{ExportItem, Expr, ExprKind, Special};
use ksl_frontend::check::{self, DataTable, Session};
use ksl_frontend::error::{ErrorKind, KslError};
use ksl_frontend::rewrite::{self, TypeNames};

use super::exports::{build_module_types, check_export_type, visible_types};
use super::imports::ModuleLink;
use super::main::{normalize_main, take_main};
use super::rewriting::rewrite_program;
use super::{
    CachedValueTypes, Exports, LinkedGraph, LinkedModule, MainEntry, ModuleValueTypes,
    PluginType,
};
use crate::module::{ModuleCache, ModuleGraph, ModuleId, ModuleUnit};

/// 模块检查结果: 类型会话, 导出值类型与 Link 值类型.
type ModuleCheck = (Session, ModuleValueTypes, ModuleValueTypes, Option<MainEntry>);

struct Linker {
    interfaces: imbl::HashMap<ModuleId, Exports>,
    value_types: CachedValueTypes,
    data: DataTable,
    /// 模块根目录; 插件身份与路径解析使用.
    root_dir: PathBuf,
}

/// 链接并检查模块图; 返回按依赖顺序排列的已检查模块.
pub(crate) fn link_and_check(graph: ModuleGraph) -> Result<LinkedGraph, KslError> {
    let cache = ModuleCache::new();
    link_and_check_cached(graph, &cache)
}

/// 链接并检查模块图, 复用会话模块缓存中的接口与类型.
pub(crate) fn link_and_check_cached(
    graph: ModuleGraph,
    cache: &ModuleCache,
) -> Result<LinkedGraph, KslError> {
    let (root, root_dir, order, mut units) = graph.into_modules();
    let mut linker = Linker {
        interfaces: cache.interfaces.clone(),
        value_types: cache.value_types.clone(),
        data: cache.data.clone(),
        root_dir,
    };
    // 依赖优先排序, 根模块在末尾; 根模块之前累积的 Data 表即缓存内容.
    debug_assert_eq!(order.last(), Some(&root), "根模块应位于依赖顺序末尾");
    let mut linked_modules = HashMap::new();
    let mut cache_data = linker.data.clone();
    for id in &order {
        if id == &root {
            cache_data = linker.data.clone();
        }
        let unit = units.remove(id).ok_or_else(|| {
            KslError::without_span(ErrorKind::ExportUnknown {
                name: id.to_string(),
            })
        })?;
        let module = linker.link_module(&unit)?;
        linked_modules.insert(id.clone(), module);
    }
    Ok(LinkedGraph {
        root,
        order,
        modules: linked_modules,
        data: linker.data,
        cache_data,
    })
}

impl Linker {
    // 模块链接步骤对应 `docs/topics/language/modules.md`.
    fn link_module(&mut self, unit: &ModuleUnit) -> Result<LinkedModule, KslError> {
        let mut link = ModuleLink::new(unit, &self.root_dir)?;
        link.collect_imports(unit, &self.interfaces, &self.value_types)?;
        let value_refs = link.collect_value_refs(unit)?;
        let exports = link.collect_exports(unit, &value_refs)?;
        let ModuleLink {
            type_refs,
            link_sources,
            imports,
            imported_value_types,
            imported_type_names,
            ..
        } = link;
        let plugin_types: HashMap<Symbol, PluginType> = type_refs
            .values()
            .filter_map(|target| {
                target.plugin.as_ref().map(|source| {
                    (
                        target.full,
                        PluginType {
                            source: source.clone(),
                            name: target.name,
                        },
                    )
                })
            })
            .collect();

        let mut opaque_types: HashSet<Symbol> =
            exports.link_types.values().map(|target| target.full).collect();
        for target in type_refs.values() {
            if self
                .interfaces
                .get(&target.module)
                .is_some_and(|interface| interface.link_types.contains_key(&target.name))
            {
                opaque_types.insert(target.full);
            }
        }

        let type_map: TypeNames = type_refs
            .iter()
            .map(|(local, target)| {
                (
                    *local,
                    rewrite::TypeName {
                        full: target.full,
                        span: target.span,
                    },
                )
            })
            .collect();
        let mut program = unit.program.clone();
        normalize_main(&mut program, unit.id.as_str())?;
        rewrite_program(&mut program, &type_map);
        let main = take_main(&mut program);

        let (session, value_types, link_value_types, main) = self.check_module(
            unit,
            &program,
            main,
            &exports,
            &opaque_types,
            &imported_value_types,
        )?;

        let types = build_module_types(
            unit,
            &session,
            &type_refs,
            &exports,
            &value_types,
            &link_value_types,
            &imported_value_types,
            &opaque_types,
        );

        self.interfaces.insert(unit.id.clone(), exports.clone());
        self.value_types.insert(unit.id.clone(), value_types.clone());
        let links = program
            .exprs()
            .filter_map(|stmt| match &stmt.node {
                ExprKind::Special(Special::Link(link)) => Some(link.clone()),
                _ => None,
            })
            .collect();
        Ok(LinkedModule {
            id: unit.id.clone(),
            path: unit.path.clone(),
            program,
            main,
            links,
            link_sources,
            plugin_types,
            imports,
            imported_value_types,
            imported_type_names,
            exports,
            value_types,
            types,
            opaque_types,
        })
    }

    /// 重写后的模块程序与导入接口一起检查, 返回类型会话与值类型.
    fn check_module(
        &mut self,
        unit: &ModuleUnit,
        program: &ksl_frontend::ast::Program,
        main: Option<Expr>,
        exports: &Exports,
        opaque_types: &HashSet<Symbol>,
        imported_value_types: &ModuleValueTypes,
    ) -> Result<ModuleCheck, KslError> {
        check::collect_data(program, &mut self.data, opaque_types)?;
        let mut session = Session::for_module(self.data.clone(), imported_value_types.clone());
        for (local, target) in &exports.link_types {
            session.add_link_type(*local, target.full);
        }
        for full in opaque_types {
            session.add_opaque_type(*full);
        }
        let mut link_value_types = HashMap::new();
        for stmt in program.exprs() {
            let ExprKind::Special(Special::Link(link)) = &stmt.node else {
                continue;
            };
            for value in &link.values {
                let ty = session.declare_link_value(value.name.node, &value.ty)?;
                check::validate_plugin_type(&ty).map_err(|detail| {
                    KslError::with_subject(
                        ErrorKind::PluginUnsupportedSignature { detail },
                        value.name.span,
                        resolve(value.name.node).to_string(),
                    )
                })?;
                link_value_types.insert(value.name.node, ty);
            }
        }
        let main_handler = session.check_loaded_with_main(program, main.as_ref())?;
        let main = match (main, main_handler) {
            (Some(expr), Some(handler)) => Some(MainEntry { expr, handler }),
            (None, None) => None,
            _ => {
                return Err(KslError::without_span(ErrorKind::ModuleFormNotResolved));
            },
        };

        let mut value_types = HashMap::new();
        for (name, target) in &exports.values {
            let ty = if target.module == unit.id {
                session.value_type(target.name)
            } else {
                imported_value_types.get(name).cloned()
            };
            let ty = ty.ok_or_else(|| {
                KslError::without_span(ErrorKind::ExportUnknown {
                    name: resolve(*name).to_string(),
                })
            })?;
            value_types.insert(*name, ty);
        }
        value_types.extend(link_value_types.iter().map(|(name, ty)| (*name, ty.clone())));

        let visible = visible_types(exports);
        for item in &unit.exports {
            match item {
                ExportItem::Value(name) => {
                    let ty = value_types.get(&name.node).ok_or_else(|| {
                        KslError::without_span(ErrorKind::ExportUnknown {
                            name: resolve(name.node).to_string(),
                        })
                    })?;
                    check_export_type(ty, &visible, name.span)?;
                },
                ExportItem::Type(name) => {
                    let target = exports.types.get(&name.node).ok_or_else(|| {
                        KslError::with_subject(
                            ErrorKind::ExportUnknown {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        )
                    })?;
                    if let Some(info) = self.data.get(&target.full) {
                        for fields in info.variants.values() {
                            for field in fields {
                                check_export_type(field, &visible, name.span)?;
                            }
                        }
                    }
                },
            }
        }
        for link in &unit.links {
            for value in &link.values {
                if let Some(ty) = link_value_types.get(&value.name.node) {
                    check_export_type(ty, &visible, value.name.span)?;
                }
            }
        }
        Ok((session, value_types, link_value_types, main))
    }
}
