//! 内置函数元数据.
//!
//! 名字, 签名与分类集中在本表; 实现分派位于运行时.

use std::collections::HashMap;
use std::sync::LazyLock;

use ksl_base::symbol::{Symbol, intern};
use ksl_base::types::Type;

/// 内置函数编号; 降级期解析, 运行时分派不再查符号表.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinId(u16);

impl BuiltinId {
    #[must_use]
    pub fn index(self) -> u16 {
        self.0
    }

    /// 由编号构造; 仅供注册表与测试.
    #[must_use]
    pub fn from_index(index: u16) -> Self {
        Self(index)
    }
}

/// 内置函数的调用形态.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
    /// 普通函数, 在抽象机内直接返回.
    Pure,
    /// I/O 任务构造器, 提交任务后挂起.
    Io,
}

/// 内置函数元数据: 名字, 签名与调用形态.
pub struct BuiltinSpec {
    /// 驻留后的函数名.
    pub symbol: Symbol,
    /// 源码中的函数名.
    pub name: &'static str,
    /// 函数签名.
    pub signature: Type,
    /// 调用形态.
    pub kind: BuiltinKind,
}

/// 全部内置函数元数据; 顺序即 `BuiltinId`.
static TABLE: LazyLock<Vec<BuiltinSpec>> = LazyLock::new(|| {
    use Type::{Bool as B, Char as C, F64 as F, I64 as I, Nil as N, Str as S, U8, U64};

    let io_entries: [(&'static str, Type); 31] = [
        ("ReadLine", fun(vec![S], input_result(Type::option(S)))),
        ("ReadFile", fun(vec![S], file_result(S))),
        ("ReadFileBytes", fun(vec![S], file_result(Type::list(U8)))),
        ("ReadKey", fun(vec![], input_result(data("KeyEvent")))),
        ("EnterRawMode", fun(vec![], input_result(N))),
        ("ExitRawMode", fun(vec![], input_result(N))),
        ("WriteFile", fun(vec![S, S], file_result(N))),
        ("WriteFileBytes", fun(vec![S, Type::list(U8)], file_result(N))),
        ("AppendFile", fun(vec![S, S], file_result(N))),
        ("AppendFileBytes", fun(vec![S, Type::list(U8)], file_result(N))),
        ("ListDirectory", fun(vec![S], file_result(Type::list(S)))),
        ("CreateDirectory", fun(vec![S], file_result(N))),
        ("CreateDirectoryAll", fun(vec![S], file_result(N))),
        ("RemoveDirectory", fun(vec![S], file_result(N))),
        ("RemoveFile", fun(vec![S], file_result(N))),
        ("RenamePath", fun(vec![S, S], file_result(N))),
        ("CopyFile", fun(vec![S, S], file_result(N))),
        ("IsPathExists", fun(vec![S], B)),
        ("IsFile", fun(vec![S], B)),
        ("IsDirectory", fun(vec![S], B)),
        ("GetCurrentDirectory", fun(vec![], file_result(S))),
        ("OpenRead", fun(vec![S], file_result(Type::file()))),
        ("OpenWrite", fun(vec![S], file_result(Type::file()))),
        ("OpenAppend", fun(vec![S], file_result(Type::file()))),
        ("Close", fun(vec![Type::file()], file_result(N))),
        ("Flush", fun(vec![Type::file()], file_result(N))),
        (
            "ReadLineFrom",
            fun(vec![Type::file()], file_result(Type::option(S))),
        ),
        (
            "ReadChunk",
            fun(
                vec![Type::file(), U64],
                file_result(Type::option(Type::list(U8))),
            ),
        ),
        ("WriteString", fun(vec![Type::file(), S], file_result(N))),
        (
            "WriteBytes",
            fun(vec![Type::file(), Type::list(U8)], file_result(N)),
        ),
        (
            "RunCommand",
            fun(
                vec![S, Type::list(S), Type::list(data("CommandOption"))],
                command_result(Type::command_output()),
            ),
        ),
    ];

    let socket_entries: [(&'static str, Type); 16] = [
        (
            "TcpListen",
            fun(vec![S, U64], socket_result(Type::tcp_listener())),
        ),
        (
            "TcpAccept",
            fun(vec![Type::tcp_listener(), U64], socket_result(Type::socket())),
        ),
        (
            "TcpConnect",
            fun(vec![S, U64, U64], socket_result(Type::socket())),
        ),
        ("TcpCloseListener", fun(vec![Type::tcp_listener()], N)),
        ("TcpLocalPort", fun(vec![Type::tcp_listener()], U64)),
        ("SocketPeer", fun(vec![Type::socket()], S)),
        ("SocketIsClosed", fun(vec![Type::socket()], B)),
        (
            "SocketShutdownWrite",
            fun(vec![Type::socket(), U64], socket_result(N)),
        ),
        (
            "SocketRead",
            fun(
                vec![Type::socket(), U64, U64],
                socket_result(Type::option(Type::list(U8))),
            ),
        ),
        (
            "SocketReadExact",
            fun(vec![Type::socket(), U64, U64], socket_result(Type::list(U8))),
        ),
        (
            "SocketReadAll",
            fun(vec![Type::socket(), U64, U64], socket_result(Type::list(U8))),
        ),
        (
            "SocketReadLine",
            fun(vec![Type::socket(), U64, U64], socket_result(Type::option(S))),
        ),
        (
            "SocketWrite",
            fun(vec![Type::socket(), Type::list(U8), U64], socket_result(U64)),
        ),
        (
            "SocketWriteAll",
            fun(vec![Type::socket(), Type::list(U8), U64], socket_result(N)),
        ),
        (
            "SocketWriteLine",
            fun(vec![Type::socket(), S, U64], socket_result(N)),
        ),
        ("SocketClose", fun(vec![Type::socket()], N)),
    ];

    let entries: [(&'static str, Type); 185] = [
        ("AddI64", bin(I)),
        ("SubI64", bin(I)),
        ("MulI64", bin(I)),
        ("DivideI64", bin(I)),
        ("ModuloI64", bin(I)),
        ("RemainderI64", bin(I)),
        ("QuotientI64", bin(I)),
        ("AddU8", bin(U8)),
        ("SubU8", bin(U8)),
        ("MulU8", bin(U8)),
        ("DivideU8", bin(U8)),
        ("ModuloU8", bin(U8)),
        ("AddU64", bin(U64)),
        ("SubU64", bin(U64)),
        ("MulU64", bin(U64)),
        ("DivideU64", bin(U64)),
        ("ModuloU64", bin(U64)),
        ("AddF64", bin(F)),
        ("SubF64", bin(F)),
        ("MulF64", bin(F)),
        ("DivideF64", bin(F)),
        ("AbsI64", un(I)),
        ("AbsF64", un(F)),
        ("Power", bin(F)),
        ("Sqrt", un(F)),
        ("Sin", un(F)),
        ("Cos", un(F)),
        ("Tan", un(F)),
        ("ArcSin", un(F)),
        ("ArcCos", un(F)),
        ("ArcTan", un(F)),
        ("ArcTan2", bin(F)),
        ("Sinh", un(F)),
        ("Cosh", un(F)),
        ("Tanh", un(F)),
        ("ArcSinh", un(F)),
        ("ArcCosh", un(F)),
        ("ArcTanh", un(F)),
        ("ShiftLeftI64", fun(vec![I, U8], I)),
        ("ShiftRightI64", fun(vec![I, U8], I)),
        ("BitOrI64", bin(I)),
        ("BitAndI64", bin(I)),
        ("BitXorI64", bin(I)),
        ("BitReverseI64", fun(vec![I], I)),
        ("ShiftLeftU8", fun(vec![U8, U8], U8)),
        ("ShiftRightU8", fun(vec![U8, U8], U8)),
        ("BitOrU8", bin(U8)),
        ("BitAndU8", bin(U8)),
        ("BitXorU8", bin(U8)),
        ("BitReverseU8", fun(vec![U8], U8)),
        ("ShiftLeftU64", fun(vec![U64, U8], U64)),
        ("ShiftRightU64", fun(vec![U64, U8], U64)),
        ("BitOrU64", bin(U64)),
        ("BitAndU64", bin(U64)),
        ("BitXorU64", bin(U64)),
        ("BitReverseU64", fun(vec![U64], U64)),
        ("I64ToF64", fun(vec![I], F)),
        ("Truncate", fun(vec![F], I)),
        ("Round", fun(vec![F], I)),
        ("Ceiling", fun(vec![F], I)),
        ("Floor", fun(vec![F], I)),
        ("U8ToI64", fun(vec![U8], I)),
        ("I64ToU8", fun(vec![I], U8)),
        ("U8ToU64", fun(vec![U8], U64)),
        ("U64ToU8", fun(vec![U64], U8)),
        ("I64ToU64", fun(vec![I], U64)),
        ("U64ToI64", fun(vec![U64], I)),
        ("U64ToF64", fun(vec![U64], F)),
        ("CharToU64", fun(vec![C], U64)),
        ("U64ToChar", fun(vec![U64], C)),
        ("StringToChars", fun(vec![S], Type::list(C))),
        ("CharsToString", fun(vec![Type::list(C)], S)),
        ("IsWhitespace", fun(vec![C], B)),
        ("IsAsciiDigit", fun(vec![C], B)),
        ("IsLetter", fun(vec![C], B)),
        ("IsUppercase", fun(vec![C], B)),
        ("IsLowercase", fun(vec![C], B)),
        ("IsAlphanumeric", fun(vec![C], B)),
        ("StringLength", fun(vec![S], U64)),
        ("StringSplit", fun(vec![S, S], Type::list(S))),
        ("StringLines", fun(vec![S], Type::list(S))),
        ("StringWords", fun(vec![S], Type::list(S))),
        ("StringTrim", fun(vec![S], S)),
        ("StringTrimStart", fun(vec![S], S)),
        ("StringTrimEnd", fun(vec![S], S)),
        ("StringStartsWith", fun(vec![S, S], B)),
        ("StringEndsWith", fun(vec![S, S], B)),
        ("StringContains", fun(vec![S, S], B)),
        ("StringUppercase", fun(vec![S], S)),
        ("StringLowercase", fun(vec![S], S)),
        ("StringJoin", fun(vec![Type::list(S)], S)),
        ("StringSlice", fun(vec![S, U64, U64], S)),
        ("EncodeUtf8", fun(vec![S], Type::list(U8))),
        ("DecodeUtf8", fun(vec![Type::list(U8)], utf8_result(S))),
        ("ParseI64", fun(vec![S], number_parse_result(I))),
        ("ParseF64", fun(vec![S], number_parse_result(F))),
        ("ParseU64", fun(vec![S], number_parse_result(U64))),
        ("JoinPath", fun(vec![S, S], S)),
        ("Print", fun(vec![S], N)),
        ("PrintError", fun(vec![S], N)),
        ("Args", fun(vec![], Type::list(S))),
        ("ClosedFile", fun(vec![], Type::file())),
        ("GetEnv", fun(vec![S], Type::option(S))),
        ("Not", fun(vec![B], B)),
        ("And", bin(B)),
        ("Or", bin(B)),
        ("EqualU8", fun(vec![U8, U8], B)),
        ("LessU8", fun(vec![U8, U8], B)),
        ("LessOrEqualU8", fun(vec![U8, U8], B)),
        ("GreaterU8", fun(vec![U8, U8], B)),
        ("GreaterOrEqualU8", fun(vec![U8, U8], B)),
        ("EqualU64", fun(vec![U64, U64], B)),
        ("LessU64", fun(vec![U64, U64], B)),
        ("LessOrEqualU64", fun(vec![U64, U64], B)),
        ("GreaterU64", fun(vec![U64, U64], B)),
        ("GreaterOrEqualU64", fun(vec![U64, U64], B)),
        ("EqualI64", fun(vec![I, I], B)),
        ("LessI64", fun(vec![I, I], B)),
        ("LessOrEqualI64", fun(vec![I, I], B)),
        ("GreaterI64", fun(vec![I, I], B)),
        ("GreaterOrEqualI64", fun(vec![I, I], B)),
        ("EqualF64", fun(vec![F, F], B)),
        ("LessF64", fun(vec![F, F], B)),
        ("LessOrEqualF64", fun(vec![F, F], B)),
        ("GreaterF64", fun(vec![F, F], B)),
        ("GreaterOrEqualF64", fun(vec![F, F], B)),
        ("EqualChar", fun(vec![C, C], B)),
        ("EqualString", fun(vec![S, S], B)),
        ("EqualBool", bin(B)),
        ("Now", fun(vec![], Type::date_time())),
        ("NowUtc", fun(vec![], Type::date_time())),
        ("DateYear", time_field()),
        ("DateMonth", time_field()),
        ("DateDay", time_field()),
        ("DateWeekday", time_field()),
        ("TimeHour", time_field()),
        ("TimeMinute", time_field()),
        ("TimeSecond", time_field()),
        ("TimeMillisecond", time_field()),
        ("DateTimeInZone", fun(vec![Type::date_time(), S], time_result())),
        ("TimeZoneName", fun(vec![Type::date_time()], S)),
        ("TimeZoneOffsetMinutes", fun(vec![Type::date_time()], I)),
        ("DateTimeFromParts", fun(vec![I, I, I, I, I, I], time_result())),
        ("ParseDateTime", fun(vec![S], time_result())),
        ("DateTimeToString", fun(vec![Type::date_time()], S)),
        ("DateTimeToEpochMillis", fun(vec![Type::date_time()], I)),
        ("EpochMillisToDateTime", fun(vec![I], Type::date_time())),
        (
            "LessDateTime",
            fun(vec![Type::date_time(), Type::date_time()], B),
        ),
        (
            "LessOrEqualDateTime",
            fun(vec![Type::date_time(), Type::date_time()], B),
        ),
        (
            "EqualDateTime",
            fun(vec![Type::date_time(), Type::date_time()], B),
        ),
        (
            "GreaterDateTime",
            fun(vec![Type::date_time(), Type::date_time()], B),
        ),
        (
            "GreaterOrEqualDateTime",
            fun(vec![Type::date_time(), Type::date_time()], B),
        ),
        ("AddYear", time_shift()),
        ("SubYear", time_shift()),
        ("AddMonth", time_shift()),
        ("SubMonth", time_shift()),
        ("AddWeek", time_shift()),
        ("SubWeek", time_shift()),
        ("AddDay", time_shift()),
        ("SubDay", time_shift()),
        ("AddHour", time_shift()),
        ("SubHour", time_shift()),
        ("AddMinute", time_shift()),
        ("SubMinute", time_shift()),
        ("AddSecond", time_shift()),
        ("SubSecond", time_shift()),
        ("AddMillisecond", time_shift()),
        ("SubMillisecond", time_shift()),
        ("RegexCompile", fun(vec![S], regex_result(Type::regex()))),
        ("RegexIsMatch", fun(vec![Type::regex(), S], B)),
        (
            "RegexFind",
            fun(vec![Type::regex(), S], Type::option(Type::regex_match())),
        ),
        (
            "RegexFindAll",
            fun(vec![Type::regex(), S], Type::list(Type::regex_match())),
        ),
        ("RegexReplace", fun(vec![Type::regex(), S, S], S)),
        ("RegexReplaceAll", fun(vec![Type::regex(), S, S], S)),
        ("RegexSplit", fun(vec![Type::regex(), S], Type::list(S))),
        ("RegexCaptureNames", fun(vec![Type::regex()], Type::list(S))),
        ("EmptyRegexMatch", fun(vec![], Type::regex_match())),
        ("RegexMatchText", fun(vec![Type::regex_match()], S)),
        ("RegexMatchStart", fun(vec![Type::regex_match()], U64)),
        ("RegexMatchEnd", fun(vec![Type::regex_match()], U64)),
        (
            "RegexMatchGroup",
            fun(vec![Type::regex_match(), U64], Type::option(S)),
        ),
        (
            "RegexMatchNamedGroup",
            fun(vec![Type::regex_match(), S], Type::option(S)),
        ),
        (
            "CommandOutputStatus",
            fun(vec![Type::command_output()], data("CommandStatus")),
        ),
        (
            "CommandOutputStdout",
            fun(vec![Type::command_output()], Type::list(U8)),
        ),
        (
            "CommandOutputStderr",
            fun(vec![Type::command_output()], Type::list(U8)),
        ),
    ];
    entries
        .into_iter()
        .map(|(name, signature)| BuiltinSpec {
            symbol: intern(name),
            name,
            signature,
            kind: BuiltinKind::Pure,
        })
        .chain(io_entries.into_iter().map(|(name, signature)| BuiltinSpec {
            symbol: intern(name),
            name,
            signature,
            kind: BuiltinKind::Io,
        }))
        .chain(socket_entries.into_iter().map(|(name, signature)| BuiltinSpec {
            symbol: intern(name),
            name,
            signature,
            kind: BuiltinKind::Io,
        }))
        .collect()
});

static INDEX: LazyLock<HashMap<Symbol, BuiltinId>> = LazyLock::new(|| {
    TABLE
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let id = BuiltinId::from_index(u16::try_from(index).expect("内置函数数量超出 u16"));
            (entry.symbol, id)
        })
        .collect()
});

/// 按符号查内置函数编号; 非内置函数返回 None.
#[must_use]
pub fn id(symbol: Symbol) -> Option<BuiltinId> {
    INDEX.get(&symbol).copied()
}

/// 按符号查函数签名; 非内置函数返回 None.
#[must_use]
pub fn signature(symbol: Symbol) -> Option<&'static Type> {
    INDEX.get(&symbol).map(|id| &TABLE[id.0 as usize].signature)
}

