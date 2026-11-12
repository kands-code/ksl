//! ksl 编译层: 模块加载, 链接与检查缓存.
//!
//! 输入是文件或内存中的源码, 产物是 [`link::CheckedGraph`].
//! 本层不依赖运行时值, 抽象机与调度器.

mod cache;
mod graph;
pub mod link;

pub use cache::CompileCache;
pub use graph::{
    CachedImports, ImportDecl, ModuleGraph, ModuleId, ModuleUnit, RootSource, compile, load,
    module_name, normalize_main, root_dir_of, take_main,
};
pub use link::{CheckedGraph, CheckedModule, ModuleInterface};
