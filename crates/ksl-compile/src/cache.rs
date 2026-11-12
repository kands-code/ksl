//! 编译缓存: 模块接口与依赖文件状态.
//!
//! 只由加载与链接阶段写入; 会话内已检查的模块直接复用.
//! 缓存记录依赖文件状态; 编辑器分析据此检测变化, 变化时整体失效.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ksl_base::name::Name;

use crate::graph::ModuleId;
use crate::link;

/// 已缓存模块文件的最后状态.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(FileStamp {
        modified: metadata.modified().ok(),
        len: metadata.len(),
    })
}

/// 编译缓存: 模块接口与文件状态.
#[derive(Debug, Clone, Default)]
pub struct CompileCache {
    pub(crate) interfaces: imbl::HashMap<ModuleId, link::ModuleInterface>,
    /// 依赖文件路径与缓存时的状态; 启用检测时任一变化即整体失效.
    files: HashMap<ModuleId, (PathBuf, FileStamp)>,
    /// 是否检测依赖文件变化; 编辑器分析启用, 交互会话保持快照.
    watch_files: bool,
}

impl CompileCache {
    /// 新建缓存; 不检测依赖文件变化.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 新建缓存; 检测依赖文件变化, 供编辑器分析使用.
    #[must_use]
    pub fn watching() -> Self {
        Self {
            watch_files: true,
            ..Self::new()
        }
    }

    /// 丢弃全部缓存; 文档被编辑或根目录变化时使用.
    pub fn invalidate(&mut self) {
        let watch_files = self.watch_files;
        *self = Self::new();
        self.watch_files = watch_files;
    }

    /// 已加载模块标识.
    #[must_use]
    pub(crate) fn loaded_ids(&self) -> HashSet<ModuleId> {
        self.interfaces.keys().cloned().collect()
    }

    /// 指定模块标识是否已有缓存.
    #[must_use]
    pub fn is_loaded(&self, name: &str) -> bool {
        self.interfaces.contains_key(&ModuleId::new(name))
    }

    /// 指定类型是否已由缓存模块导出.
    #[must_use]
    pub fn has_data(&self, name: &Name) -> bool {
        self.interfaces
            .values()
            .any(|interface| interface.type_decls.contains_key(name))
    }

    /// 依赖文件变化时丢弃全部缓存; 未启用检测时保持快照.
    pub(crate) fn refresh(&mut self) {
        if !self.watch_files {
            return;
        }
        let stale = self
            .files
            .values()
            .any(|(path, stamp)| file_stamp(path) != Some(*stamp));
        if stale {
            self.invalidate();
        }
    }

    /// 记录本次链接产生的模块接口.
    pub(crate) fn update_graph(&mut self, graph: &link::CheckedGraph) {
        for (id, module) in &graph.modules {
            if *id == graph.root {
                continue;
            }
            self.interfaces.insert(id.clone(), module.interface.clone());
            if let Some(stamp) = file_stamp(&module.path) {
                self.files.insert(id.clone(), (module.path.clone(), stamp));
            }
        }
    }
}
