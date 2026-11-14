//! 内置函数注册表数据.
//!
//! 注册表逐项引用父模块的实现函数, 使用通配导入.

#![allow(clippy::wildcard_imports)]

use std::collections::HashMap;
use std::sync::LazyLock;

use super::*;
use crate::check::Type;
use crate::symbol::{Symbol, intern};
use crate::time;

/// 内置函数编号; 降级期解析, 运行时分派不再查符号表.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuiltinId(u16);

impl BuiltinId {
    /// 内部编号.
    #[must_use]
    pub fn index(self) -> u16 {
        self.0
    }

    /// 由编号构造; 仅供注册表与测试.
    #[must_use]
    pub(crate) fn from_index(index: u16) -> Self {
        Self(index)
    }
}

pub(super) enum EntryKind {
    Pure(BuiltinFn),
    Io(crate::io::Builder),
}

/// 内置函数的可执行形态; 检查器与运行时共用同一份表.
#[derive(Clone, Copy)]
pub(crate) enum Builtin {
    Pure(BuiltinFn),
    Io(crate::io::Builder),
}

pub(super) struct Entry {
    pub(super) symbol: Symbol,
    pub(super) name: &'static str,
    signature: Type,
    kind: EntryKind,
}

/// 按符号查内置函数编号; 非内置函数返回 None.
pub(crate) fn id(symbol: Symbol) -> Option<BuiltinId> {
    INDEX.get(&symbol).copied()
}

/// 按符号查内置函数签名; 非内置函数返回 None.
///
/// 返回表内引用, 避免检查器每次解析标识符都复制一份签名.
pub(crate) fn signature(symbol: Symbol) -> Option<&'static Type> {
    INDEX.get(&symbol).map(|id| &TABLE[id.0 as usize].signature)
}

/// 全部内置函数符号; 保留名与高亮名单核对使用.
#[must_use]
pub fn names() -> Vec<Symbol> {
    TABLE.iter().map(|entry| entry.symbol).collect()
}

/// 按编号取内置函数的可执行形态; 普通实现与 I/O 构造器只查一次表.
pub(crate) fn resolve(id: BuiltinId) -> Option<Builtin> {
    match TABLE.get(id.0 as usize)?.kind {
        EntryKind::Pure(function) => Some(Builtin::Pure(function)),
        EntryKind::Io(builder) => Some(Builtin::Io(builder)),
    }
}

/// 按编号取驻留符号; 调试与测试使用.
pub(crate) fn symbol(id: BuiltinId) -> Symbol {
    TABLE[id.0 as usize].symbol
}

/// 按编号取显示名.
pub(crate) fn name(id: BuiltinId) -> &'static str {
    TABLE[id.0 as usize].name
}

