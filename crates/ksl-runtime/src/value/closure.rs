//! 闭包与递归写一次单元.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use ksl_base::symbol::Symbol;
use parking_lot::RwLock;

use super::Value;
use crate::cps::linear::{Function as LinearFunction, Functions};

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
    /// 具名函数显示用名字; 匿名函数与 Handler 为 None.
    pub name: Option<Symbol>,
}

impl Closure {
    #[must_use]
    pub(crate) fn new(
        arity: usize,
        function: Arc<LinearFunction>,
        captures: Arc<[Capture]>,
        functions: Arc<Functions>,
        name: Option<Symbol>,
    ) -> Self {
        Self {
            arity,
            function,
            empty_captures: AtomicBool::new(captures.is_empty()),
            captures: RwLock::new(captures),
            functions,
            name,
        }
    }

    /// 显示用签名, 例如 `Fn[[i64] : i64]`; 只在展示闭包时构造.
    #[must_use]
    pub fn signature_text(&self) -> String {
        let params = self
            .function
            .params
            .iter()
            .map(|binder| binder.ty.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        match self.function.kind {
            crate::cps::ir::FunctionKind::Fn => {
                format!("Fn[[{params}] : {}]", self.function.ret)
            },
            crate::cps::ir::FunctionKind::Handler => {
                format!("Handler[[{params}] : {}]", self.function.ret)
            },
        }
    }

    /// 读取捕获值.
    ///
    /// 捕获只会在构造时写入, 之后仅由环回收清空, 因此标志一旦为真就一直为真.
    #[must_use]
    pub(crate) fn captures(&self) -> Arc<[Capture]> {
        if self.empty_captures.load(Ordering::Relaxed) {
            return Arc::clone(&EMPTY_CAPTURES);
        }
        Arc::clone(&self.captures.read())
    }

    /// 断开不可达闭包的捕获引用; 只能由环回收在安全点调用.
    pub(crate) fn clear_captures(&self) {
        *self.captures.write() = Arc::from(Vec::new());
        self.empty_captures.store(true, Ordering::Relaxed);
    }
}

/// 闭包捕获: 普通绑定按值捕获, 递归绑定共享写一次单元.
#[derive(Debug, Clone)]
pub(crate) enum Capture {
    Value(Value),
    Fix(FixCell),
    /// 全局环境缺少该名字; 读取时报告内部错误.
    Missing(Symbol),
}

impl Capture {
    pub(crate) fn get(&self) -> Option<Value> {
        match self {
            Self::Value(value) => Some(value.clone()),
            Self::Fix(cell) => cell.get().cloned(),
            Self::Missing(_) => None,
        }
    }
}

/// 递归定义的写一次单元; 建好闭包后回填.
pub(crate) type FixCell = Arc<OnceLock<Value>>;
