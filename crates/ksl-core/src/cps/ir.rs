//! CPS IR: 类型检查后的 AST 降级为函数级 CPS 形式.
//!
//! 该 IR 是线性化的输入, 本身不直接执行; `ksl cps` 输出它供调试. 续延,
//! Actor 挂起与尾调用在文本中显式可见, 也为后续生成字节码保留稳定的降级目标.
//!
//! 约定:
//! - 每个函数带一个续延参数 `$k`; `jump $k(value)` 即函数返回.
//! - `tailcall` 把当前续延交给被调函数, 不增长续延链.
//! - `call` 与 `prim` 显式创建续延; `receive` 是 Actor 的挂起点.
//! - 中间值与生成的续延以稳定编号表示, 显示为 `%N` 与 `$kN`; 源码标识符不会包含 `$`.

mod display;
mod lower;
mod model;

pub use lower::lower_program;
pub use model::*;

#[cfg(test)]
mod tests;