pub(super) static TABLE: LazyLock<Vec<Entry>> = LazyLock::new(|| {
    use Type::{
        Bool as B, Char as C, F64 as F, Fn as Fun, I64 as I, Nil as N, Str as S, U8, U64,
    };

    let io_entries: [(&'static str, Type, crate::io::Builder); 29] = [
        (
            "ReadLine",
            Fun(
                vec![S],
                Box::new(Type::result(Type::option(S), Type::data(intern("IOError")))),
            ),
            crate::io::read_line_task,
        ),
        (
            "ReadLinePlain",
            Fun(
                vec![S],
                Box::new(Type::result(Type::option(S), Type::data(intern("IOError")))),
            ),
            crate::io::read_line_plain_task,
        ),
        (
            "ReadFile",
            Fun(
                vec![S],
                Box::new(Type::result(S, Type::data(intern("IOError")))),
            ),
            crate::io::read_file_task,
        ),
        (
            "ReadFileBytes",
            Fun(
                vec![S],
                Box::new(Type::result(Type::list(U8), Type::data(intern("IOError")))),
            ),
            crate::io::read_file_bytes_task,
        ),
        (
            "ReadKey",
            Fun(
                vec![],
                Box::new(Type::result(
                    Type::data(intern("KeyEvent")),
                    Type::data(intern("IOError")),
                )),
            ),
            crate::io::read_key_task,
        ),
        (
            "WriteFile",
            Fun(
                vec![S, S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::write_file_task,
        ),
        (
            "WriteFileBytes",
            Fun(
                vec![S, Type::list(U8)],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::write_file_bytes_task,
        ),
        (
            "AppendFile",
            Fun(
                vec![S, S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::append_file_task,
        ),
        (
            "AppendFileBytes",
            Fun(
                vec![S, Type::list(U8)],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::append_file_bytes_task,
        ),
        (
            "ListDirectory",
            Fun(
                vec![S],
                Box::new(Type::result(Type::list(S), Type::data(intern("IOError")))),
            ),
            crate::io::list_directory_task,
        ),
        (
            "CreateDirectory",
            Fun(
                vec![S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::create_directory_task,
        ),
        (
            "CreateDirectoryAll",
            Fun(
                vec![S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::create_directory_all_task,
        ),
        (
            "RemoveDirectory",
            Fun(
                vec![S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::remove_directory_task,
        ),
        (
            "RemoveFile",
            Fun(
                vec![S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::remove_file_task,
        ),
        (
            "RenamePath",
            Fun(
                vec![S, S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::rename_path_task,
        ),
        (
            "CopyFile",
            Fun(
                vec![S, S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::copy_file_task,
        ),
        (
            "IsPathExists",
            Fun(vec![S], Box::new(B)),
            crate::io::is_path_exists_task,
        ),
        ("IsFile", Fun(vec![S], Box::new(B)), crate::io::is_file_task),
        (
            "IsDirectory",
            Fun(vec![S], Box::new(B)),
            crate::io::is_directory_task,
        ),
        (
            "GetCurrentDirectory",
            Fun(vec![], Box::new(Type::result(S, Type::data(intern("IOError"))))),
            crate::io::get_current_directory_task,
        ),
        (
            "OpenRead",
            Fun(
                vec![S],
                Box::new(Type::result(Type::file(), Type::data(intern("IOError")))),
            ),
            crate::io::open_read_task,
        ),
        (
            "OpenWrite",
            Fun(
                vec![S],
                Box::new(Type::result(Type::file(), Type::data(intern("IOError")))),
            ),
            crate::io::open_write_task,
        ),
        (
            "OpenAppend",
            Fun(
                vec![S],
                Box::new(Type::result(Type::file(), Type::data(intern("IOError")))),
            ),
            crate::io::open_append_task,
        ),
        (
            "Close",
            Fun(
                vec![Type::file()],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::close_task,
        ),
        (
            "Flush",
            Fun(
                vec![Type::file()],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::flush_task,
        ),
        (
            "ReadLineFrom",
            Fun(
                vec![Type::file()],
                Box::new(Type::result(Type::option(S), Type::data(intern("IOError")))),
            ),
            crate::io::read_line_from_task,
        ),
        (
            "ReadChunk",
            Fun(
                vec![Type::file(), U64],
                Box::new(Type::result(
                    Type::option(Type::list(U8)),
                    Type::data(intern("IOError")),
                )),
            ),
            crate::io::read_chunk_task,
        ),
        (
            "WriteString",
            Fun(
                vec![Type::file(), S],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::write_string_task,
        ),
        (
            "WriteBytes",
            Fun(
                vec![Type::file(), Type::list(U8)],
                Box::new(Type::result(N, Type::data(intern("IOError")))),
            ),
            crate::io::write_bytes_task,
        ),
    ];

    let socket_entries: [(&'static str, Type, crate::io::Builder); 16] = [
        (
            "TcpListen",
            Fun(vec![S, U64], Box::new(socket_result(Type::tcp_listener()))),
            crate::io::net::tcp_listen_task,
        ),
        (
            "TcpAccept",
            Fun(
                vec![Type::tcp_listener(), U64],
                Box::new(socket_result(Type::socket())),
            ),
            crate::io::net::tcp_accept_task,
        ),
        (
            "TcpConnect",
            Fun(vec![S, U64, U64], Box::new(socket_result(Type::socket()))),
            crate::io::net::tcp_connect_task,
        ),
        (
            "TcpCloseListener",
            Fun(vec![Type::tcp_listener()], Box::new(N)),
            crate::io::net::tcp_close_listener_task,
        ),
        (
            "TcpLocalPort",
            Fun(vec![Type::tcp_listener()], Box::new(U64)),
            crate::io::net::tcp_local_port_task,
        ),
        (
            "SocketPeer",
            Fun(vec![Type::socket()], Box::new(S)),
            crate::io::net::socket_peer_task,
        ),
        (
            "SocketIsClosed",
            Fun(vec![Type::socket()], Box::new(B)),
            crate::io::net::socket_is_closed_task,
        ),
        (
            "SocketShutdownWrite",
            Fun(vec![Type::socket()], Box::new(socket_result(N))),
            crate::io::net::socket_shutdown_write_task,
        ),
        (
            "SocketRead",
            Fun(
                vec![Type::socket(), U64, U64],
                Box::new(socket_result(Type::option(Type::list(U8)))),
            ),
            crate::io::net::socket_read_task,
        ),
        (
            "SocketReadExact",
            Fun(
                vec![Type::socket(), U64, U64],
                Box::new(socket_result(Type::list(U8))),
            ),
            crate::io::net::socket_read_exact_task,
        ),
        (
            "SocketReadAll",
            Fun(
                vec![Type::socket(), U64, U64],
                Box::new(socket_result(Type::list(U8))),
            ),
            crate::io::net::socket_read_all_task,
        ),
        (
            "SocketReadLine",
            Fun(
                vec![Type::socket(), U64, U64],
                Box::new(socket_result(Type::option(S))),
            ),
            crate::io::net::socket_read_line_task,
        ),
        (
            "SocketWrite",
            Fun(
                vec![Type::socket(), Type::list(U8), U64],
                Box::new(socket_result(U64)),
            ),
            crate::io::net::socket_write_task,
        ),
        (
            "SocketWriteAll",
            Fun(
                vec![Type::socket(), Type::list(U8), U64],
                Box::new(socket_result(N)),
            ),
            crate::io::net::socket_write_all_task,
        ),
        (
            "SocketWriteLine",
            Fun(vec![Type::socket(), S, U64], Box::new(socket_result(N))),
            crate::io::net::socket_write_line_task,
        ),
        (
            "SocketClose",
            Fun(vec![Type::socket()], Box::new(N)),
            crate::io::net::socket_close_task,
        ),
    ];

    let entries = [
        ("AddI64", bin(I), add_i64 as BuiltinFn),
        ("SubI64", bin(I), subtract_i64 as BuiltinFn),
        ("MulI64", bin(I), multiply_i64 as BuiltinFn),
        ("DivideI64", bin(I), divide_i64 as BuiltinFn),
        ("ModuloI64", bin(I), modulo_i64 as BuiltinFn),
        ("RemainderI64", bin(I), remainder_i64 as BuiltinFn),
        ("QuotientI64", bin(I), quotient_i64 as BuiltinFn),
        ("AddU8", bin(U8), add_u8 as BuiltinFn),
        ("SubU8", bin(U8), subtract_u8 as BuiltinFn),
        ("MulU8", bin(U8), multiply_u8 as BuiltinFn),
        ("DivideU8", bin(U8), divide_u8 as BuiltinFn),
        ("ModuloU8", bin(U8), modulo_u8 as BuiltinFn),
        ("AddU64", bin(U64), add_u64 as BuiltinFn),
        ("SubU64", bin(U64), subtract_u64 as BuiltinFn),
        ("MulU64", bin(U64), multiply_u64 as BuiltinFn),
        ("DivideU64", bin(U64), divide_u64 as BuiltinFn),
        ("ModuloU64", bin(U64), modulo_u64 as BuiltinFn),
        ("AddF64", bin(F), add_f64 as BuiltinFn),
        ("SubF64", bin(F), subtract_f64 as BuiltinFn),
        ("MulF64", bin(F), multiply_f64 as BuiltinFn),
        ("DivideF64", bin(F), divide_f64 as BuiltinFn),
        ("AbsI64", un(I), abs_i64 as BuiltinFn),
        ("AbsF64", un(F), abs_f64 as BuiltinFn),
        ("Power", bin(F), power as BuiltinFn),
        ("Sqrt", un(F), sqrt as BuiltinFn),
        ("Sin", un(F), sin as BuiltinFn),
        ("Cos", un(F), cos as BuiltinFn),
        ("Tan", un(F), tan as BuiltinFn),
        ("ArcSin", un(F), arc_sin as BuiltinFn),
        ("ArcCos", un(F), arc_cos as BuiltinFn),
        ("ArcTan", un(F), arc_tan as BuiltinFn),
        ("ArcTan2", bin(F), arc_tan2 as BuiltinFn),
        ("Sinh", un(F), sinh as BuiltinFn),
        ("Cosh", un(F), cosh as BuiltinFn),
        ("Tanh", un(F), tanh as BuiltinFn),
        ("ArcSinh", un(F), arc_sinh as BuiltinFn),
        ("ArcCosh", un(F), arc_cosh as BuiltinFn),
        ("ArcTanh", un(F), arc_tanh as BuiltinFn),
        (
            "ShiftLeftI64",
            Fun(vec![I, U8], Box::new(I)),
            shift_left_i64 as BuiltinFn,
        ),
        (
            "ShiftRightI64",
            Fun(vec![I, U8], Box::new(I)),
            shift_right_i64 as BuiltinFn,
        ),
        ("BitOrI64", bin(I), bit_or_i64 as BuiltinFn),
        ("BitAndI64", bin(I), bit_and_i64 as BuiltinFn),
        ("BitXorI64", bin(I), bit_xor_i64 as BuiltinFn),
        (
            "BitReverseI64",
            Fun(vec![I], Box::new(I)),
            bit_reverse_i64 as BuiltinFn,
        ),
        (
            "ShiftLeftU8",
            Fun(vec![U8, U8], Box::new(U8)),
            shift_left_u8 as BuiltinFn,
        ),
        (
            "ShiftRightU8",
            Fun(vec![U8, U8], Box::new(U8)),
            shift_right_u8 as BuiltinFn,
        ),
        ("BitOrU8", bin(U8), bit_or_u8 as BuiltinFn),
        ("BitAndU8", bin(U8), bit_and_u8 as BuiltinFn),
        ("BitXorU8", bin(U8), bit_xor_u8 as BuiltinFn),
        (
            "BitReverseU8",
            Fun(vec![U8], Box::new(U8)),
            bit_reverse_u8 as BuiltinFn,
        ),
        (
            "ShiftLeftU64",
            Fun(vec![U64, U8], Box::new(U64)),
            shift_left_u64 as BuiltinFn,
        ),
        (
            "ShiftRightU64",
            Fun(vec![U64, U8], Box::new(U64)),
            shift_right_u64 as BuiltinFn,
        ),
        ("BitOrU64", bin(U64), bit_or_u64 as BuiltinFn),
        ("BitAndU64", bin(U64), bit_and_u64 as BuiltinFn),
        ("BitXorU64", bin(U64), bit_xor_u64 as BuiltinFn),
        (
            "BitReverseU64",
            Fun(vec![U64], Box::new(U64)),
            bit_reverse_u64 as BuiltinFn,
        ),
        ("I64ToF64", Fun(vec![I], Box::new(F)), i64_to_f64 as BuiltinFn),
        ("Truncate", Fun(vec![F], Box::new(I)), truncate as BuiltinFn),
        ("Round", Fun(vec![F], Box::new(I)), round as BuiltinFn),
        ("Ceiling", Fun(vec![F], Box::new(I)), ceiling as BuiltinFn),
        ("Floor", Fun(vec![F], Box::new(I)), floor as BuiltinFn),
        ("U8ToI64", Fun(vec![U8], Box::new(I)), u8_to_i64 as BuiltinFn),
        ("I64ToU8", Fun(vec![I], Box::new(U8)), i64_to_u8 as BuiltinFn),
        ("U8ToU64", Fun(vec![U8], Box::new(U64)), u8_to_u64 as BuiltinFn),
        ("U64ToU8", Fun(vec![U64], Box::new(U8)), u64_to_u8 as BuiltinFn),
        ("I64ToU64", Fun(vec![I], Box::new(U64)), i64_to_u64 as BuiltinFn),
        ("U64ToI64", Fun(vec![U64], Box::new(I)), u64_to_i64 as BuiltinFn),
        ("U64ToF64", Fun(vec![U64], Box::new(F)), u64_to_f64 as BuiltinFn),
        (
            "CharToU64",
            Fun(vec![C], Box::new(U64)),
            char_to_u64 as BuiltinFn,
        ),
        (
            "U64ToChar",
            Fun(vec![U64], Box::new(C)),
            u64_to_char as BuiltinFn,
        ),
        (
            "StringToChars",
            Fun(vec![S], Box::new(Type::list(C))),
            string_to_chars as BuiltinFn,
        ),
        (
            "CharsToString",
            Fun(vec![Type::list(C)], Box::new(S)),
            chars_to_string as BuiltinFn,
        ),
        (
            "IsWhitespace",
            Fun(vec![C], Box::new(B)),
            is_whitespace as BuiltinFn,
        ),
        (
            "IsAsciiDigit",
            Fun(vec![C], Box::new(B)),
            is_ascii_digit as BuiltinFn,
        ),
        ("IsLetter", Fun(vec![C], Box::new(B)), is_letter as BuiltinFn),
        (
            "IsUppercase",
            Fun(vec![C], Box::new(B)),
            is_uppercase as BuiltinFn,
        ),
        (
            "IsLowercase",
            Fun(vec![C], Box::new(B)),
            is_lowercase as BuiltinFn,
        ),
        (
            "IsAlphanumeric",
            Fun(vec![C], Box::new(B)),
            is_alphanumeric as BuiltinFn,
        ),
        (
            "StringLength",
            Fun(vec![S], Box::new(U64)),
            string_length as BuiltinFn,
        ),
        (
            "StringSplit",
            Fun(vec![S, S], Box::new(Type::list(S))),
            string_split as BuiltinFn,
        ),
        (
            "StringLines",
            Fun(vec![S], Box::new(Type::list(S))),
            string_lines as BuiltinFn,
        ),
        (
            "StringWords",
            Fun(vec![S], Box::new(Type::list(S))),
            string_words as BuiltinFn,
        ),
        (
            "StringTrim",
            Fun(vec![S], Box::new(S)),
            string_trim as BuiltinFn,
        ),
        (
            "StringTrimStart",
            Fun(vec![S], Box::new(S)),
            string_trim_start as BuiltinFn,
        ),
        (
            "StringTrimEnd",
            Fun(vec![S], Box::new(S)),
            string_trim_end as BuiltinFn,
        ),
        (
            "StringStartsWith",
            Fun(vec![S, S], Box::new(B)),
            string_starts_with as BuiltinFn,
        ),
        (
            "StringEndsWith",
            Fun(vec![S, S], Box::new(B)),
            string_ends_with as BuiltinFn,
        ),
        (
            "StringContains",
            Fun(vec![S, S], Box::new(B)),
            string_contains as BuiltinFn,
        ),
        (
            "StringUppercase",
            Fun(vec![S], Box::new(S)),
            string_uppercase as BuiltinFn,
        ),
        (
            "StringLowercase",
            Fun(vec![S], Box::new(S)),
            string_lowercase as BuiltinFn,
        ),
        (
            "StringJoin",
            Fun(vec![Type::list(S)], Box::new(S)),
            string_join as BuiltinFn,
        ),
        (
            "EncodeUtf8",
            Fun(vec![S], Box::new(Type::list(U8))),
            encode_utf8 as BuiltinFn,
        ),
        (
            "DecodeUtf8",
            Fun(vec![Type::list(U8)], Box::new(Type::result(S, S))),
            decode_utf8 as BuiltinFn,
        ),
        (
            "ParseI64",
            Fun(vec![S], Box::new(Type::result(I, S))),
            parse_i64 as BuiltinFn,
        ),
        (
            "ParseF64",
            Fun(vec![S], Box::new(Type::result(F, S))),
            parse_f64 as BuiltinFn,
        ),
        (
            "ParseU64",
            Fun(vec![S], Box::new(Type::result(U64, S))),
            parse_u64 as BuiltinFn,
        ),
        ("JoinPath", Fun(vec![S, S], Box::new(S)), join_path as BuiltinFn),
        ("Print", Fun(vec![S], Box::new(N)), print as BuiltinFn),
        ("PrintRaw", Fun(vec![S], Box::new(N)), print_raw as BuiltinFn),
        (
            "PrintError",
            Fun(vec![S], Box::new(N)),
            print_error as BuiltinFn,
        ),
        ("Args", Fun(vec![], Box::new(Type::list(S))), args as BuiltinFn),
        (
            "ClosedFile",
            Fun(vec![], Box::new(Type::file())),
            closed_file as BuiltinFn,
        ),
        (
            "GetEnv",
            Fun(vec![S], Box::new(Type::option(S))),
            get_env as BuiltinFn,
        ),
        ("Not", Fun(vec![B], Box::new(B)), not as BuiltinFn),
        ("And", bin(B), and as BuiltinFn),
        ("Or", bin(B), or as BuiltinFn),
        ("EqualU8", Fun(vec![U8, U8], Box::new(B)), equal_u8 as BuiltinFn),
        ("LessU8", Fun(vec![U8, U8], Box::new(B)), less_u8 as BuiltinFn),
        (
            "LessOrEqualU8",
            Fun(vec![U8, U8], Box::new(B)),
            less_or_equal_u8 as BuiltinFn,
        ),
        (
            "GreaterU8",
            Fun(vec![U8, U8], Box::new(B)),
            greater_u8 as BuiltinFn,
        ),
        (
            "GreaterOrEqualU8",
            Fun(vec![U8, U8], Box::new(B)),
            greater_or_equal_u8 as BuiltinFn,
        ),
        (
            "EqualU64",
            Fun(vec![U64, U64], Box::new(B)),
            equal_u64 as BuiltinFn,
        ),
        (
            "LessU64",
            Fun(vec![U64, U64], Box::new(B)),
            less_u64 as BuiltinFn,
        ),
        (
            "LessOrEqualU64",
            Fun(vec![U64, U64], Box::new(B)),
            less_or_equal_u64 as BuiltinFn,
        ),
        (
            "GreaterU64",
            Fun(vec![U64, U64], Box::new(B)),
            greater_u64 as BuiltinFn,
        ),
        (
            "GreaterOrEqualU64",
            Fun(vec![U64, U64], Box::new(B)),
            greater_or_equal_u64 as BuiltinFn,
        ),
        ("EqualI64", Fun(vec![I, I], Box::new(B)), equal_i64 as BuiltinFn),
        ("LessI64", Fun(vec![I, I], Box::new(B)), less_i64 as BuiltinFn),
        (
            "LessOrEqualI64",
            Fun(vec![I, I], Box::new(B)),
            less_or_equal_i64 as BuiltinFn,
        ),
        (
            "GreaterI64",
            Fun(vec![I, I], Box::new(B)),
            greater_i64 as BuiltinFn,
        ),
        (
            "GreaterOrEqualI64",
            Fun(vec![I, I], Box::new(B)),
            greater_or_equal_i64 as BuiltinFn,
        ),
        ("EqualF64", Fun(vec![F, F], Box::new(B)), equal_f64 as BuiltinFn),
        ("LessF64", Fun(vec![F, F], Box::new(B)), less_f64 as BuiltinFn),
        (
            "LessOrEqualF64",
            Fun(vec![F, F], Box::new(B)),
            less_or_equal_f64 as BuiltinFn,
        ),
        (
            "GreaterF64",
            Fun(vec![F, F], Box::new(B)),
            greater_f64 as BuiltinFn,
        ),
        (
            "GreaterOrEqualF64",
            Fun(vec![F, F], Box::new(B)),
            greater_or_equal_f64 as BuiltinFn,
        ),
        (
            "EqualChar",
            Fun(vec![C, C], Box::new(B)),
            equal_char as BuiltinFn,
        ),
        (
            "EqualString",
            Fun(vec![S, S], Box::new(B)),
            equal_string as BuiltinFn,
        ),
        ("EqualBool", bin(B), equal_bool as BuiltinFn),
        // 时间与日期.
        (
            "Now",
            Fun(vec![], Box::new(Type::date_time())),
            time::now as BuiltinFn,
        ),
        (
            "NowUtc",
            Fun(vec![], Box::new(Type::date_time())),
            time::now_utc as BuiltinFn,
        ),
        ("DateYear", time_field(), time::date_year as BuiltinFn),
        ("DateMonth", time_field(), time::date_month as BuiltinFn),
        ("DateDay", time_field(), time::date_day as BuiltinFn),
        ("DateWeekday", time_field(), time::date_weekday as BuiltinFn),
        ("TimeHour", time_field(), time::time_hour as BuiltinFn),
        ("TimeMinute", time_field(), time::time_minute as BuiltinFn),
        ("TimeSecond", time_field(), time::time_second as BuiltinFn),
        (
            "TimeMillisecond",
            time_field(),
            time::time_millisecond as BuiltinFn,
        ),
        (
            "DateTimeInZone",
            Fun(vec![Type::date_time(), S], Box::new(time_result())),
            time::date_time_in_zone as BuiltinFn,
        ),
        (
            "TimeZoneName",
            Fun(vec![Type::date_time()], Box::new(S)),
            time::time_zone_name as BuiltinFn,
        ),
        (
            "TimeZoneOffsetMinutes",
            Fun(vec![Type::date_time()], Box::new(I)),
            time::time_zone_offset_minutes as BuiltinFn,
        ),
        (
            "DateTimeFromParts",
            Fun(vec![I, I, I, I, I, I], Box::new(time_result())),
            time::date_time_from_parts as BuiltinFn,
        ),
        (
            "ParseDateTime",
            Fun(vec![S], Box::new(time_result())),
            time::parse_date_time as BuiltinFn,
        ),
        (
            "DateTimeToString",
            Fun(vec![Type::date_time()], Box::new(S)),
            time::date_time_to_string as BuiltinFn,
        ),
        (
            "DateTimeToEpochMillis",
            Fun(vec![Type::date_time()], Box::new(I)),
            time::date_time_to_epoch_millis as BuiltinFn,
        ),
        (
            "EpochMillisToDateTime",
            Fun(vec![I], Box::new(Type::date_time())),
            time::epoch_millis_to_date_time as BuiltinFn,
        ),
        (
            "LessDateTime",
            Fun(vec![Type::date_time(), Type::date_time()], Box::new(B)),
            time::less_date_time as BuiltinFn,
        ),
        (
            "LessOrEqualDateTime",
            Fun(vec![Type::date_time(), Type::date_time()], Box::new(B)),
            time::less_or_equal_date_time as BuiltinFn,
        ),
        (
            "EqualDateTime",
            Fun(vec![Type::date_time(), Type::date_time()], Box::new(B)),
            time::equal_date_time as BuiltinFn,
        ),
        (
            "GreaterDateTime",
            Fun(vec![Type::date_time(), Type::date_time()], Box::new(B)),
            time::greater_date_time as BuiltinFn,
        ),
        (
            "GreaterOrEqualDateTime",
            Fun(vec![Type::date_time(), Type::date_time()], Box::new(B)),
            time::greater_or_equal_date_time as BuiltinFn,
        ),
        ("AddYear", time_shift(), time::add_year as BuiltinFn),
        ("SubYear", time_shift(), time::sub_year as BuiltinFn),
        ("AddMonth", time_shift(), time::add_month as BuiltinFn),
        ("SubMonth", time_shift(), time::sub_month as BuiltinFn),
        ("AddWeek", time_shift(), time::add_week as BuiltinFn),
        ("SubWeek", time_shift(), time::sub_week as BuiltinFn),
        ("AddDay", time_shift(), time::add_day as BuiltinFn),
        ("SubDay", time_shift(), time::sub_day as BuiltinFn),
        ("AddHour", time_shift(), time::add_hour as BuiltinFn),
        ("SubHour", time_shift(), time::sub_hour as BuiltinFn),
        ("AddMinute", time_shift(), time::add_minute as BuiltinFn),
        ("SubMinute", time_shift(), time::sub_minute as BuiltinFn),
        ("AddSecond", time_shift(), time::add_second as BuiltinFn),
        ("SubSecond", time_shift(), time::sub_second as BuiltinFn),
        (
            "AddMillisecond",
            time_shift(),
            time::add_millisecond as BuiltinFn,
        ),
        (
            "SubMillisecond",
            time_shift(),
            time::sub_millisecond as BuiltinFn,
        ),
    ];
    entries
        .into_iter()
        .map(|(name, signature, function)| Entry {
            symbol: intern(name),
            name,
            signature,
            kind: EntryKind::Pure(function),
        })
        .chain(io_entries.into_iter().map(|(name, signature, builder)| Entry {
            symbol: intern(name),
            name,
            signature,
            kind: EntryKind::Io(builder),
        }))
        .chain(
            socket_entries
                .into_iter()
                .map(|(name, signature, builder)| Entry {
                    symbol: intern(name),
                    name,
                    signature,
                    kind: EntryKind::Io(builder),
                }),
        )
        .collect()
});

/// 一元同类型函数的便捷构造.
fn un(ty: Type) -> Type {
    Type::Fn(vec![ty.clone()], Box::new(ty))
}

/// 时间字段取值: `DateTime -> i64`.
fn time_field() -> Type {
    Type::Fn(vec![Type::date_time()], Box::new(Type::I64))
}

/// 可失败的时间操作: `Result[DateTime, TimeError]`.
fn time_result() -> Type {
    Type::result(Type::date_time(), Type::data(intern("TimeError")))
}

/// 时间增减: `DateTime, u64 -> DateTime`.
fn time_shift() -> Type {
    Type::Fn(vec![Type::date_time(), Type::U64], Box::new(Type::date_time()))
}

/// 二元同类型运算的便捷构造.
fn bin(ty: Type) -> Type {
    Type::Fn(vec![ty.clone(), ty.clone()], Box::new(ty))
}

/// 套接字操作结果: `Result[T, SocketError]`.
fn socket_result(ok: Type) -> Type {
    Type::result(ok, Type::data(intern("SocketError")))
}

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
