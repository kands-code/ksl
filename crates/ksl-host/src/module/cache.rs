//! 模块运行缓存: 已执行模块的导出值与插件实例.
//!
//! 只由执行阶段写入; 与编译缓存一起提交或回滚.

use std::collections::HashMap;

use ksl_base::name::Name;
use ksl_compile::ModuleId;
use ksl_value::value::Value;

use crate::plugin::LinkCache;

/// 已执行模块的导出值.
#[derive(Debug, Clone)]
pub struct ModuleInstance {
    pub exports: HashMap<Name, Value>,
}

/// 运行缓存: 已执行模块的导出值与插件实例.
#[derive(Debug, Clone, Default)]
pub struct InstanceCache {
    pub(crate) instances: imbl::HashMap<ModuleId, ModuleInstance>,
    pub(crate) links: LinkCache,
}

impl InstanceCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 缓存导出的运行时值; 环回收作为外部根传入.
    #[must_use]
    pub fn root_values(&self) -> Vec<Value> {
        value_roots(&self.instances)
    }

    /// 记录本次执行产生的模块导出值.
    pub(crate) fn update_values(&mut self, instances: imbl::HashMap<ModuleId, ModuleInstance>) {
        self.instances = instances;
    }
}

/// 模块导出值展平为根列表.
#[must_use]
pub(crate) fn value_roots(instances: &imbl::HashMap<ModuleId, ModuleInstance>) -> Vec<Value> {
    instances
        .values()
        .flat_map(|instance| instance.exports.values().cloned())
        .collect()
}