/// 按编号取元数据.
#[must_use]
pub fn spec(id: BuiltinId) -> &'static BuiltinSpec {
    &TABLE[id.0 as usize]
}

/// 全部内置函数元数据, 顺序即编号.
pub fn specs() -> impl Iterator<Item = &'static BuiltinSpec> {
    TABLE.iter()
}

/// 全部内置函数名.
#[must_use]
pub fn names() -> Vec<Symbol> {
    TABLE.iter().map(|entry| entry.symbol).collect()
}

/// 按编号取驻留名.
#[must_use]
pub fn symbol(id: BuiltinId) -> Symbol {
    TABLE[id.0 as usize].symbol
}

/// 按编号取源码名.
#[must_use]
pub fn name(id: BuiltinId) -> &'static str {
    TABLE[id.0 as usize].name
}

/// 内置函数数量.
#[must_use]
pub fn len() -> usize {
    TABLE.len()
}

/// 一元同类型函数的便捷构造.
fn un(ty: Type) -> Type {
    fun(vec![ty.clone()], ty)
}

/// 时间字段取值: `DateTime -> i64`.
fn time_field() -> Type {
    fun(vec![Type::date_time()], Type::I64)
}

/// 可失败的时间操作: `Result[DateTime, TimeError]`.
fn time_result() -> Type {
    Type::result(Type::date_time(), data("TimeError"))
}

