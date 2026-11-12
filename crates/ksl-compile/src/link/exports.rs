//! 导出可见性与模块类型表.

use std::collections::HashSet;

use ksl_base::display::TypeNames;
use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::KslError;
use ksl_frontend::check;

use super::ModuleInterface;

/// `@Link` 类型的完整名; 身份按插件来源与类型名共享, 不随引用模块变化.
pub(super) fn plugin_type_name(id: &str, name: &str) -> String {
    format!("$plugin/{id}/{name}")
}

pub(super) fn visible_types(exports: &ModuleInterface) -> HashSet<Name> {
    let mut visible: HashSet<Name> = exports
        .types
        .values()
        .chain(exports.link_types.values())
        .map(|target| target.full.clone())
        .collect();
    // 内置 Data 与不透明类型始终可见.
    visible.extend(check::builtin_data_type_names().iter().map(Name::new));
    visible.extend(check::builtin_opaque_type_names().iter().map(Name::new));
    visible
}

/// 导出签名中的 Data 类型必须已导出或为内置类型. 不透明类型均为内置.
pub(super) fn check_export_type(
    ty: &Type,
    visible: &HashSet<Name>,
    names: &TypeNames,
    span: Span,
) -> Result<(), KslError> {
    match ty {
        Type::Data(name) | Type::Opaque(name) => {
            if visible.contains(name) {
                Ok(())
            } else {
                let display = names.resolve(name);
                Err(KslError::export_private_type(&display, span))
            }
        },
        Type::Fn(params, ret) => {
            for param in params.iter() {
                check_export_type(param, visible, names, span)?;
            }
            check_export_type(ret, visible, names, span)
        },
        other => {
            if let Some(args) = other.type_args() {
                for arg in args {
                    check_export_type(arg, visible, names, span)?;
                }
            }
            Ok(())
        },
    }
}
