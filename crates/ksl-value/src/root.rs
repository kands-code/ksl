//! 环回收根集合.
//!
//! 宿主根作用域, 挂起任务的实参与网络目标句柄共用这一表示,
//! 使根值的遍历入口保持一致.

use crate::value::{Env, Value};

/// 一组必须在环回收中保持存活的值; 由调度器根作用域与在途 I/O 操作持有.
#[derive(Debug, Default, Clone)]
pub struct RootSet(Vec<Value>);

impl RootSet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 环境中已初始化的值.
    #[must_use]
    pub fn from_env(env: &Env) -> Self {
        Self(env.values().cloned().collect())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        self.0.iter()
    }
}

impl From<Vec<Value>> for RootSet {
    fn from(values: Vec<Value>) -> Self {
        Self(values)
    }
}

impl FromIterator<Value> for RootSet {
    fn from_iter<I: IntoIterator<Item = Value>>(values: I) -> Self {
        Self(values.into_iter().collect())
    }
}
