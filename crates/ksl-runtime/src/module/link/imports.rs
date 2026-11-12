//! 单模块的导入与导出收集.
//!
//! 负责 `@Link` 声明, `@Use` 别名与 `@Export` 的解析;
//! 链接与检查阶段由父模块继续处理.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, intern, resolve};
use ksl_frontend::ast::{ExportItem, Spanned, UseItem};
use ksl_frontend::error::{ErrorKind, KslError};
use ksl_frontend::rewrite::{self, TypeNames};

use super::exports::plugin_type_name;
use super::{
    CachedValueTypes, Exports, ModuleValueTypes, TypeRef, ValueRef, check_local_name,
    insert_type_import, insert_value_import, link_name_conflict, local_type_names,
    local_value_names, plugin,
};
use crate::ffi::PluginSource;
use crate::module::{ModuleId, ModuleUnit};

/// 单模块链接中导入与导出阶段的状态.
pub(super) struct ModuleLink {
    /// `@Link` 类型到插件来源.
    link_type_plugins: HashMap<Symbol, PluginSource>,
    /// `@Link` 插件名到链接阶段解析出的来源.
    pub(super) link_sources: HashMap<String, PluginSource>,
    /// `@Link` 值到声明位置.
    link_value_spans: HashMap<Symbol, Span>,
    pub(super) type_refs: HashMap<Symbol, TypeRef>,
    pub(super) imports: HashMap<Symbol, ValueRef>,
    pub(super) imported_value_types: ModuleValueTypes,
    pub(super) imported_type_names: TypeNames,
}

impl ModuleLink {
    /// 收集 `@Link` 声明与本地类型名, 建立可见名到真实类型的映射.
    pub(super) fn new(unit: &ModuleUnit, root: &Path) -> Result<Self, KslError> {
        let mut type_names = local_type_names(&unit.program)?;
        let mut link_value_spans = HashMap::new();
        for link in &unit.links {
            for ty in &link.types {
                check_local_name(ty.node, ty.span)?;
                if type_names.contains_key(&ty.node) {
                    return Err(super::link_name_conflict(ty.node, ty.span));
                }
                type_names.insert(ty.node, ty.span);
            }
            for value in &link.values {
                check_local_name(value.name.node, value.name.span)?;
                if link_value_spans
                    .insert(value.name.node, value.name.span)
                    .is_some()
                {
                    return Err(super::link_name_conflict(value.name.node, value.name.span));
                }
            }
        }
        // 解析放在声明校验之后: 声明错误优先于插件文件缺失.
        let mut link_type_plugins = HashMap::new();
        let mut link_sources = HashMap::new();
        for link in &unit.links {
            let span = link
                .types
                .first()
                .map(|ty| ty.span)
                .or_else(|| link.values.first().map(|value| value.name.span))
                .unwrap_or_default();
            let source = plugin::resolve(&link.plugin, &unit.path, root, span)?;
            link_sources.insert(link.plugin.clone(), source.clone());
            for ty in &link.types {
                link_type_plugins.insert(ty.node, source.clone());
            }
        }
        let mut type_refs = HashMap::new();
        for (name, span) in &type_names {
            check_local_name(*name, *span)?;
            let plugin = link_type_plugins.get(name).cloned();
            let full = match &plugin {
                Some(source) => intern(&plugin_type_name(&source.id, &resolve(*name))),
                None => intern(&unit.id.full_name(&resolve(*name))),
            };
            type_refs.insert(
                *name,
                TypeRef {
                    full,
                    module: unit.id.clone(),
                    name: *name,
                    span: *span,
                    plugin,
                },
            );
        }
        Ok(Self {
            link_type_plugins,
            link_sources,
            link_value_spans,
            type_refs,
            imports: HashMap::new(),
            imported_value_types: HashMap::new(),
            imported_type_names: TypeNames::new(),
        })
    }

    /// 把 `@Use` 解析为导入值, 导入类型与本地别名.
    pub(super) fn collect_imports(
        &mut self,
        unit: &ModuleUnit,
        interfaces: &imbl::HashMap<ModuleId, Exports>,
        value_types: &CachedValueTypes,
    ) -> Result<(), KslError> {
        let link_type_names: HashSet<Symbol> = self.link_type_plugins.keys().copied().collect();
        for import in &unit.uses {
            let interface = interfaces.get(&import.target).ok_or_else(|| {
                KslError::with_subject(
                    ErrorKind::ImportNotExported {
                        module: import.target.to_string(),
                        name: import.target.to_string(),
                    },
                    import.span,
                    import.target.to_string(),
                )
            })?;
            for item in &import.items {
                match item {
                    UseItem::Value(name) => {
                        self.import_value(interface, &import.target, name, value_types)?;
                    },
                    UseItem::Rename { from, to } => {
                        self.import_value_rename(
                            interface,
                            &import.target,
                            from,
                            to,
                            value_types,
                        )?;
                    },
                    UseItem::Type(name) => {
                        self.import_type(interface, &import.target, name, &link_type_names)?;
                    },
                    UseItem::TypeRename { from, to } => {
                        self.import_type_rename(
                            interface,
                            &import.target,
                            from,
                            to,
                            &link_type_names,
                        )?;
                    },
                }
            }
        }
        Ok(())
    }

    fn import_value(
        &mut self,
        interface: &Exports,
        target: &ModuleId,
        name: &Spanned<Symbol>,
        value_types: &CachedValueTypes,
    ) -> Result<(), KslError> {
        let local = name.node;
        check_local_name(local, name.span)?;
        if self.link_value_spans.contains_key(&local) {
            return Err(link_name_conflict(local, name.span));
        }
        let value = interface.value(local).cloned().ok_or_else(|| {
            KslError::with_subject(
                ErrorKind::ImportNotExported {
                    module: target.to_string(),
                    name: resolve(local).to_string(),
                },
                name.span,
                resolve(local).to_string(),
            )
        })?;
        insert_value_import(&mut self.imports, local, value, name.span)?;
        if let Some(ty) = value_types.get(target).and_then(|types| types.get(&local)) {
            self.imported_value_types.insert(local, ty.clone());
        }
        Ok(())
    }

