//! 导出可见性与模块类型表.

use std::collections::{HashMap, HashSet};

use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, intern, resolve};
use ksl_frontend::ast::{ExportItem, UseItem};
use ksl_frontend::check::{self, Session};
use ksl_frontend::error::{ErrorKind, KslError};

use super::{Exports, ModuleValueTypes, TypeRef};
use crate::module::ModuleUnit;

/// 汇总本模块源码区间到类型的映射.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_module_types(
    unit: &ModuleUnit,
    session: &Session,
    type_refs: &HashMap<Symbol, TypeRef>,
    exports: &Exports,
    value_types: &ModuleValueTypes,
    link_value_types: &ModuleValueTypes,
    imported_value_types: &ModuleValueTypes,
    opaque_types: &HashSet<Symbol>,
) -> ksl_frontend::check::TypeTable {
    let mut types = session.types().clone();
    for link in &unit.links {
        for ty in &link.types {
            if let Some(target) = exports.link_types.get(&ty.node) {
                types.insert(ty.key(), ksl_frontend::check::Type::opaque(target.full));
            }
        }
    }
    for (name, ty) in link_value_types {
        types.insert(
            unit.links
                .iter()
                .flat_map(|link| link.values.iter())
                .find(|value| value.name.node == *name)
                .expect("Link 值已登记")
                .name
                .key(),
            ty.clone(),
        );
    }
    for import in &unit.uses {
        for item in &import.items {
            match item {
                UseItem::Value(name) => {
                    if let Some(ty) = imported_value_types.get(&name.node) {
                        types.insert(name.key(), ty.clone());
                    }
                },
                UseItem::Rename { to, .. } => {
                    if let Some(ty) = imported_value_types.get(&to.node) {
                        types.insert(to.key(), ty.clone());
                    }
                },
                UseItem::Type(name) => {
                    if let Some(target) = type_refs.get(&name.node) {
                        types.insert(name.key(), type_value(target.full, opaque_types));
                    }
                },
                UseItem::TypeRename { to, .. } => {
                    if let Some(target) = type_refs.get(&to.node) {
                        types.insert(to.key(), type_value(target.full, opaque_types));
                    }
                },
            }
        }
    }
    for item in &unit.exports {
        match item {
            ExportItem::Value(name) => {
                if let Some(ty) = value_types.get(&name.node) {
                    types.insert(name.key(), ty.clone());
                }
            },
            ExportItem::Type(name) => {
                if let Some(target) = exports.types.get(&name.node) {
                    types.insert(name.key(), type_value(target.full, opaque_types));
                }
            },
        }
    }
    types
}

/// `@Link` 类型的完整名; 身份按插件来源与类型名共享, 不随引用模块变化.
pub(super) fn plugin_type_name(id: &str, name: &str) -> String {
    format!("$plugin/{id}/{name}")
}

fn type_value(full: Symbol, opaque_types: &HashSet<Symbol>) -> ksl_frontend::check::Type {
    if opaque_types.contains(&full) {
        ksl_frontend::check::Type::opaque(full)
    } else {
        ksl_frontend::check::Type::data(full)
    }
}

pub(super) fn visible_types(exports: &Exports) -> HashSet<Symbol> {
    let mut visible: HashSet<Symbol> = exports
        .types
        .values()
        .chain(exports.link_types.values())
        .map(|target| target.full)
        .collect();
    // 内置 Data 与不透明类型始终可见, 否则导出时间与 I/O 相关签名会被误判为私有类型.
    visible.extend(check::builtin_data_type_names().iter().map(|name| intern(name)));
    visible.extend(
        check::builtin_opaque_type_names()
            .iter()
            .map(|name| intern(name)),
    );
    visible
}

/// 导出签名中的 Data 类型必须已导出或为内置类型. 不透明类型均为内置.
pub(super) fn check_export_type(
    ty: &ksl_frontend::check::Type,
    visible: &HashSet<Symbol>,
    span: Span,
) -> Result<(), KslError> {
    use ksl_frontend::check::Type;
    match ty {
        Type::App { name, args } if args.is_empty() => {
            if visible.contains(name) {
                Ok(())
            } else {
                Err(KslError::with_subject(
                    ErrorKind::ExportPrivateType {
                        name: resolve(*name).to_string(),
                    },
                    span,
                    resolve(*name).to_string(),
                ))
            }
        },
        Type::App { args, .. } => {
            for arg in args.iter() {
                check_export_type(arg, visible, span)?;
            }
            Ok(())
        },
        Type::Fn(params, ret) => {
            for param in params {
                check_export_type(param, visible, span)?;
            }
            check_export_type(ret, visible, span)
        },
        Type::Opaque(name) => {
            if visible.contains(name) {
                Ok(())
            } else {
                Err(KslError::with_subject(
                    ErrorKind::ExportPrivateType {
                        name: resolve(*name).to_string(),
                    },
                    span,
                    resolve(*name).to_string(),
                ))
            }
        },
        Type::I64
        | Type::U8
        | Type::U64
        | Type::F64
        | Type::Char
        | Type::Bool
        | Type::Str
        | Type::Nil
        | Type::Never => Ok(()),
    }
}
