//! 符号驻留.
//!
//! 标识符与类型名在词法层驻留为进程内唯一的 u32.
//! 内表用字符串到编号的哈希表与编号到字符串的向量维护双向查找.
//! 全局驻留表由读写锁保护, 写锁只在驻留新名时获取.

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

/// 解析符号为字符串. 正常路径符号必已驻留, 未知符号给出占位文本以避免二次故障.
pub fn resolve(symbol: Symbol) -> Arc<str> {
    INTERNER
        .read()
        .texts
        .get(symbol.index() as usize)
        .cloned()
        .unwrap_or_else(|| Arc::from("<未知符号>"))
}

/// 需要按符号比较的常用语言级名字: 字面值, 类型名与内置变体名.
/// 字段在首次访问时驻留. 名字只在此定义一次, 避免多处字面量不一致.
/// 特殊形式名不在此表: 它们由 `ksl_catalog::forms` 的形式表给出, `ast` 仅重导出.
pub struct Keywords {
    pub true_: Symbol,
    pub false_: Symbol,
    pub nil_: Symbol,
    pub fn_: Symbol,
    pub handler_: Symbol,
    pub i64_: Symbol,
    pub u8_: Symbol,
    pub u64_: Symbol,
    pub f64_: Symbol,
    pub char_: Symbol,
    pub bool_: Symbol,
    pub string_: Symbol,
    pub list_: Symbol,
    pub dict_: Symbol,
    pub set_: Symbol,
    pub pair_: Symbol,
    pub option_: Symbol,
    pub result_: Symbol,
    pub pid_: Symbol,
    pub file_: Symbol,
    pub command_output_: Symbol,
    pub date_time_: Symbol,
    pub socket_: Symbol,
    pub tcp_listener_: Symbol,
    pub regex_: Symbol,
    pub regex_match_: Symbol,
    /// Option/Result 变体名; 检查器与运行时按符号比较.
    pub some_: Symbol,
    pub none_: Symbol,
    pub ok_: Symbol,
    pub err_: Symbol,
    /// Actor 退出原因的类型名与变体名.
    pub exit_reason_: Symbol,
    pub halted_: Symbol,
    pub killed_: Symbol,
    pub failed_: Symbol,
}

static KEYWORDS: LazyLock<Keywords> = LazyLock::new(|| Keywords {
    true_: intern("true"),
    false_: intern("false"),
    nil_: intern("nil"),
    fn_: intern("Fn"),
    handler_: intern("Handler"),
    i64_: intern("i64"),
    u8_: intern("u8"),
    u64_: intern("u64"),
    f64_: intern("f64"),
    char_: intern("char"),
    bool_: intern("bool"),
    string_: intern("string"),
    list_: intern("List"),
    dict_: intern("Dict"),
    set_: intern("Set"),
    pair_: intern("Pair"),
    option_: intern("Option"),
    result_: intern("Result"),
    pid_: intern("Pid"),
    file_: intern("File"),
    command_output_: intern("CommandOutput"),
    date_time_: intern("DateTime"),
    socket_: intern("Socket"),
    tcp_listener_: intern("TcpListener"),
    regex_: intern("Regex"),
    regex_match_: intern("RegexMatch"),
    some_: intern("Some"),
    none_: intern("None"),
    ok_: intern("Ok"),
    err_: intern("Err"),
    exit_reason_: intern("ExitReason"),
    halted_: intern("Halted"),
    killed_: intern("Killed"),
    failed_: intern("Failed"),
});

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