/// 时间增减: `DateTime, u64 -> DateTime`.
fn time_shift() -> Type {
    fun(vec![Type::date_time(), Type::U64], Type::date_time())
}

/// 二元同类型运算的便捷构造.
fn bin(ty: Type) -> Type {
    fun(vec![ty.clone(), ty.clone()], ty)
}

/// 套接字操作结果: `Result[T, SocketError]`.
fn socket_result(ok: Type) -> Type {
    Type::result(ok, data("SocketError"))
}

/// 函数签名构造.
fn fun(params: Vec<Type>, ret: Type) -> Type {
    Type::Fn(params, Box::new(ret))
}

/// 内置 Data 类型构造.
fn data(name: &str) -> Type {
    Type::data(intern(name))
}

/// 文件操作结果: `Result[T, FileError]`.
fn file_result(ok: Type) -> Type {
    Type::result(ok, data("FileError"))
}

/// 输入操作结果: `Result[T, InputError]`.
fn input_result(ok: Type) -> Type {
    Type::result(ok, data("InputError"))
}

/// 命令执行结果: `Result[T, CommandError]`.
fn command_result(ok: Type) -> Type {
    Type::result(ok, data("CommandError"))
}

/// 数值解析结果: `Result[T, NumberParseError]`.
fn number_parse_result(ok: Type) -> Type {
    Type::result(ok, data("NumberParseError"))
}

/// UTF-8 解码结果: `Result[T, Utf8Error]`.
fn utf8_result(ok: Type) -> Type {
    Type::result(ok, data("Utf8Error"))
}

/// 正则编译结果: `Result[T, RegexError]`.
fn regex_result(ok: Type) -> Type {
    Type::result(ok, data("RegexError"))
}
