//! 插件路径约定.
//!
//! `@Link` 与宿主都使用同一套路径解析结果; 本模块不加载动态库.

use std::path::PathBuf;

/// `@Link` 解析后的插件来源.
///
/// `id` 是根相对, 去扩展名的路径, 构成不透明类型的身份;
/// `path` 是规范化绝对路径, 作为加载与缓存键.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSource {
    pub id: String,
    pub path: PathBuf,
}

/// 平台约定的插件文件名; 名字不合法时返回 None.
///
/// 名字是 `plugin/` 下的文件名, 不含路径分隔符与扩展名.
#[must_use]
pub fn plugin_file(name: &str) -> Option<String> {
    let invalid = name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().any(|ch| matches!(ch, '/' | '\\' | '\0'));
    if invalid {
        return None;
    }
    let file = if cfg!(target_os = "windows") {
        format!("{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{name}.dylib")
    } else {
        format!("lib{name}.so")
    };
    Some(file)
}
