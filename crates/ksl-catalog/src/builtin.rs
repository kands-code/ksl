//! 内置函数元数据.
//!
//! 名字, 签名与分类集中在本表; 实现分派位于运行时.

use std::collections::HashMap;
use std::sync::LazyLock;

use ksl_base::name::Name;
use ksl_base::types::Type;

/// 内置函数的调用形态.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinKind {
    /// 编译期常量: 由检查器按调用点展开, 不进入运行时.
    Constant,
    /// 普通函数, 在抽象机内直接返回, 无副作用.
    Pure,
    /// 同步但有副作用, 如写出标准输出; 降级时保留续延.
    Effect,
    /// I/O 任务构造器, 提交任务后挂起.
    Io,
}

/// 内置函数元数据: 名字, 签名与调用形态.
pub struct BuiltinSpec {
    pub name: &'static str,
    pub signature: Type,
    pub kind: BuiltinKind,
}

/// 由一份名字列表生成内置函数枚举与元数据表.
///
/// 枚举变体, 元数据表与 `BuiltinId::ALL` 来自同一份输入, 三者顺序一致.
/// 分组决定调用形态.
macro_rules! define_builtins {
    (
        pure { $($pure:ident : $pure_sig:expr),* $(,)? }
        effect { $($effect:ident : $effect_sig:expr),* $(,)? }
        io { $($io:ident : $io_sig:expr),* $(,)? }
        socket { $($socket:ident : $socket_sig:expr),* $(,)? }
        constant { $($constant:ident : $constant_sig:expr),* $(,)? }
    ) => {
        /// 内置函数编号; 降级期解析, 运行时按编号分派.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum BuiltinId {
            $($pure,)*
            $($effect,)*
            $($io,)*
            $($socket,)*
            $($constant,)*
        }

        impl BuiltinId {
            /// 全部内置函数, 顺序与元数据表一致.
            pub const ALL: &'static [Self] = &[
                $(Self::$pure,)*
                $(Self::$effect,)*
                $(Self::$io,)*
                $(Self::$socket,)*
                $(Self::$constant,)*
            ];

            /// 元数据表下标.
            #[must_use]
            pub const fn index(self) -> usize {
                self as usize
            }

            /// 源码名.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$pure => stringify!($pure),)*
                    $(Self::$effect => stringify!($effect),)*
                    $(Self::$io => stringify!($io),)*
                    $(Self::$socket => stringify!($socket),)*
                    $(Self::$constant => stringify!($constant),)*
                }
            }
        }

        static TABLE: LazyLock<Vec<BuiltinSpec>> = LazyLock::new(|| {
            vec![
                $(BuiltinSpec {
                    name: stringify!($pure),
                    signature: $pure_sig,
                    kind: BuiltinKind::Pure,
                },)*
                $(BuiltinSpec {
                    name: stringify!($effect),
                    signature: $effect_sig,
                    kind: BuiltinKind::Effect,
                },)*
                $(BuiltinSpec {
                    name: stringify!($io),
                    signature: $io_sig,
                    kind: BuiltinKind::Io,
                },)*
                $(BuiltinSpec {
                    name: stringify!($socket),
                    signature: $socket_sig,
                    kind: BuiltinKind::Io,
                },)*
                $(BuiltinSpec {
                    name: stringify!($constant),
                    signature: $constant_sig,
                    kind: BuiltinKind::Constant,
                },)*
            ]
        });
    };
}

use Type::{
    Bool as B, Char as C, F32 as G, F64 as F, I32, I64 as I, Nil as N, Str as S, U8, U32, U64,
};

