//! CPS IR: typed AST 降级为函数级 CPS 形式.
//!
//! 该 IR 是线性化的输入, 本身不直接执行; `ksl cps` 输出它供调试.
//! 续延, Actor 挂起与尾调用在文本中显式可见.
//!
//! 约定:
//! - 每个函数带一个续延参数 `$k`; `jump $k(value)` 即函数返回.
//! - `tailcall` 把当前续延交给被调函数, 不增长续延链.
//! - `Eval` 是同步求值, 直接绑定结果并继续; `call` 与 `prim` 为需要续延的调用与原语,
//!   `receive` 是 Actor 的挂起点.
//! - 中间值与生成的续延以稳定编号表示, 显示为 `%N` 与 `$kN`; 源码标识符不会包含 `$`.

mod display;
mod model;

pub use model::*;

#[cfg(test)]
mod tests;