    fn import_value_rename(
        &mut self,
        interface: &Exports,
        target: &ModuleId,
        from: &Spanned<Symbol>,
        to: &Spanned<Symbol>,
        value_types: &CachedValueTypes,
    ) -> Result<(), KslError> {
        check_local_name(to.node, to.span)?;
        if self.link_value_spans.contains_key(&to.node) {
            return Err(link_name_conflict(to.node, to.span));
        }
        let value = interface.value(from.node).cloned().ok_or_else(|| {
            KslError::with_subject(
                ErrorKind::ImportNotExported {
                    module: target.to_string(),
                    name: resolve(from.node).to_string(),
                },
                from.span,
                resolve(from.node).to_string(),
            )
        })?;
        insert_value_import(&mut self.imports, to.node, value, to.span)?;
        if let Some(ty) = value_types.get(target).and_then(|types| types.get(&from.node)) {
            self.imported_value_types.insert(to.node, ty.clone());
        }
        Ok(())
    }

    fn import_type(
        &mut self,
        interface: &Exports,
        target: &ModuleId,
        name: &Spanned<Symbol>,
        link_type_names: &HashSet<Symbol>,
    ) -> Result<(), KslError> {
        let local = name.node;
        check_local_name(local, name.span)?;
        if link_type_names.contains(&local) {
            return Err(link_name_conflict(local, name.span));
        }
        let value = interface.type_ref(local).cloned().ok_or_else(|| {
            KslError::with_subject(
                ErrorKind::ImportNotExported {
                    module: target.to_string(),
                    name: resolve(local).to_string(),
                },
                name.span,
                resolve(local).to_string(),
            )
        })?;
        insert_type_import(&mut self.type_refs, local, value.clone(), name.span)?;
        self.imported_type_names.insert(
            local,
            rewrite::TypeName {
                full: value.full,
                span: name.span,
            },
        );
        Ok(())
    }

    fn import_type_rename(
        &mut self,
        interface: &Exports,
        target: &ModuleId,
        from: &Spanned<Symbol>,
        to: &Spanned<Symbol>,
        link_type_names: &HashSet<Symbol>,
    ) -> Result<(), KslError> {
        check_local_name(to.node, to.span)?;
        if link_type_names.contains(&to.node) {
            return Err(link_name_conflict(to.node, to.span));
        }
        let value = interface.type_ref(from.node).cloned().ok_or_else(|| {
            KslError::with_subject(
                ErrorKind::ImportNotExported {
                    module: target.to_string(),
                    name: resolve(from.node).to_string(),
                },
                from.span,
                resolve(from.node).to_string(),
            )
        })?;
        insert_type_import(&mut self.type_refs, to.node, value.clone(), to.span)?;
        self.imported_type_names.insert(
            to.node,
            rewrite::TypeName {
                full: value.full,
                span: to.span,
            },
        );
        Ok(())
    }

    /// 本地值定义遮蔽同名导入值.
    pub(super) fn collect_value_refs(
        &self,
        unit: &ModuleUnit,
    ) -> Result<HashMap<Symbol, ValueRef>, KslError> {
        let mut value_refs = self.imports.clone();
        for (name, span) in local_value_names(&unit.program) {
            if self.link_value_spans.contains_key(&name) {
                return Err(link_name_conflict(name, span));
            }
            value_refs.insert(
                name,
                ValueRef {
                    module: unit.id.clone(),
                    name,
                },
            );
        }
        Ok(value_refs)
    }

    /// 汇总 `@Link` 导出与普通导出, 并校验重复与未知名.
    pub(super) fn collect_exports(
        &self,
        unit: &ModuleUnit,
        value_refs: &HashMap<Symbol, ValueRef>,
    ) -> Result<Exports, KslError> {
        let mut exports = Exports::default();
        for link in &unit.links {
            for ty in &link.types {
                let target = self.type_refs.get(&ty.node).cloned().expect("Link 类型已登记");
                exports.link_types.insert(ty.node, target);
            }
            for value in &link.values {
                exports.link_values.insert(
                    value.name.node,
                    ValueRef {
                        module: unit.id.clone(),
                        name: value.name.node,
                    },
                );
            }
        }
        for item in &unit.exports {
            match item {
                ExportItem::Value(name) => {
                    if exports.link_values.contains_key(&name.node) {
                        return Err(KslError::with_subject(
                            ErrorKind::LinkExportConflict {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    }
                    let Some(target) = value_refs.get(&name.node).cloned() else {
                        return Err(KslError::with_subject(
                            ErrorKind::ExportUnknown {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    };
                    if exports.values.insert(name.node, target).is_some() {
                        return Err(KslError::with_subject(
                            ErrorKind::DuplicateExport {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    }
                },
                ExportItem::Type(name) => {
                    if exports.link_types.contains_key(&name.node) {
                        return Err(KslError::with_subject(
                            ErrorKind::LinkExportConflict {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    }
                    let Some(target) = self.type_refs.get(&name.node).cloned() else {
                        return Err(KslError::with_subject(
                            ErrorKind::ExportUnknown {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    };
                    if exports.types.insert(name.node, target).is_some() {
                        return Err(KslError::with_subject(
                            ErrorKind::DuplicateExport {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    }
                },
            }
        }
        Ok(exports)
    }
}