define_builtins! {
    pure {
        AddI64: bin(I),
        SubI64: bin(I),
        NegI64: un(I),
        MulI64: bin(I),
        DivideI64: bin(I),
        ModuloI64: bin(I),
        RemainderI64: bin(I),
        QuotientI64: bin(I),
        AddI32: bin(I32),
        SubI32: bin(I32),
        NegI32: un(I32),
        MulI32: bin(I32),
        DivideI32: bin(I32),
        ModuloI32: bin(I32),
        RemainderI32: bin(I32),
        QuotientI32: bin(I32),
        AddU8: bin(U8),
        SubU8: bin(U8),
        MulU8: bin(U8),
        DivideU8: bin(U8),
        ModuloU8: bin(U8),
        AddU32: bin(U32),
        SubU32: bin(U32),
        MulU32: bin(U32),
        DivideU32: bin(U32),
        ModuloU32: bin(U32),
        AddU64: bin(U64),
        SubU64: bin(U64),
        MulU64: bin(U64),
        DivideU64: bin(U64),
        ModuloU64: bin(U64),
        AddF64: bin(F),
        SubF64: bin(F),
        NegF64: un(F),
        MulF64: bin(F),
        DivideF64: bin(F),
        AddF32: bin(G),
        SubF32: bin(G),
        NegF32: un(G),
        MulF32: bin(G),
        DivideF32: bin(G),
        AbsI64: un(I),
        AbsI32: un(I32),
        AbsF64: un(F),
        AbsF32: un(G),
        PowerF64: bin(F),
        PowerF32: bin(G),
        SqrtF64: un(F),
        SqrtF32: un(G),
        SinF64: un(F),
        SinF32: un(G),
        CosF64: un(F),
        CosF32: un(G),
        TanF64: un(F),
        TanF32: un(G),
        ArcSinF64: un(F),
        ArcSinF32: un(G),
        ArcCosF64: un(F),
        ArcCosF32: un(G),
        ArcTanF64: un(F),
        ArcTanF32: un(G),
        ArcTan2F64: bin(F),
        ArcTan2F32: bin(G),
        SinhF64: un(F),
        SinhF32: un(G),
        CoshF64: un(F),
        CoshF32: un(G),
        TanhF64: un(F),
        TanhF32: un(G),
        ArcSinhF64: un(F),
        ArcSinhF32: un(G),
        ArcCoshF64: un(F),
        ArcCoshF32: un(G),
        ArcTanhF64: un(F),
        ArcTanhF32: un(G),
        ShiftLeftI64: fun(vec![I, U8], I),
        ShiftRightI64: fun(vec![I, U8], I),
        BitOrI64: bin(I),
        BitAndI64: bin(I),
        BitXorI64: bin(I),
        BitReverseI64: fun(vec![I], I),
        ShiftLeftI32: fun(vec![I32, U8], I32),
        ShiftRightI32: fun(vec![I32, U8], I32),
        BitOrI32: bin(I32),
        BitAndI32: bin(I32),
        BitXorI32: bin(I32),
        BitReverseI32: fun(vec![I32], I32),
        ShiftLeftU8: fun(vec![U8, U8], U8),
        ShiftRightU8: fun(vec![U8, U8], U8),
        BitOrU8: bin(U8),
        BitAndU8: bin(U8),
        BitXorU8: bin(U8),
        BitReverseU8: fun(vec![U8], U8),
        ShiftLeftU32: fun(vec![U32, U8], U32),
        ShiftRightU32: fun(vec![U32, U8], U32),
        BitOrU32: bin(U32),
        BitAndU32: bin(U32),
        BitXorU32: bin(U32),
        BitReverseU32: fun(vec![U32], U32),
        ShiftLeftU64: fun(vec![U64, U8], U64),
        ShiftRightU64: fun(vec![U64, U8], U64),
        BitOrU64: bin(U64),
        BitAndU64: bin(U64),
        BitXorU64: bin(U64),
        BitReverseU64: fun(vec![U64], U64),
        I64ToF64: fun(vec![I], F),
        I64ToF32: fun(vec![I], G),
        I32ToF64: fun(vec![I32], F),
        I32ToF32: fun(vec![I32], G),
        TruncateF64: fun(vec![F], I),
        TruncateF32: fun(vec![G], I),
        RoundF64: fun(vec![F], I),
        RoundF32: fun(vec![G], I),
        CeilingF64: fun(vec![F], I),
        CeilingF32: fun(vec![G], I),
        FloorF64: fun(vec![F], I),
        FloorF32: fun(vec![G], I),
        F32ToF64: fun(vec![G], F),
        F64ToF32: fun(vec![F], G),
        U8ToI64: fun(vec![U8], I),
        I64ToU8: fun(vec![I], U8),
        I32ToI64: fun(vec![I32], I),
        I64ToI32: fun(vec![I], I32),
        I32ToU64: fun(vec![I32], U64),
        U64ToI32: fun(vec![U64], I32),
        U8ToI32: fun(vec![U8], I32),
        I32ToU8: fun(vec![I32], U8),
        U8ToU64: fun(vec![U8], U64),
        U64ToU8: fun(vec![U64], U8),
        U32ToI64: fun(vec![U32], I),
        I64ToU32: fun(vec![I], U32),
        U32ToI32: fun(vec![U32], I32),
        I32ToU32: fun(vec![I32], U32),
        U32ToU64: fun(vec![U32], U64),
        U64ToU32: fun(vec![U64], U32),
        U32ToU8: fun(vec![U32], U8),
        U8ToU32: fun(vec![U8], U32),
        U32ToF64: fun(vec![U32], F),
        U32ToF32: fun(vec![U32], G),
        CharToU32: fun(vec![C], U32),
        U32ToChar: fun(vec![U32], C),
        I64ToU64: fun(vec![I], U64),
        U64ToI64: fun(vec![U64], I),
        U64ToF64: fun(vec![U64], F),
        U64ToF32: fun(vec![U64], G),
        CharToU64: fun(vec![C], U64),
        U64ToChar: fun(vec![U64], C),
        StringToChars: fun(vec![S], Type::list(C)),
        CharsToString: fun(vec![Type::list(C)], S),
        IsWhitespace: fun(vec![C], B),
        IsAsciiDigit: fun(vec![C], B),
        IsLetter: fun(vec![C], B),
        IsUppercase: fun(vec![C], B),
        IsLowercase: fun(vec![C], B),
        IsAlphanumeric: fun(vec![C], B),
        StringLength: fun(vec![S], U64),
        StringSplit: fun(vec![S, S], Type::list(S)),
        StringLines: fun(vec![S], Type::list(S)),
        StringWords: fun(vec![S], Type::list(S)),
        StringTrim: fun(vec![S], S),
        StringTrimStart: fun(vec![S], S),
        StringTrimEnd: fun(vec![S], S),
        StringStartsWith: fun(vec![S, S], B),
        StringEndsWith: fun(vec![S, S], B),
        StringContains: fun(vec![S, S], B),
        StringIndexOf: fun(vec![S, S], Type::option(U64)),
        StringUppercase: fun(vec![S], S),
        StringLowercase: fun(vec![S], S),
        StringJoin: fun(vec![Type::list(S)], S),
        StringReplace: fun(vec![S, S, S], S),
        StringReplaceAll: fun(vec![S, S, S], S),
        StringSlice: fun(vec![S, U64, U64], S),
        EncodeUtf8: fun(vec![S], Type::list(U8)),
        DecodeUtf8: fun(vec![Type::list(U8)], utf8_result(S)),
        ParseI64: fun(vec![S], number_parse_result(I)),
        ParseI32: fun(vec![S], number_parse_result(I32)),
        ParseF32: fun(vec![S], number_parse_result(G)),
        ParseF64: fun(vec![S], number_parse_result(F)),
        ParseU32: fun(vec![S], number_parse_result(U32)),
        ParseU64: fun(vec![S], number_parse_result(U64)),
        JoinPath: fun(vec![S, S], S),
        Args: fun(vec![], Type::list(S)),
        ClosedFile: fun(vec![], Type::file()),
        GetEnv: fun(vec![S], Type::option(S)),
        Not: fun(vec![B], B),
        And: bin(B),
        Or: bin(B),
        EqualU8: fun(vec![U8, U8], B),
        LessU8: fun(vec![U8, U8], B),
        LessOrEqualU8: fun(vec![U8, U8], B),
        GreaterU8: fun(vec![U8, U8], B),
        GreaterOrEqualU8: fun(vec![U8, U8], B),
        EqualU32: fun(vec![U32, U32], B),
        LessU32: fun(vec![U32, U32], B),
        LessOrEqualU32: fun(vec![U32, U32], B),
        GreaterU32: fun(vec![U32, U32], B),
        GreaterOrEqualU32: fun(vec![U32, U32], B),
        EqualU64: fun(vec![U64, U64], B),
        LessU64: fun(vec![U64, U64], B),
        LessOrEqualU64: fun(vec![U64, U64], B),
        GreaterU64: fun(vec![U64, U64], B),
        GreaterOrEqualU64: fun(vec![U64, U64], B),
        EqualI64: fun(vec![I, I], B),
        LessI64: fun(vec![I, I], B),
        LessOrEqualI64: fun(vec![I, I], B),
        GreaterI64: fun(vec![I, I], B),
        GreaterOrEqualI64: fun(vec![I, I], B),
        EqualI32: fun(vec![I32, I32], B),
        LessI32: fun(vec![I32, I32], B),
        LessOrEqualI32: fun(vec![I32, I32], B),
        GreaterI32: fun(vec![I32, I32], B),
        GreaterOrEqualI32: fun(vec![I32, I32], B),
        EqualF64: fun(vec![F, F], B),
        LessF64: fun(vec![F, F], B),
        LessOrEqualF64: fun(vec![F, F], B),
        GreaterF64: fun(vec![F, F], B),
        GreaterOrEqualF64: fun(vec![F, F], B),
        EqualF32: fun(vec![G, G], B),
        LessF32: fun(vec![G, G], B),
        LessOrEqualF32: fun(vec![G, G], B),
        GreaterF32: fun(vec![G, G], B),
        GreaterOrEqualF32: fun(vec![G, G], B),
        EqualChar: fun(vec![C, C], B),
        EqualString: fun(vec![S, S], B),
        EqualBool: bin(B),
        Now: fun(vec![], Type::date_time()),
        NowUtc: fun(vec![], Type::date_time()),
        DateYear: time_field(),
        DateMonth: time_field(),
        DateDay: time_field(),
        DateWeekday: time_field(),
        TimeHour: time_field(),
        TimeMinute: time_field(),
        TimeSecond: time_field(),
        TimeMillisecond: time_field(),
        DateTimeInZone: fun(vec![Type::date_time(), S], time_result()),
        TimeZoneName: fun(vec![Type::date_time()], S),
        TimeZoneOffsetMinutes: fun(vec![Type::date_time()], I),
        DateTimeFromParts: fun(vec![I, I, I, I, I, I], time_result()),
        ParseDateTime: fun(vec![S], time_result()),
        DateTimeToString: fun(vec![Type::date_time()], S),
        DateTimeToEpochMillis: fun(vec![Type::date_time()], I),
        EpochMillisToDateTime: fun(vec![I], Type::date_time()),
        LessDateTime: fun(vec![Type::date_time(), Type::date_time()], B),
        LessOrEqualDateTime: fun(vec![Type::date_time(), Type::date_time()], B),
        EqualDateTime: fun(vec![Type::date_time(), Type::date_time()], B),
        GreaterDateTime: fun(vec![Type::date_time(), Type::date_time()], B),
        GreaterOrEqualDateTime: fun(vec![Type::date_time(), Type::date_time()], B),
        AddYear: time_shift(),
        SubYear: time_shift(),
        AddMonth: time_shift(),
        SubMonth: time_shift(),
        AddWeek: time_shift(),
        SubWeek: time_shift(),
        AddDay: time_shift(),
        SubDay: time_shift(),
        AddHour: time_shift(),
        SubHour: time_shift(),
        AddMinute: time_shift(),
        SubMinute: time_shift(),
        AddSecond: time_shift(),
        SubSecond: time_shift(),
        AddMillisecond: time_shift(),
        SubMillisecond: time_shift(),
        RegexCompile: fun(vec![S], regex_result(Type::regex())),
        RegexIsMatch: fun(vec![Type::regex(), S], B),
        RegexFind: fun(vec![Type::regex(), S], Type::option(Type::regex_match())),
        RegexFindAll: fun(vec![Type::regex(), S], Type::list(Type::regex_match())),
        RegexReplace: fun(vec![Type::regex(), S, S], S),
        RegexReplaceAll: fun(vec![Type::regex(), S, S], S),
        RegexSplit: fun(vec![Type::regex(), S], Type::list(S)),
        RegexCaptureNames: fun(vec![Type::regex()], Type::list(S)),
        EmptyRegexMatch: fun(vec![], Type::regex_match()),
        RegexMatchText: fun(vec![Type::regex_match()], S),
        RegexMatchStart: fun(vec![Type::regex_match()], U64),
        RegexMatchEnd: fun(vec![Type::regex_match()], U64),
        RegexMatchGroup: fun(vec![Type::regex_match(), U64], Type::option(S)),
        RegexMatchNamedGroup: fun(vec![Type::regex_match(), S], Type::option(S)),
        CommandOutputStatus: fun(vec![Type::command_output()], data("CommandStatus")),
        CommandOutputStdout: fun(vec![Type::command_output()], Type::list(U8)),
        CommandOutputStderr: fun(vec![Type::command_output()], Type::list(U8)),
    }
    effect {
        Print: fun(vec![S], N),
        PrintError: fun(vec![S], N),
    }
    io {
        ReadLine: fun(vec![S], input_result(Type::option(S))),
        ReadFile: fun(vec![S], file_result(S)),
        ReadFileBytes: fun(vec![S], file_result(Type::list(U8))),
        ReadKey: fun(vec![], input_result(data("KeyEvent"))),
        EnterRawMode: fun(vec![], input_result(N)),
        ExitRawMode: fun(vec![], input_result(N)),
        WriteFile: fun(vec![S, S], file_result(N)),
        WriteFileBytes: fun(vec![S, Type::list(U8)], file_result(N)),
        AppendFile: fun(vec![S, S], file_result(N)),
        AppendFileBytes: fun(vec![S, Type::list(U8)], file_result(N)),
        ListDirectory: fun(vec![S], file_result(Type::list(S))),
        CreateDirectory: fun(vec![S], file_result(N)),
        CreateDirectoryAll: fun(vec![S], file_result(N)),
        RemoveDirectory: fun(vec![S], file_result(N)),
        RemoveFile: fun(vec![S], file_result(N)),
        RenamePath: fun(vec![S, S], file_result(N)),
        CopyFile: fun(vec![S, S], file_result(N)),
        IsPathExists: fun(vec![S], B),
        IsFile: fun(vec![S], B),
        IsDirectory: fun(vec![S], B),
        GetCurrentDirectory: fun(vec![], file_result(S)),
        OpenRead: fun(vec![S], file_result(Type::file())),
        OpenWrite: fun(vec![S], file_result(Type::file())),
        OpenAppend: fun(vec![S], file_result(Type::file())),
        Close: fun(vec![Type::file()], file_result(N)),
        Flush: fun(vec![Type::file()], file_result(N)),
        ReadLineFrom: fun(vec![Type::file()], file_result(Type::option(S))),
        ReadChunk: fun(
                vec![Type::file(), U64],
                file_result(Type::option(Type::list(U8))),
            ),
        WriteString: fun(vec![Type::file(), S], file_result(N)),
        WriteBytes: fun(vec![Type::file(), Type::list(U8)], file_result(N)),
        RunCommand: fun(
                vec![S, Type::list(S), Type::list(data("CommandOption"))],
                command_result(Type::command_output()),
            ),
    }
    socket {
        TcpListen: fun(vec![S, U64], socket_result(Type::tcp_listener())),
        TcpAccept: fun(vec![Type::tcp_listener(), U64], socket_result(Type::socket())),
        TcpConnect: fun(vec![S, U64, U64], socket_result(Type::socket())),
        TcpCloseListener: fun(vec![Type::tcp_listener()], N),
        TcpLocalPort: fun(vec![Type::tcp_listener()], U64),
        SocketPeer: fun(vec![Type::socket()], S),
        SocketIsClosed: fun(vec![Type::socket()], B),
        SocketShutdownWrite: fun(vec![Type::socket(), U64], socket_result(N)),
        SocketRead: fun(
                vec![Type::socket(), U64, U64],
                socket_result(Type::option(Type::list(U8))),
            ),
        SocketReadExact: fun(vec![Type::socket(), U64, U64], socket_result(Type::list(U8))),
        SocketReadAll: fun(vec![Type::socket(), U64, U64], socket_result(Type::list(U8))),
        SocketReadLine: fun(vec![Type::socket(), U64, U64], socket_result(Type::option(S))),
        SocketWrite: fun(vec![Type::socket(), Type::list(U8), U64], socket_result(U64)),
        SocketWriteAll: fun(vec![Type::socket(), Type::list(U8), U64], socket_result(N)),
        SocketWriteLine: fun(vec![Type::socket(), S, U64], socket_result(N)),
        SocketClose: fun(vec![Type::socket()], N),
    }
    constant {
        GetModuleDirectory: fun(vec![], S),
    }
}

