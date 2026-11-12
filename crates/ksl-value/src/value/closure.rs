//! 闭包与递归写一次单元.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use ksl_base::name::Name;
use ksl_plan::{Function as LinearFunction, Functions};
use parking_lot::RwLock;

use super::Value;

/// 空捕获向量; 无捕获的闭包共用这一份.
static EMPTY_CAPTURES: std::sync::LazyLock<Arc<[Capture]>> =
    std::sync::LazyLock::new(|| Arc::from(Vec::new()));

/// 线性 IR 闭包: 指向函数体, 携带捕获与函数表.
#[derive(Debug)]
pub struct Closure {
    /// 参数个数; 调用时校验实参个数.
    pub arity: usize,
    pub function: Arc<LinearFunction>,
    /// 捕获值; 递归定义使用写一次单元, 其余按值捕获.
    captures: RwLock<Arc<[Capture]>>,
    /// 捕获是否为空; 空捕获时读取不需要取锁.
    empty_captures: AtomicBool,
    pub functions: Arc<Functions>,
}

impl Closure {
    #[must_use]
    pub fn new(
        arity: usize,
        function: Arc<LinearFunction>,
        captures: Arc<[Capture]>,
        functions: Arc<Functions>,
    ) -> Self {
        Self {
            arity,
            function,
            empty_captures: AtomicBool::new(captures.is_empty()),
            captures: RwLock::new(captures),
            functions,
        }
    }

    /// 是否捕获了外部值; 供宿主与测试检查闭包存活状态.
    #[must_use]
    pub fn has_captures(&self) -> bool {
        !self.empty_captures.load(Ordering::Relaxed) && !self.captures.read().is_empty()
    }

    /// 捕获只在构造时写入, 只由环回收清空; 标志一旦为真便保持为真.
    #[must_use]
    pub fn captures(&self) -> Arc<[Capture]> {
        if self.empty_captures.load(Ordering::Relaxed) {
            return Arc::clone(&EMPTY_CAPTURES);
        }
        Arc::clone(&self.captures.read())
    }

    /// 断开不可达闭包的捕获引用; 只能由环回收在安全点调用.
    pub fn clear_captures(&self) {
        *self.captures.write() = Arc::from(Vec::new());
        self.empty_captures.store(true, Ordering::Relaxed);
    }
}

/// 闭包捕获: 普通绑定按值捕获, 递归绑定共享写一次单元.
#[derive(Debug, Clone)]
pub enum Capture {
    Value(Value),
    Fix(FixCell),
    /// 全局环境缺少该名字; 读取时报告内部错误.
    Missing(Name),
}

impl Capture {
    #[must_use]
    pub fn get(&self) -> Option<Value> {
        match self {
            Self::Value(value) => Some(value.clone()),
            Self::Fix(cell) => cell.get().cloned(),
            Self::Missing(_) => None,
        }
    }
}

/// 递归定义的写一次单元; 建好闭包后回填.
pub type FixCell = Arc<OnceLock<Value>>;
