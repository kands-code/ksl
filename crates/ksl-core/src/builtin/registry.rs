//! 内置函数注册表数据.
//!
//! 注册表要逐项引用父模块的实现函数; 显式列出近百个名字会与表本身重复,
//! 故允许通配导入父模块.

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

/// 按编号取普通实现; I/O 内置返回 None.
pub(crate) fn lookup(id: BuiltinId) -> Option<BuiltinFn> {
    match TABLE.get(id.0 as usize)?.kind {
        EntryKind::Pure(function) => Some(function),
        EntryKind::Io(_) => None,
    }
}

/// 按编号取 I/O 任务构造器; 普通内置返回 None.
pub(crate) fn io_builder(id: BuiltinId) -> Option<crate::io::Builder> {
    match TABLE.get(id.0 as usize)?.kind {
        EntryKind::Io(builder) => Some(builder),
        EntryKind::Pure(_) => None,
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
        Bool as B, Char as C, DateTime as D, F64 as F, Fn as Fun, I64 as I, List as L,
        Nil as N, Str as S, U8, U64,
    };

    let io_entries: [(&'static str, Type, crate::io::Builder); 28] = [
        (
            "ReadLine",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(Type::OptionT(Box::new(S))),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::read_line_task,
        ),
        (
            "ReadFile",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(S),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::read_file_task,
        ),
        (
            "ReadFileBytes",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(L(Box::new(U8))),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::read_file_bytes_task,
        ),
        (
            "ReadKey",
            Fun(
                vec![],
                Box::new(Type::ResultT(
                    Box::new(Type::Data(intern("KeyEvent"))),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::read_key_task,
        ),
        (
            "WriteFile",
            Fun(
                vec![S, S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::write_file_task,
        ),
        (
            "WriteFileBytes",
            Fun(
                vec![S, L(Box::new(U8))],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::write_file_bytes_task,
        ),
        (
            "AppendFile",
            Fun(
                vec![S, S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::append_file_task,
        ),
        (
            "AppendFileBytes",
            Fun(
                vec![S, L(Box::new(U8))],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::append_file_bytes_task,
        ),
        (
            "ListDirectory",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(L(Box::new(S))),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::list_directory_task,
        ),
        (
            "CreateDirectory",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::create_directory_task,
        ),
        (
            "CreateDirectoryAll",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::create_directory_all_task,
        ),
        (
            "RemoveDirectory",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::remove_directory_task,
        ),
        (
            "RemoveFile",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::remove_file_task,
        ),
        (
            "RenamePath",
            Fun(
                vec![S, S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::rename_path_task,
        ),
        (
            "CopyFile",
            Fun(
                vec![S, S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
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
            Fun(
                vec![],
                Box::new(Type::ResultT(
                    Box::new(S),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::get_current_directory_task,
        ),
        (
            "OpenRead",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(Type::File),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::open_read_task,
        ),
        (
            "OpenWrite",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(Type::File),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::open_write_task,
        ),
        (
            "OpenAppend",
            Fun(
                vec![S],
                Box::new(Type::ResultT(
                    Box::new(Type::File),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::open_append_task,
        ),
        (
            "Close",
            Fun(
                vec![Type::File],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::close_task,
        ),
        (
            "Flush",
            Fun(
                vec![Type::File],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::flush_task,
        ),
        (
            "ReadLineFrom",
            Fun(
                vec![Type::File],
                Box::new(Type::ResultT(
                    Box::new(Type::OptionT(Box::new(S))),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::read_line_from_task,
        ),
        (
            "ReadChunk",
            Fun(
                vec![Type::File, U64],
                Box::new(Type::ResultT(
                    Box::new(Type::OptionT(Box::new(L(Box::new(U8))))),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::read_chunk_task,
        ),
        (
            "WriteString",
            Fun(
                vec![Type::File, S],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::write_string_task,
        ),
        (
            "WriteBytes",
            Fun(
                vec![Type::File, L(Box::new(U8))],
                Box::new(Type::ResultT(
                    Box::new(N),
                    Box::new(Type::Data(intern("IOError"))),
                )),
            ),
            crate::io::write_bytes_task,
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
        ("CharToI64", Fun(vec![C], Box::new(I)), char_to_i64 as BuiltinFn),
        ("I64ToChar", Fun(vec![I], Box::new(C)), i64_to_char as BuiltinFn),
        (
            "StringToChars",
            Fun(vec![S], Box::new(L(Box::new(C)))),
            string_to_chars as BuiltinFn,
        ),
        (
            "CharsToString",
            Fun(vec![L(Box::new(C))], Box::new(S)),
            chars_to_string as BuiltinFn,
        ),
        (
            "StringLength",
            Fun(vec![S], Box::new(U64)),
            string_length as BuiltinFn,
        ),
        (
            "StringSplit",
            Fun(vec![S, S], Box::new(L(Box::new(S)))),
            string_split as BuiltinFn,
        ),
        (
            "StringLines",
            Fun(vec![S], Box::new(L(Box::new(S)))),
            string_lines as BuiltinFn,
        ),
        (
            "StringWords",
            Fun(vec![S], Box::new(L(Box::new(S)))),
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
            Fun(vec![L(Box::new(S))], Box::new(S)),
            string_join as BuiltinFn,
        ),
        (
            "EncodeUtf8",
            Fun(vec![S], Box::new(L(Box::new(U8)))),
            encode_utf8 as BuiltinFn,
        ),
        (
            "DecodeUtf8",
            Fun(
                vec![L(Box::new(U8))],
                Box::new(Type::ResultT(Box::new(S), Box::new(S))),
            ),
            decode_utf8 as BuiltinFn,
        ),
        (
            "ParseI64",
            Fun(vec![S], Box::new(Type::ResultT(Box::new(I), Box::new(S)))),
            parse_i64 as BuiltinFn,
        ),
        (
            "ParseF64",
            Fun(vec![S], Box::new(Type::ResultT(Box::new(F), Box::new(S)))),
            parse_f64 as BuiltinFn,
        ),
        (
            "ParseU64",
            Fun(vec![S], Box::new(Type::ResultT(Box::new(U64), Box::new(S)))),
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
        ("Args", Fun(vec![], Box::new(L(Box::new(S)))), args as BuiltinFn),
        (
            "ClosedFile",
            Fun(vec![], Box::new(Type::File)),
            closed_file as BuiltinFn,
        ),
        (
            "GetEnv",
            Fun(vec![S], Box::new(Type::OptionT(Box::new(S)))),
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
        ("Now", Fun(vec![], Box::new(D)), time::now as BuiltinFn),
        ("NowUtc", Fun(vec![], Box::new(D)), time::now_utc as BuiltinFn),
        ("TimeYear", time_field(), time::time_year as BuiltinFn),
        ("TimeMonth", time_field(), time::time_month as BuiltinFn),
        ("TimeDay", time_field(), time::time_day as BuiltinFn),
        ("TimeWeekday", time_field(), time::time_weekday as BuiltinFn),
        ("TimeHour", time_field(), time::time_hour as BuiltinFn),
        ("TimeMinute", time_field(), time::time_minute as BuiltinFn),
        ("TimeSecond", time_field(), time::time_second as BuiltinFn),
        (
            "TimeMillisecond",
            time_field(),
            time::time_millisecond as BuiltinFn,
        ),
        (
            "TimeInZone",
            Fun(vec![D, S], Box::new(time_result())),
            time::time_in_zone as BuiltinFn,
        ),
        (
            "TimeZoneName",
            Fun(vec![D], Box::new(S)),
            time::time_zone_name as BuiltinFn,
        ),
        (
            "TimeZoneOffsetMinutes",
            Fun(vec![D], Box::new(I)),
            time::time_zone_offset_minutes as BuiltinFn,
        ),
        (
            "TimeFromParts",
            Fun(vec![I, I, I, I, I, I], Box::new(time_result())),
            time::time_from_parts as BuiltinFn,
        ),
        (
            "ParseDateTime",
            Fun(vec![S], Box::new(time_result())),
            time::parse_date_time as BuiltinFn,
        ),
        (
            "TimeToString",
            Fun(vec![D], Box::new(S)),
            time::time_to_string as BuiltinFn,
        ),
        (
            "DateTimeToEpochMillis",
            Fun(vec![D], Box::new(I)),
            time::date_time_to_epoch_millis as BuiltinFn,
        ),
        (
            "EpochMillisToDateTime",
            Fun(vec![I], Box::new(D)),
            time::epoch_millis_to_date_time as BuiltinFn,
        ),
        (
            "LessDateTime",
            Fun(vec![D, D], Box::new(B)),
            time::less_date_time as BuiltinFn,
        ),
        (
            "EqualDateTime",
            Fun(vec![D, D], Box::new(B)),
            time::equal_date_time as BuiltinFn,
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
        .collect()
});

/// 一元同类型函数的便捷构造.
fn un(ty: Type) -> Type {
    Type::Fn(vec![ty.clone()], Box::new(ty))
}

/// 时间字段取值: `DateTime -> i64`.
fn time_field() -> Type {
    Type::Fn(vec![Type::DateTime], Box::new(Type::I64))
}

/// 可失败的时间操作: `Result[DateTime, TimeError]`.
fn time_result() -> Type {
    Type::ResultT(
        Box::new(Type::DateTime),
        Box::new(Type::Data(intern("TimeError"))),
    )
}

/// 时间增减: `DateTime, u64 -> DateTime`.
fn time_shift() -> Type {
    Type::Fn(vec![Type::DateTime, Type::U64], Box::new(Type::DateTime))
}

/// 二元同类型运算的便捷构造.
fn bin(ty: Type) -> Type {
    Type::Fn(vec![ty.clone(), ty.clone()], Box::new(ty))
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
