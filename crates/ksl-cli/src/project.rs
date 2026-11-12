//! 项目标记与分析根.
//!
//! `.kslproj` 为空文件, 只表示目录是项目根; 内容不读取.
//! 标记只在启动目录检查, 不向上查找.

use std::path::{Path, PathBuf};

use ksl_compile::root_dir_of;

/// 项目标记文件名.
pub const PROJECT_MARKER: &str = ".kslproj";

/// 目录是否含项目标记.
#[must_use]
pub fn has_project_marker(dir: &Path) -> bool {
    dir.join(PROJECT_MARKER).is_file()
}

/// 按启动目录确定分析根.
///
/// 启动目录含标记且文件位于其下时取启动目录, 否则取文件所在目录.
#[must_use]
pub fn project_root(startup: &Path, file: &Path) -> PathBuf {
    let startup = std::fs::canonicalize(startup).unwrap_or_else(|_| startup.to_path_buf());
    let dir = root_dir_of(file);
    if has_project_marker(&startup) && dir.starts_with(&startup) {
        startup
    } else {
        dir
    }
}