static INDEX: LazyLock<HashMap<Name, BuiltinId>> = LazyLock::new(|| {
    BuiltinId::ALL
        .iter()
        .map(|id| (Name::new(id.name()), *id))
        .collect()
});

/// 按名字查内置函数编号; 非内置函数返回 None.
#[must_use]
pub fn id(symbol: &Name) -> Option<BuiltinId> {
    INDEX.get(symbol).copied()
}

/// 内置函数是否可原地调用且无副作用.
///
/// 降级器只把该类别直接降为同步求值; `Effect` 与 `Io` 都保留续延.
#[must_use]
pub fn is_pure(id: BuiltinId) -> bool {
    TABLE[id.index()].kind == BuiltinKind::Pure
}

/// 按名字查函数签名; 非内置函数返回 None.
#[must_use]
pub fn signature(symbol: &Name) -> Option<&'static Type> {
    INDEX.get(symbol).map(|id| &TABLE[id.index()].signature)
}

/// 全部内置函数元数据, 顺序即编号.
pub fn specs() -> impl Iterator<Item = &'static BuiltinSpec> {
    TABLE.iter()
}

#[must_use]
pub fn names() -> Vec<Name> {
    TABLE.iter().map(|entry| Name::new(entry.name)).collect()
}

#[must_use]
pub fn symbol(id: BuiltinId) -> Name {
    Name::new(id.name())
}

#[must_use]
pub fn name(id: BuiltinId) -> &'static str {
    id.name()
}

#[must_use]
pub fn len() -> usize {
    BuiltinId::ALL.len()
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
    Type::function(params, ret)
}

/// 内置 Data 类型构造.
fn data(name: &str) -> Type {
    Type::data(Name::new(name))
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
