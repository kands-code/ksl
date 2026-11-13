//! 符号驻留.
//!
//! 标识符与类型名在词法层驻留为进程内唯一的 u32; 内表用字符串到编号的哈希表
//! 与编号到字符串的向量维护双向查找. 全局唯一表加读写锁; 已驻留名走读锁
//! 快路径, 只有新名才取写锁, 求值热路径因此不争用写锁.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, LazyLock};

use parking_lot::RwLock;

/// 驻留后的标识符, 进程内唯一, 拷贝与比较均为 O(1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(u32);

impl Symbol {
    /// 内部编号, 仅供驻留器与调试使用.
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", resolve(*self))
    }
}

/// 全局驻留器; 编号即插入顺序, 预置名在首次使用时先占号.
struct Interner {
    by_text: HashMap<Arc<str>, Symbol>,
    texts: Vec<Arc<str>>,
}

impl Interner {
    fn intern(&mut self, text: &str) -> Symbol {
        if let Some(symbol) = self.by_text.get(text) {
            return *symbol;
        }
        let index = u32::try_from(self.texts.len()).expect("符号数量不超过 u32 上限");
        let symbol = Symbol(index);
        let text: Arc<str> = Arc::from(text);
        self.texts.push(Arc::clone(&text));
        self.by_text.insert(text, symbol);
        symbol
    }
}

static INTERNER: LazyLock<RwLock<Interner>> = LazyLock::new(|| {
    RwLock::new(Interner {
        by_text: HashMap::new(),
        texts: Vec::new(),
    })
});

/// 驻留字符串, 已驻留的字符串直接返回既有符号.
///
/// 先走读锁快路径: `Option/Some` 一类构造在求值热路径上反复取同一批名字,
/// 不应每次都申请写锁.
pub fn intern(text: &str) -> Symbol {
    if let Some(symbol) = INTERNER.read().by_text.get(text) {
        return *symbol;
    }
    INTERNER.write().intern(text)
}

/// 解析符号为字符串; 正常路径符号必已驻留, 未知符号给出占位文本以避免二次故障.
pub fn resolve(symbol: Symbol) -> Arc<str> {
    INTERNER
        .read()
        .texts
        .get(symbol.index() as usize)
        .cloned()
        .unwrap_or_else(|| Arc::from("<未知符号>"))
}

/// 需要按符号比较的常用语言级名字: 字面值, 特殊形式与部分类型名.
/// 字段在首次访问时驻留, 以此处字面量为唯一来源, 不存在漂移问题.
pub struct Keywords {
    pub true_: Symbol,
    pub false_: Symbol,
    pub nil_: Symbol,
    pub let_: Symbol,
    pub block_: Symbol,
    pub if_: Symbol,
    pub fn_: Symbol,
    pub def_: Symbol,
    pub exit_: Symbol,
    pub handler_: Symbol,
    pub main_: Symbol,
    pub continue_: Symbol,
    pub spawn_: Symbol,
    pub spawn_detached_: Symbol,
    pub send_: Symbol,
    pub receive_: Symbol,
    pub join_: Symbol,
    pub kill_: Symbol,
    pub monitor_: Symbol,
    pub unmonitor_: Symbol,
    pub pid_serial_: Symbol,
    pub set_timer_: Symbol,
    pub cancel_timer_: Symbol,
    pub halt_: Symbol,
    pub sleep_: Symbol,
    pub fix_: Symbol,
    pub data_: Symbol,
    pub export_: Symbol,
    pub use_: Symbol,
    pub match_: Symbol,
    pub i64_: Symbol,
    pub u8_: Symbol,
    pub u64_: Symbol,
    pub f64_: Symbol,
    pub char_: Symbol,
    pub bool_: Symbol,
    pub string_: Symbol,
    pub list_: Symbol,
    pub pair_: Symbol,
    pub option_: Symbol,
    pub result_: Symbol,
    pub pid_: Symbol,
    pub assert_: Symbol,
    pub panic_: Symbol,
    pub todo_: Symbol,
    pub unreachable_: Symbol,
}

static KEYWORDS: LazyLock<Keywords> = LazyLock::new(|| Keywords {
    true_: intern("true"),
    false_: intern("false"),
    nil_: intern("nil"),
    let_: intern("Let"),
    block_: intern("Block"),
    if_: intern("If"),
    fn_: intern("Fn"),
    def_: intern("Def"),
    exit_: intern("Exit"),
    handler_: intern("Handler"),
    main_: intern("Main"),
    continue_: intern("Continue"),
    spawn_: intern("Spawn"),
    spawn_detached_: intern("SpawnDetached"),
    send_: intern("Send"),
    receive_: intern("Receive"),
    join_: intern("Join"),
    kill_: intern("Kill"),
    monitor_: intern("Monitor"),
    unmonitor_: intern("Unmonitor"),
    pid_serial_: intern("PidSerial"),
    set_timer_: intern("SetTimer"),
    cancel_timer_: intern("CancelTimer"),
    halt_: intern("Halt"),
    sleep_: intern("Sleep"),
    fix_: intern("Fix"),
    data_: intern("Data"),
    export_: intern("Export"),
    use_: intern("Use"),
    match_: intern("Match"),
    i64_: intern("i64"),
    u8_: intern("u8"),
    u64_: intern("u64"),
    f64_: intern("f64"),
    char_: intern("char"),
    bool_: intern("bool"),
    string_: intern("string"),
    list_: intern("List"),
    pair_: intern("Pair"),
    option_: intern("Option"),
    result_: intern("Result"),
    pid_: intern("Pid"),
    assert_: intern("Assert"),
    panic_: intern("Panic"),
    todo_: intern("Todo"),
    unreachable_: intern("Unreachable"),
});

/// 语言级名字的预驻留表.
#[must_use]
pub fn keywords() -> &'static Keywords {
    &KEYWORDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_idempotent_and_reversible() {
        let a = intern("some_identifier");
        let b = intern("some_identifier");
        assert_eq!(a, b);
        assert_eq!(&*resolve(a), "some_identifier");
        assert_ne!(a, intern("other"));

        let mut seen = std::collections::HashSet::new();
        for i in 0..1024 {
            let text = format!("many_symbol_{i}");
            let symbol = intern(&text);
            assert!(seen.insert(symbol.index()), "新文本应分配唯一编号");
            assert_eq!(&*resolve(symbol), text.as_str());
        }
    }
}
