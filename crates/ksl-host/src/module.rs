//! 模块执行: 按依赖顺序执行已检查模块并缓存导出值.
//!
//! 加载, 链接与检查由 `ksl-compile` 完成; 本模块只消费 `CheckedGraph`.

mod cache;
mod exec;

pub(crate) use cache::value_roots;
pub use cache::{InstanceCache, ModuleInstance};
#[cfg(test)]
pub(crate) use exec::execute;
pub(crate) use exec::{RootEnv, execute_with_cache};

#[cfg(test)]
mod tests;
