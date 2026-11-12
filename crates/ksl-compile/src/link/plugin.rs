//! 插件来源解析.
//!
//! 插件与模块使用同一套路径规则: 位于根目录内, 以规范化路径定位.
//! 解析结果同时给出类型身份与加载路径, 链接与执行两阶段共用.

use std::path::Path;

use ksl_base::plugin::{PluginSource, plugin_file};
use ksl_base::source::Span;
use ksl_diagnostics::error::KslError;

use crate::graph::relative_id;

/// 解析 `@Link` 的插件文件; 文件必须存在且位于根目录内.
pub(super) fn resolve(
    name: &str,
    module_file: &Path,
    root: &Path,
    span: Span,
) -> Result<PluginSource, KslError> {
    let Some(file) = plugin_file(name) else {
        return Err(KslError::plugin_name_invalid(name, span));
    };
    let dir = module_file.parent().unwrap_or_else(|| Path::new("."));
    let path = dir.join("plugin").join(file);
    let canonical = std::fs::canonicalize(&path).map_err(|error| {
        KslError::plugin_load_failed(format!("{}: {error}", path.display()), name, span)
    })?;
    if !canonical.starts_with(root) {
        return Err(KslError::plugin_load_failed(
            format!("{}: 位于模块根目录之外", path.display()),
            name,
            span,
        ));
    }
    let id = relative_id(root, &canonical);
    Ok(PluginSource {
        id,
        path: canonical,
    })
}
