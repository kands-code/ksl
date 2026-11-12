//! 名字类型.
//!
//! 名字是自包含的短字符串: 22 字节以内内联, 更长的使用共享堆分配.
//! 多份运行时副本之间可以直接比较.

use std::fmt;

use smol_str::SmolStr;

/// 源程序中的名字; 自包含, 不依赖全局状态.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(SmolStr);

impl Name {
    #[must_use]
    pub fn new(text: impl AsRef<str>) -> Self {
        Self(SmolStr::new(text))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0.as_str(), f)
    }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        self.0.as_str()
    }
}

impl From<&str> for Name {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

/// 内置名字.
pub struct Keywords {
    pub true_: Name,
    pub false_: Name,
    pub nil_: Name,
    pub fn_: Name,
    pub handler_: Name,
    pub i64_: Name,
    pub i32_: Name,
    pub u8_: Name,
    pub u32_: Name,
    pub u64_: Name,
    pub f32_: Name,
    pub f64_: Name,
    pub char_: Name,
    pub bool_: Name,
    pub string_: Name,
    pub list_: Name,
    pub dict_: Name,
    pub set_: Name,
    pub pair_: Name,
    pub option_: Name,
    pub result_: Name,
    pub pid_: Name,
    pub file_: Name,
    pub command_output_: Name,
    pub date_time_: Name,
    pub socket_: Name,
    pub tcp_listener_: Name,
    pub regex_: Name,
    pub regex_match_: Name,
    pub some_: Name,
    pub none_: Name,
    pub ok_: Name,
    pub err_: Name,
    pub exit_reason_: Name,
    pub halted_: Name,
    pub killed_: Name,
    pub failed_: Name,
}

static KEYWORDS: std::sync::LazyLock<Keywords> = std::sync::LazyLock::new(|| Keywords {
    true_: Name::new("true"),
    false_: Name::new("false"),
    nil_: Name::new("nil"),
    fn_: Name::new("Fn"),
    handler_: Name::new("Handler"),
    i64_: Name::new("i64"),
    i32_: Name::new("i32"),
    u8_: Name::new("u8"),
    u32_: Name::new("u32"),
    u64_: Name::new("u64"),
    f32_: Name::new("f32"),
    f64_: Name::new("f64"),
    char_: Name::new("char"),
    bool_: Name::new("bool"),
    string_: Name::new("string"),
    list_: Name::new("List"),
    dict_: Name::new("Dict"),
    set_: Name::new("Set"),
    pair_: Name::new("Pair"),
    option_: Name::new("Option"),
    result_: Name::new("Result"),
    pid_: Name::new("Pid"),
    file_: Name::new("File"),
    command_output_: Name::new("CommandOutput"),
    date_time_: Name::new("DateTime"),
    socket_: Name::new("Socket"),
    tcp_listener_: Name::new("TcpListener"),
    regex_: Name::new("Regex"),
    regex_match_: Name::new("RegexMatch"),
    some_: Name::new("Some"),
    none_: Name::new("None"),
    ok_: Name::new("Ok"),
    err_: Name::new("Err"),
    exit_reason_: Name::new("ExitReason"),
    halted_: Name::new("Halted"),
    killed_: Name::new("Killed"),
    failed_: Name::new("Failed"),
});

#[must_use]
pub fn keywords() -> &'static Keywords {
    &KEYWORDS
}
