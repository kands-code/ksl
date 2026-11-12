//! 特殊形式的种类与名称.
//!
//! 名称表是特殊形式名的唯一来源: 解析层按名查种类并分派,
//! 保留名与编辑器名单按表取名.

/// 由一份形式清单生成种类, 名字查询与名单.
macro_rules! define_forms {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $text:literal
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            /// 按名查种类; 名字不是特殊形式时返回 `None`.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $(
                        $text => Some(Self::$variant),
                    )*
                    _ => None,
                }
            }
        }

        const SPECIAL_FORM_NAMES: &[&str] = &[$($text),*];
    };
}

define_forms! {
    /// 特殊形式的种类.
    ///
    /// 种类只区分形式本身, 不含操作数与结果类型.
    pub enum FormKind {
        Let => "Let",
        Block => "Block",
        If => "If",
        Which => "Which",
        Fn => "Fn",
        Def => "Def",
        Exit => "Exit",
        Handler => "Handler",
        Main => "Main",
        Continue => "Continue",
        Spawn => "Spawn",
        SpawnDetached => "SpawnDetached",
        Send => "Send",
        DataGet => "DataGet",
        Receive => "Receive",
        Join => "Join",
        Kill => "Kill",
        PidSerial => "PidSerial",
        SetTimer => "SetTimer",
        CancelTimer => "CancelTimer",
        Halt => "Halt",
        Sleep => "Sleep",
        Fix => "Fix",
        Data => "Data",
        Export => "Export",
        Use => "Use",
        /// 外来库声明.
        Link => "Link",
        Match => "Match",
        Dict => "Dict",
        Set => "Set",
        Assert => "Assert",
        Panic => "Panic",
        Todo => "Todo",
        Unreachable => "Unreachable",
    }
}

/// 全部特殊形式名.
pub fn special_form_names() -> impl Iterator<Item = &'static str> {
    SPECIAL_FORM_NAMES.iter().copied()
}
