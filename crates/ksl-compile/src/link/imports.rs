//! 单模块的导入与导出收集.
//!
//! 收集 `@Link` 声明, `@Use` 别名与 `@Export`;
//! 链接与检查由父模块继续处理.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use ksl_base::display::TypeNames;
use ksl_base::name::Name;
use ksl_base::plugin::PluginSource;
use ksl_base::source::Span;
use ksl_diagnostics::error::KslError;
use ksl_frontend::ast::{ExportItem, Spanned, UseItem};
use ksl_frontend::check::{DataTable, TypeBinding, TypeBindings, TypeTarget};

use super::exports::plugin_type_name;
use super::{
    ModuleInterface, TypeRef, ValueRef, ValueTypes, check_local_name, insert_type_import,
    insert_value_import, local_type_names, local_value_names, plugin,
};
use crate::graph::{ModuleId, ModuleUnit};

/// 单模块链接中导入与导出阶段的状态.
pub(super) struct ModuleLink {
    /// `@Link` 类型到插件来源.
    link_type_plugins: HashMap<Name, PluginSource>,
    /// `@Link` 插件名到链接阶段解析出的来源.
    pub(super) link_sources: HashMap<String, PluginSource>,
    /// `@Link` 值到声明位置.
    link_value_spans: HashMap<Name, Span>,
    pub(super) type_refs: HashMap<Name, TypeRef>,
    pub(super) imports: HashMap<Name, ValueRef>,
    pub(super) imported_value_types: ValueTypes,
    /// 模块内可见的全部类型名绑定; 含 Link 类型与导入类型.
    pub(super) bindings: TypeBindings,
    /// 仅导入的类型名绑定; 供行内 `@Use` 并入会话.
    pub(super) imported_bindings: TypeBindings,
    /// 导入 Data 类型的声明; 按真实身份索引.
    pub(super) imported_data: DataTable,
}

