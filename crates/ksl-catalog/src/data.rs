//! 内置 Data 类型表.
//!
//! 类型名与变体顺序是语言的一部分; 前端据此建立 Data 表, 保留名与签名共用同一份定义.

use std::sync::LazyLock;

use ksl_base::name::Name;
use ksl_base::types::Type;

/// 一个内置 Data 类型.
pub struct BuiltinData {
    pub name: &'static str,
    pub variants: Vec<BuiltinVariant>,
}

/// 一个内置变体.
pub struct BuiltinVariant {
    pub name: &'static str,
    pub fields: Vec<Type>,
}

/// 由变体表构造内置 Data 类型; 项顺序即变体顺序.
macro_rules! data_info {
    ($($name:literal => [$($field:expr),*]),* $(,)?) => {
        vec![$(BuiltinVariant { name: $name, fields: vec![$($field),*] }),*]
    };
}

/// 全部内置 Data 类型.
#[must_use]
pub fn builtin_data_types() -> &'static [BuiltinData] {
    static TYPES: LazyLock<Vec<BuiltinData>> = LazyLock::new(|| {
        vec![
            BuiltinData {
                name: "ExitReason",
                variants: exit_reason(),
            },
            BuiltinData {
                name: "FileError",
                variants: file_error(),
            },
            BuiltinData {
                name: "InputError",
                variants: input_error(),
            },
            BuiltinData {
                name: "NumberParseError",
                variants: number_parse_error(),
            },
            BuiltinData {
                name: "Utf8Error",
                variants: utf8_error(),
            },
            BuiltinData {
                name: "SocketError",
                variants: socket_error(),
            },
            BuiltinData {
                name: "TimeError",
                variants: time_error(),
            },
            BuiltinData {
                name: "KeyEvent",
                variants: key_event(),
            },
            BuiltinData {
                name: "RegexError",
                variants: regex_error(),
            },
            BuiltinData {
                name: "CommandInput",
                variants: command_input(),
            },
            BuiltinData {
                name: "CommandOption",
                variants: command_option(),
            },
            BuiltinData {
                name: "CommandStatus",
                variants: command_status(),
            },
            BuiltinData {
                name: "CommandError",
                variants: command_error(),
            },
        ]
    });
    TYPES.as_slice()
}

fn exit_reason() -> Vec<BuiltinVariant> {
    data_info! {
        "Halted" => [],
        "Killed" => [],
        "Failed" => [Type::I64, Type::Str],
    }
}

fn file_error() -> Vec<BuiltinVariant> {
    data_info! {
        "NotFound" => [Type::Str],
        "PermissionDenied" => [Type::Str],
        "AlreadyExists" => [Type::Str],
        "InvalidUtf8" => [Type::Str],
        "NotADirectory" => [Type::Str],
        "IsADirectory" => [Type::Str],
        "DirectoryNotEmpty" => [Type::Str],
        // 句柄已关闭或无效; 无路径可报, 故不带载荷.
        "InvalidHandle" => [],
        // 句柄方向不符; 无路径或消息可报.
        "NotReadable" => [],
        "NotWritable" => [],
        "Other" => [Type::Str],
    }
}

fn input_error() -> Vec<BuiltinVariant> {
    data_info! {
        // 以下变体均不带载荷, 没有路径或消息可报.
        // 标准输入不是终端.
        "NotATerminal" => [],
        // 用户中断当前行读取.
        "Interrupted" => [],
        // 原始模式下调用了需要行规程的读取.
        "RawModeActive" => [],
        // 原始模式已由其他持有者占用.
        "RawModeHeld" => [],
        // 当前 Actor 不是原始模式持有者.
        "NotRawModeHolder" => [],
        // 读取因原始模式持有被释放而中止.
        "RawModeReleased" => [],
        "Other" => [Type::Str],
    }
}

fn number_parse_error() -> Vec<BuiltinVariant> {
    data_info! {
        "Empty" => [],
        "Invalid" => [],
        "OutOfRange" => [],
    }
}

fn utf8_error() -> Vec<BuiltinVariant> {
    data_info! {
        "Invalid" => [Type::U64],
        "Incomplete" => [Type::U64],
    }
}

fn socket_error() -> Vec<BuiltinVariant> {
    data_info! {
        "TimedOut" => [],
        "ConnectionRefused" => [],
        "ConnectionReset" => [],
        "ConnectionClosed" => [],
        "Closed" => [],
        "AddrInUse" => [],
        "AddrNotAvailable" => [],
        "PermissionDenied" => [],
        "InvalidUtf8" => [],
        "LineTooLong" => [],
        "TooLarge" => [],
        "UnexpectedEof" => [],
        "InvalidArgument" => [Type::Str],
        "Other" => [Type::Str],
    }
}

fn time_error() -> Vec<BuiltinVariant> {
    data_info! {
        "InvalidFormat" => [Type::Str],
        "UnknownZone" => [Type::Str],
        "InvalidField" => [Type::Str],
    }
}

fn key_event() -> Vec<BuiltinVariant> {
    data_info! {
        // 修饰键折进变体, F 键带编号.
        "Char" => [Type::Char],
        "Ctrl" => [Type::Char],
        "Alt" => [Type::Char],
        "F" => [Type::U8],
        "Enter" => [],
        "Escape" => [],
        "Backspace" => [],
        "Tab" => [],
        "BackTab" => [],
        "Up" => [],
        "Down" => [],
        "Left" => [],
        "Right" => [],
        "Home" => [],
        "End" => [],
        "PageUp" => [],
        "PageDown" => [],
        "Delete" => [],
        "Insert" => [],
    }
}

fn regex_error() -> Vec<BuiltinVariant> {
    data_info! {
        "Invalid" => [Type::U64],
        "UnclosedGroup" => [Type::U64],
        "UnopenedGroup" => [Type::U64],
        "UnclosedClass" => [Type::U64],
        "InvalidClass" => [Type::U64],
        "InvalidEscape" => [Type::U64],
        "InvalidNumber" => [Type::U64],
        "InvalidRepeat" => [Type::U64],
        "InvalidName" => [Type::U64],
        "DuplicateName" => [Type::U64],
        "InvalidFlag" => [Type::U64],
        "InvalidUnicode" => [Type::U64],
        "UnsupportedFeature" => [Type::U64],
        "TooComplex" => [Type::U64],
        // 编译体积超限针对整个模式, 没有位置可报.
        "TooLarge" => [],
    }
}

fn command_input() -> Vec<BuiltinVariant> {
    data_info! {
        "Text" => [Type::Str],
        "Binary" => [Type::list(Type::U8)],
    }
}

fn command_option() -> Vec<BuiltinVariant> {
    data_info! {
        "Cwd" => [Type::Str],
        "Env" => [Type::list(Type::pair(Type::Str, Type::Str))],
        "Input" => [Type::data(Name::new("CommandInput"))],
    }
}

fn command_status() -> Vec<BuiltinVariant> {
    data_info! {
        "Exit" => [Type::U64],
        "Signal" => [Type::U64],
    }
}

fn command_error() -> Vec<BuiltinVariant> {
    data_info! {
        "NotFound" => [Type::Str],
        "PermissionDenied" => [Type::Str],
        "InvalidCwd" => [Type::Str],
        "DuplicateOption" => [Type::Str],
        "DuplicateEnv" => [Type::Str],
        "OutputTooLarge" => [Type::Str],
        "Other" => [Type::Str],
    }
}
