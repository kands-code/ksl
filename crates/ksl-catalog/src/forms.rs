//! 特殊形式的种类与名称.
//!
//! 名称表是特殊形式名的唯一来源: 解析层按名查种类并分派,
//! 保留名与编辑器名单按表取名. 新增特殊形式须在此登记.

/// 特殊形式的种类.
///
/// 种类只区分形式本身, 不含操作数与结果类型.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FormKind {
    Let,
    Block,
    If,
    Which,
    Fn,
    Def,
    Exit,
    Handler,
    Main,
    Continue,
    Spawn,
    SpawnDetached,
    Send,
    Receive,
    Join,
    Kill,
    Monitor,
    Unmonitor,
    PidSerial,
    SetTimer,
    CancelTimer,
    Halt,
    Sleep,
    Fix,
    Data,
    Export,
    Use,
    /// 外来库声明.
    Link,
    Match,
    Dict,
    Set,
    Assert,
    Panic,
    Todo,
    Unreachable,
}

/// 种类与形式名的对应表.
///
/// 除 `Dict` 与 `Set` 构造器外, 操作名不在此表; 它们由操作元数据给出.
const TABLE: &[(FormKind, &str)] = &[
    (FormKind::Let, "Let"),
    (FormKind::Block, "Block"),
    (FormKind::If, "If"),
    (FormKind::Which, "Which"),
    (FormKind::Fn, "Fn"),
    (FormKind::Def, "Def"),
    (FormKind::Exit, "Exit"),
    (FormKind::Handler, "Handler"),
    (FormKind::Main, "Main"),
    (FormKind::Continue, "Continue"),
    (FormKind::Spawn, "Spawn"),
    (FormKind::SpawnDetached, "SpawnDetached"),
    (FormKind::Send, "Send"),
    (FormKind::Receive, "Receive"),
    (FormKind::Join, "Join"),
    (FormKind::Kill, "Kill"),
    (FormKind::Monitor, "Monitor"),
    (FormKind::Unmonitor, "Unmonitor"),
    (FormKind::PidSerial, "PidSerial"),
    (FormKind::SetTimer, "SetTimer"),
    (FormKind::CancelTimer, "CancelTimer"),
    (FormKind::Halt, "Halt"),
    (FormKind::Sleep, "Sleep"),
    (FormKind::Fix, "Fix"),
    (FormKind::Data, "Data"),
    (FormKind::Export, "Export"),
    (FormKind::Use, "Use"),
    (FormKind::Link, "Link"),
    (FormKind::Match, "Match"),
    (FormKind::Dict, "Dict"),
    (FormKind::Set, "Set"),
    (FormKind::Assert, "Assert"),
    (FormKind::Panic, "Panic"),
    (FormKind::Todo, "Todo"),
    (FormKind::Unreachable, "Unreachable"),
];

impl FormKind {
    /// 按名查种类; 名字不是特殊形式时返回 `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        TABLE
            .iter()
            .find(|(_, text)| *text == name)
            .map(|(kind, _)| *kind)
    }
}

/// 全部特殊形式名.
pub fn special_form_names() -> impl Iterator<Item = &'static str> {
    TABLE.iter().map(|(_, name)| *name)
}