impl ModuleLink {
    /// 收集 `@Link` 声明与本地类型名, 建立可见名到真实类型的映射.
    pub(super) fn new(unit: &ModuleUnit, root: &Path) -> Result<Self, KslError> {
        let mut type_names = local_type_names(&unit.program)?;
        let mut link_value_spans = HashMap::new();
        for link in &unit.links {
            for ty in &link.types {
                check_local_name(&ty.node, ty.span)?;
                if type_names.contains_key(&ty.node) {
                    return Err(KslError::link_name_conflict(&ty.node, ty.span));
                }
                type_names.insert(ty.node.clone(), ty.span);
            }
            for value in &link.values {
                check_local_name(&value.name.node, value.name.span)?;
                if link_value_spans
                    .insert(value.name.node.clone(), value.name.span)
                    .is_some()
                {
                    return Err(KslError::link_name_conflict(
                        &value.name.node,
                        value.name.span,
                    ));
                }
            }
        }
        // 先校验声明, 再解析插件文件.
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
                link_type_plugins.insert(ty.node.clone(), source.clone());
            }
        }
        let mut type_refs = HashMap::new();
        for (name, span) in &type_names {
            check_local_name(name, *span)?;
            let plugin = link_type_plugins.get(name).cloned();
            let full = match &plugin {
                Some(source) => Name::new(plugin_type_name(&source.id, name.as_str())),
                None => Name::new(unit.id.full_name(name.as_str())),
            };
            type_refs.insert(
                name.clone(),
                TypeRef {
                    full,
                    module: unit.id.clone(),
                    name: name.clone(),
                    span: *span,
                    plugin,
                },
            );
        }
        // Link 类型在检查期以不透明类型参与解析; 本地 Data 由声明收集器登记.
        let mut bindings = TypeBindings::new();
        for (name, target) in &type_refs {
            if target.plugin.is_some() {
                bindings.insert(
                    name.clone(),
                    TypeBinding::new(TypeTarget::Opaque(target.full.clone()), target.span),
                );
            }
        }
        Ok(Self {
            link_type_plugins,
            link_sources,
            link_value_spans,
            type_refs,
            imports: HashMap::new(),
            imported_value_types: HashMap::new(),
            bindings,
            imported_bindings: TypeBindings::new(),
            imported_data: DataTable::new(),
        })
    }

    /// 把 `@Use` 解析为导入值, 导入类型与本地别名.
    pub(super) fn collect_imports(
        &mut self,
        unit: &ModuleUnit,
        interfaces: &imbl::HashMap<ModuleId, ModuleInterface>,
    ) -> Result<(), KslError> {
        let link_type_names: HashSet<Name> = self.link_type_plugins.keys().cloned().collect();
        for import in &unit.uses {
            let interface = interfaces.get(&import.target).ok_or_else(|| {
                KslError::internal(
                    format!("导入模块尚未链接: {}", import.target),
                    Some(import.span),
                )
            })?;
            for item in &import.items {
                match item {
                    UseItem::Value(name) => {
                        self.import_value(interface, &import.target, name)?;
                    },
                    UseItem::Rename { from, to } => {
                        self.import_value_rename(interface, &import.target, from, to)?;
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

    /// 复制导入 Data 类型的声明; Link 类型是不透明类型, 没有声明.
    fn import_decl(
        &mut self,
        interface: &ModuleInterface,
        target: &TypeRef,
        span: Span,
    ) -> Result<(), KslError> {
        if target.plugin.is_some() {
            return Ok(());
        }
        let info = interface.type_decls.get(&target.full).ok_or_else(|| {
            KslError::internal(format!("类型缺少声明: {}", target.full), Some(span))
        })?;
        self.imported_data.insert(target.full.clone(), info.clone());
        Ok(())
    }

    fn import_value(
        &mut self,
        interface: &ModuleInterface,
        target: &ModuleId,
        name: &Spanned<Name>,
    ) -> Result<(), KslError> {
        let local = name.node.clone();
        check_local_name(&local, name.span)?;
        if self.link_value_spans.contains_key(&local) {
            return Err(KslError::link_name_conflict(&local, name.span));
        }
        let value = interface
            .value(&local)
            .cloned()
            .ok_or_else(|| KslError::import_not_exported(target.as_str(), &local, name.span))?;
        insert_value_import(&mut self.imports, local.clone(), value, name.span)?;
        if let Some(ty) = interface.value_types.get(&local) {
            self.imported_value_types.insert(local.clone(), ty.clone());
        }
        Ok(())
    }

    fn import_value_rename(
        &mut self,
        interface: &ModuleInterface,
        target: &ModuleId,
        from: &Spanned<Name>,
        to: &Spanned<Name>,
    ) -> Result<(), KslError> {
        check_local_name(&to.node, to.span)?;
        if self.link_value_spans.contains_key(&to.node) {
            return Err(KslError::link_name_conflict(&to.node, to.span));
        }
        let value = interface.value(&from.node).cloned().ok_or_else(|| {
            KslError::import_not_exported(target.as_str(), &from.node, from.span)
        })?;
        insert_value_import(&mut self.imports, to.node.clone(), value, to.span)?;
        if let Some(ty) = interface.value_types.get(&from.node) {
            self.imported_value_types.insert(to.node.clone(), ty.clone());
        }
        Ok(())
    }

    fn import_type(
        &mut self,
        interface: &ModuleInterface,
        target: &ModuleId,
        name: &Spanned<Name>,
        link_type_names: &HashSet<Name>,
    ) -> Result<(), KslError> {
        let local = name.node.clone();
        check_local_name(&local, name.span)?;
        if link_type_names.contains(&local) {
            return Err(KslError::link_name_conflict(&local, name.span));
        }
        let value = interface
            .type_ref(&local)
            .cloned()
            .ok_or_else(|| KslError::import_not_exported(target.as_str(), &local, name.span))?;
        insert_type_import(&mut self.type_refs, local.clone(), value.clone(), name.span)?;
        self.import_decl(interface, &value, name.span)?;
        let binding = TypeBinding::new(value.target(), name.span);
        self.imported_bindings.insert(local.clone(), binding.clone());
        self.bindings.insert(local, binding);
        Ok(())
    }

    fn import_type_rename(
        &mut self,
        interface: &ModuleInterface,
        target: &ModuleId,
        from: &Spanned<Name>,
        to: &Spanned<Name>,
        link_type_names: &HashSet<Name>,
    ) -> Result<(), KslError> {
        check_local_name(&to.node, to.span)?;
        if link_type_names.contains(&to.node) {
            return Err(KslError::link_name_conflict(&to.node, to.span));
        }
        let value = interface.type_ref(&from.node).cloned().ok_or_else(|| {
            KslError::import_not_exported(target.as_str(), &from.node, from.span)
        })?;
        insert_type_import(&mut self.type_refs, to.node.clone(), value.clone(), to.span)?;
        self.import_decl(interface, &value, to.span)?;
        let binding = TypeBinding::new(value.target(), to.span);
        self.imported_bindings.insert(to.node.clone(), binding.clone());
        self.bindings.insert(to.node.clone(), binding);
        Ok(())
    }

    /// 本地值定义遮蔽同名导入值.
    pub(super) fn collect_value_refs(
        &self,
        unit: &ModuleUnit,
    ) -> Result<HashMap<Name, ValueRef>, KslError> {
        let mut value_refs = self.imports.clone();
        for (name, span) in local_value_names(&unit.program) {
            if self.link_value_spans.contains_key(&name) {
                return Err(KslError::link_name_conflict(&name, span));
            }
            value_refs.insert(
                name.clone(),
                ValueRef {
                    module: unit.id.clone(),
                    name,
                },
            );
        }
        Ok(value_refs)
    }

    /// 模块内真实名到可见名的显示表; 供导出签名检查使用.
    #[must_use]
    pub(super) fn type_names(&self) -> TypeNames {
        let mut aliases: Vec<(Name, Name, Span)> = self
            .type_refs
            .iter()
            .map(|(local, target)| (target.full.clone(), local.clone(), target.span))
            .collect();
        // 同一真实类型的多个可见名按声明或导入位置取第一个.
        aliases.sort_by_key(|(_, _, span)| (span.start, span.end));
        TypeNames::new(aliases.into_iter().map(|(full, local, _)| (full, local)))
    }

    /// 汇总 `@Link` 导出与普通导出, 并校验重复与未知名.
    pub(super) fn collect_exports(
        &self,
        unit: &ModuleUnit,
        value_refs: &HashMap<Name, ValueRef>,
    ) -> Result<ModuleInterface, KslError> {
        let mut exports = ModuleInterface::default();
        for link in &unit.links {
            for ty in &link.types {
                let target = self.type_refs.get(&ty.node).cloned().expect("Link 类型已登记");
                exports.link_types.insert(ty.node.clone(), target);
            }
            for value in &link.values {
                exports.link_values.insert(
                    value.name.node.clone(),
                    ValueRef {
                        module: unit.id.clone(),
                        name: value.name.node.clone(),
                    },
                );
            }
        }
        for item in &unit.exports {
            match item {
                ExportItem::Value(name) => {
                    if exports.link_values.contains_key(&name.node) {
                        return Err(KslError::link_export_conflict(&name.node, name.span));
                    }
                    let Some(target) = value_refs.get(&name.node).cloned() else {
                        return Err(KslError::export_unknown(&name.node, name.span));
                    };
                    if exports.values.insert(name.node.clone(), target).is_some() {
                        return Err(KslError::duplicate_export(&name.node, name.span));
                    }
                },
                ExportItem::Type(name) => {
                    if exports.link_types.contains_key(&name.node) {
                        return Err(KslError::link_export_conflict(&name.node, name.span));
                    }
                    let Some(target) = self.type_refs.get(&name.node).cloned() else {
                        return Err(KslError::export_unknown(&name.node, name.span));
                    };
                    if exports.types.insert(name.node.clone(), target).is_some() {
                        return Err(KslError::duplicate_export(&name.node, name.span));
                    }
                },
            }
        }
        Ok(exports)
    }
}
