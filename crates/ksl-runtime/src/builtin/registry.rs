//! 内置函数实现分派.
//!
//! 按 `BuiltinId` 编号取实现, 顺序必须与 `ksl-catalog` 的签名表一致.

use std::sync::LazyLock;

pub use ksl_catalog::builtin::{BuiltinId, id, name, names, signature, symbol};

use super::*;
use crate::time;

/// 内置函数的可执行形态.
#[derive(Clone, Copy)]
pub(crate) enum Builtin {
    Pure(BuiltinFn),
    Io(crate::io::Builder),
    /// 标准输出或标准错误; 由运行时写出服务执行.
    Print {
        error: bool,
    },
}

static IMPLS: LazyLock<Vec<(&'static str, Builtin)>> = LazyLock::new(|| {
    let pure_impls: [(&'static str, Builtin); 185] = [
        ("AddI64", Builtin::Pure(add_i64)),
        ("SubI64", Builtin::Pure(subtract_i64)),
        ("MulI64", Builtin::Pure(multiply_i64)),
        ("DivideI64", Builtin::Pure(divide_i64)),
        ("ModuloI64", Builtin::Pure(modulo_i64)),
        ("RemainderI64", Builtin::Pure(remainder_i64)),
        ("QuotientI64", Builtin::Pure(quotient_i64)),
        ("AddU8", Builtin::Pure(add_u8)),
        ("SubU8", Builtin::Pure(subtract_u8)),
        ("MulU8", Builtin::Pure(multiply_u8)),
        ("DivideU8", Builtin::Pure(divide_u8)),
        ("ModuloU8", Builtin::Pure(modulo_u8)),
        ("AddU64", Builtin::Pure(add_u64)),
        ("SubU64", Builtin::Pure(subtract_u64)),
        ("MulU64", Builtin::Pure(multiply_u64)),
        ("DivideU64", Builtin::Pure(divide_u64)),
        ("ModuloU64", Builtin::Pure(modulo_u64)),
        ("AddF64", Builtin::Pure(add_f64)),
        ("SubF64", Builtin::Pure(subtract_f64)),
        ("MulF64", Builtin::Pure(multiply_f64)),
        ("DivideF64", Builtin::Pure(divide_f64)),
        ("AbsI64", Builtin::Pure(abs_i64)),
        ("AbsF64", Builtin::Pure(abs_f64)),
        ("Power", Builtin::Pure(power)),
        ("Sqrt", Builtin::Pure(sqrt)),
        ("Sin", Builtin::Pure(sin)),
        ("Cos", Builtin::Pure(cos)),
        ("Tan", Builtin::Pure(tan)),
        ("ArcSin", Builtin::Pure(arc_sin)),
        ("ArcCos", Builtin::Pure(arc_cos)),
        ("ArcTan", Builtin::Pure(arc_tan)),
        ("ArcTan2", Builtin::Pure(arc_tan2)),
        ("Sinh", Builtin::Pure(sinh)),
        ("Cosh", Builtin::Pure(cosh)),
        ("Tanh", Builtin::Pure(tanh)),
        ("ArcSinh", Builtin::Pure(arc_sinh)),
        ("ArcCosh", Builtin::Pure(arc_cosh)),
        ("ArcTanh", Builtin::Pure(arc_tanh)),
        ("ShiftLeftI64", Builtin::Pure(shift_left_i64)),
        ("ShiftRightI64", Builtin::Pure(shift_right_i64)),
        ("BitOrI64", Builtin::Pure(bit_or_i64)),
        ("BitAndI64", Builtin::Pure(bit_and_i64)),
        ("BitXorI64", Builtin::Pure(bit_xor_i64)),
        ("BitReverseI64", Builtin::Pure(bit_reverse_i64)),
        ("ShiftLeftU8", Builtin::Pure(shift_left_u8)),
        ("ShiftRightU8", Builtin::Pure(shift_right_u8)),
        ("BitOrU8", Builtin::Pure(bit_or_u8)),
        ("BitAndU8", Builtin::Pure(bit_and_u8)),
        ("BitXorU8", Builtin::Pure(bit_xor_u8)),
        ("BitReverseU8", Builtin::Pure(bit_reverse_u8)),
        ("ShiftLeftU64", Builtin::Pure(shift_left_u64)),
        ("ShiftRightU64", Builtin::Pure(shift_right_u64)),
        ("BitOrU64", Builtin::Pure(bit_or_u64)),
        ("BitAndU64", Builtin::Pure(bit_and_u64)),
        ("BitXorU64", Builtin::Pure(bit_xor_u64)),
        ("BitReverseU64", Builtin::Pure(bit_reverse_u64)),
        ("I64ToF64", Builtin::Pure(i64_to_f64)),
        ("Truncate", Builtin::Pure(truncate)),
        ("Round", Builtin::Pure(round)),
        ("Ceiling", Builtin::Pure(ceiling)),
        ("Floor", Builtin::Pure(floor)),
        ("U8ToI64", Builtin::Pure(u8_to_i64)),
        ("I64ToU8", Builtin::Pure(i64_to_u8)),
        ("U8ToU64", Builtin::Pure(u8_to_u64)),
        ("U64ToU8", Builtin::Pure(u64_to_u8)),
        ("I64ToU64", Builtin::Pure(i64_to_u64)),
        ("U64ToI64", Builtin::Pure(u64_to_i64)),
        ("U64ToF64", Builtin::Pure(u64_to_f64)),
        ("CharToU64", Builtin::Pure(char_to_u64)),
        ("U64ToChar", Builtin::Pure(u64_to_char)),
        ("StringToChars", Builtin::Pure(string_to_chars)),
        ("CharsToString", Builtin::Pure(chars_to_string)),
        ("IsWhitespace", Builtin::Pure(is_whitespace)),
        ("IsAsciiDigit", Builtin::Pure(is_ascii_digit)),
        ("IsLetter", Builtin::Pure(is_letter)),
        ("IsUppercase", Builtin::Pure(is_uppercase)),
        ("IsLowercase", Builtin::Pure(is_lowercase)),
        ("IsAlphanumeric", Builtin::Pure(is_alphanumeric)),
        ("StringLength", Builtin::Pure(string_length)),
        ("StringSplit", Builtin::Pure(string_split)),
        ("StringLines", Builtin::Pure(string_lines)),
        ("StringWords", Builtin::Pure(string_words)),
        ("StringTrim", Builtin::Pure(string_trim)),
        ("StringTrimStart", Builtin::Pure(string_trim_start)),
        ("StringTrimEnd", Builtin::Pure(string_trim_end)),
        ("StringStartsWith", Builtin::Pure(string_starts_with)),
        ("StringEndsWith", Builtin::Pure(string_ends_with)),
        ("StringContains", Builtin::Pure(string_contains)),
        ("StringUppercase", Builtin::Pure(string_uppercase)),
        ("StringLowercase", Builtin::Pure(string_lowercase)),
        ("StringJoin", Builtin::Pure(string_join)),
        ("StringSlice", Builtin::Pure(string_slice)),
        ("EncodeUtf8", Builtin::Pure(encode_utf8)),
        ("DecodeUtf8", Builtin::Pure(decode_utf8)),
        ("ParseI64", Builtin::Pure(parse_i64)),
        ("ParseF64", Builtin::Pure(parse_f64)),
        ("ParseU64", Builtin::Pure(parse_u64)),
        ("JoinPath", Builtin::Pure(join_path)),
        ("Print", Builtin::Print { error: false }),
        ("PrintError", Builtin::Print { error: true }),
        ("Args", Builtin::Pure(args)),
        ("ClosedFile", Builtin::Pure(closed_file)),
        ("GetEnv", Builtin::Pure(get_env)),
        ("Not", Builtin::Pure(not)),
        ("And", Builtin::Pure(and)),
        ("Or", Builtin::Pure(or)),
        ("EqualU8", Builtin::Pure(equal_u8)),
        ("LessU8", Builtin::Pure(less_u8)),
        ("LessOrEqualU8", Builtin::Pure(less_or_equal_u8)),
        ("GreaterU8", Builtin::Pure(greater_u8)),
        ("GreaterOrEqualU8", Builtin::Pure(greater_or_equal_u8)),
        ("EqualU64", Builtin::Pure(equal_u64)),
        ("LessU64", Builtin::Pure(less_u64)),
        ("LessOrEqualU64", Builtin::Pure(less_or_equal_u64)),
        ("GreaterU64", Builtin::Pure(greater_u64)),
        ("GreaterOrEqualU64", Builtin::Pure(greater_or_equal_u64)),
        ("EqualI64", Builtin::Pure(equal_i64)),
        ("LessI64", Builtin::Pure(less_i64)),
        ("LessOrEqualI64", Builtin::Pure(less_or_equal_i64)),
        ("GreaterI64", Builtin::Pure(greater_i64)),
        ("GreaterOrEqualI64", Builtin::Pure(greater_or_equal_i64)),
        ("EqualF64", Builtin::Pure(equal_f64)),
        ("LessF64", Builtin::Pure(less_f64)),
        ("LessOrEqualF64", Builtin::Pure(less_or_equal_f64)),
        ("GreaterF64", Builtin::Pure(greater_f64)),
        ("GreaterOrEqualF64", Builtin::Pure(greater_or_equal_f64)),
        ("EqualChar", Builtin::Pure(equal_char)),
        ("EqualString", Builtin::Pure(equal_string)),
        ("EqualBool", Builtin::Pure(equal_bool)),
        ("Now", Builtin::Pure(time::now)),
        ("NowUtc", Builtin::Pure(time::now_utc)),
        ("DateYear", Builtin::Pure(time::date_year)),
        ("DateMonth", Builtin::Pure(time::date_month)),
        ("DateDay", Builtin::Pure(time::date_day)),
        ("DateWeekday", Builtin::Pure(time::date_weekday)),
        ("TimeHour", Builtin::Pure(time::time_hour)),
        ("TimeMinute", Builtin::Pure(time::time_minute)),
        ("TimeSecond", Builtin::Pure(time::time_second)),
        ("TimeMillisecond", Builtin::Pure(time::time_millisecond)),
        ("DateTimeInZone", Builtin::Pure(time::date_time_in_zone)),
        ("TimeZoneName", Builtin::Pure(time::time_zone_name)),
        (
            "TimeZoneOffsetMinutes",
            Builtin::Pure(time::time_zone_offset_minutes),
        ),
        ("DateTimeFromParts", Builtin::Pure(time::date_time_from_parts)),
        ("ParseDateTime", Builtin::Pure(time::parse_date_time)),
        ("DateTimeToString", Builtin::Pure(time::date_time_to_string)),
        (
            "DateTimeToEpochMillis",
            Builtin::Pure(time::date_time_to_epoch_millis),
        ),
        (
            "EpochMillisToDateTime",
            Builtin::Pure(time::epoch_millis_to_date_time),
        ),
        ("LessDateTime", Builtin::Pure(time::less_date_time)),
        (
            "LessOrEqualDateTime",
            Builtin::Pure(time::less_or_equal_date_time),
        ),
        ("EqualDateTime", Builtin::Pure(time::equal_date_time)),
        ("GreaterDateTime", Builtin::Pure(time::greater_date_time)),
        (
            "GreaterOrEqualDateTime",
            Builtin::Pure(time::greater_or_equal_date_time),
        ),
        ("AddYear", Builtin::Pure(time::add_year)),
        ("SubYear", Builtin::Pure(time::sub_year)),
        ("AddMonth", Builtin::Pure(time::add_month)),
        ("SubMonth", Builtin::Pure(time::sub_month)),
        ("AddWeek", Builtin::Pure(time::add_week)),
        ("SubWeek", Builtin::Pure(time::sub_week)),
        ("AddDay", Builtin::Pure(time::add_day)),
        ("SubDay", Builtin::Pure(time::sub_day)),
        ("AddHour", Builtin::Pure(time::add_hour)),
        ("SubHour", Builtin::Pure(time::sub_hour)),
        ("AddMinute", Builtin::Pure(time::add_minute)),
        ("SubMinute", Builtin::Pure(time::sub_minute)),
        ("AddSecond", Builtin::Pure(time::add_second)),
        ("SubSecond", Builtin::Pure(time::sub_second)),
        ("AddMillisecond", Builtin::Pure(time::add_millisecond)),
        ("SubMillisecond", Builtin::Pure(time::sub_millisecond)),
        ("RegexCompile", Builtin::Pure(regex_compile)),
        ("RegexIsMatch", Builtin::Pure(regex_is_match)),
        ("RegexFind", Builtin::Pure(regex_find)),
        ("RegexFindAll", Builtin::Pure(regex_find_all)),
        ("RegexReplace", Builtin::Pure(regex_replace)),
        ("RegexReplaceAll", Builtin::Pure(regex_replace_all)),
        ("RegexSplit", Builtin::Pure(regex_split)),
        ("RegexCaptureNames", Builtin::Pure(regex_capture_names)),
        ("EmptyRegexMatch", Builtin::Pure(regex_empty_match)),
        ("RegexMatchText", Builtin::Pure(regex_match_text)),
        ("RegexMatchStart", Builtin::Pure(regex_match_start)),
        ("RegexMatchEnd", Builtin::Pure(regex_match_end)),
        ("RegexMatchGroup", Builtin::Pure(regex_match_group)),
        ("RegexMatchNamedGroup", Builtin::Pure(regex_match_named_group)),
        (
            "CommandOutputStatus",
            Builtin::Pure(crate::io::command_output_status),
        ),
        (
            "CommandOutputStdout",
            Builtin::Pure(crate::io::command_output_stdout),
        ),
        (
            "CommandOutputStderr",
            Builtin::Pure(crate::io::command_output_stderr),
        ),
    ];

    let io_impls: [(&'static str, Builtin); 31] = [
        ("ReadLine", Builtin::Io(crate::io::read_line_task)),
        ("ReadFile", Builtin::Io(crate::io::read_file_task)),
        ("ReadFileBytes", Builtin::Io(crate::io::read_file_bytes_task)),
        ("ReadKey", Builtin::Io(crate::io::read_key_task)),
        ("EnterRawMode", Builtin::Io(crate::io::enter_raw_mode_task)),
        ("ExitRawMode", Builtin::Io(crate::io::exit_raw_mode_task)),
        ("WriteFile", Builtin::Io(crate::io::write_file_task)),
        ("WriteFileBytes", Builtin::Io(crate::io::write_file_bytes_task)),
        ("AppendFile", Builtin::Io(crate::io::append_file_task)),
        (
            "AppendFileBytes",
            Builtin::Io(crate::io::append_file_bytes_task),
        ),
        ("ListDirectory", Builtin::Io(crate::io::list_directory_task)),
        ("CreateDirectory", Builtin::Io(crate::io::create_directory_task)),
        (
            "CreateDirectoryAll",
            Builtin::Io(crate::io::create_directory_all_task),
        ),
        ("RemoveDirectory", Builtin::Io(crate::io::remove_directory_task)),
        ("RemoveFile", Builtin::Io(crate::io::remove_file_task)),
        ("RenamePath", Builtin::Io(crate::io::rename_path_task)),
        ("CopyFile", Builtin::Io(crate::io::copy_file_task)),
        ("IsPathExists", Builtin::Io(crate::io::is_path_exists_task)),
        ("IsFile", Builtin::Io(crate::io::is_file_task)),
        ("IsDirectory", Builtin::Io(crate::io::is_directory_task)),
        (
            "GetCurrentDirectory",
            Builtin::Io(crate::io::get_current_directory_task),
        ),
        ("OpenRead", Builtin::Io(crate::io::open_read_task)),
        ("OpenWrite", Builtin::Io(crate::io::open_write_task)),
        ("OpenAppend", Builtin::Io(crate::io::open_append_task)),
        ("Close", Builtin::Io(crate::io::close_task)),
        ("Flush", Builtin::Io(crate::io::flush_task)),
        ("ReadLineFrom", Builtin::Io(crate::io::read_line_from_task)),
        ("ReadChunk", Builtin::Io(crate::io::read_chunk_task)),
        ("WriteString", Builtin::Io(crate::io::write_string_task)),
        ("WriteBytes", Builtin::Io(crate::io::write_bytes_task)),
        ("RunCommand", Builtin::Io(crate::io::run_command_task)),
    ];

    let socket_impls: [(&'static str, Builtin); 16] = [
        ("TcpListen", Builtin::Io(crate::io::net::tcp_listen_task)),
        ("TcpAccept", Builtin::Io(crate::io::net::tcp_accept_task)),
        ("TcpConnect", Builtin::Io(crate::io::net::tcp_connect_task)),
        (
            "TcpCloseListener",
            Builtin::Io(crate::io::net::tcp_close_listener_task),
        ),
        ("TcpLocalPort", Builtin::Io(crate::io::net::tcp_local_port_task)),
        ("SocketPeer", Builtin::Io(crate::io::net::socket_peer_task)),
        (
            "SocketIsClosed",
            Builtin::Io(crate::io::net::socket_is_closed_task),
        ),
        (
            "SocketShutdownWrite",
            Builtin::Io(crate::io::net::socket_shutdown_write_task),
        ),
        ("SocketRead", Builtin::Io(crate::io::net::socket_read_task)),
        (
            "SocketReadExact",
            Builtin::Io(crate::io::net::socket_read_exact_task),
        ),
        (
            "SocketReadAll",
            Builtin::Io(crate::io::net::socket_read_all_task),
        ),
        (
            "SocketReadLine",
            Builtin::Io(crate::io::net::socket_read_line_task),
        ),
        ("SocketWrite", Builtin::Io(crate::io::net::socket_write_task)),
        (
            "SocketWriteAll",
            Builtin::Io(crate::io::net::socket_write_all_task),
        ),
        (
            "SocketWriteLine",
            Builtin::Io(crate::io::net::socket_write_line_task),
        ),
        ("SocketClose", Builtin::Io(crate::io::net::socket_close_task)),
    ];
    pure_impls
        .into_iter()
        .chain(io_impls)
        .chain(socket_impls)
        .collect()
});

/// 按编号取内置函数的可执行形态.
pub(crate) fn resolve(id: BuiltinId) -> Option<Builtin> {
    let implementation = IMPLS.get(id.index() as usize)?;
    debug_assert_eq!(implementation.0, name(id), "内置函数实现与 catalog 顺序不一致");
    Some(implementation.1)
}

/// 实现数量; 供测试核对.
#[cfg(test)]
#[must_use]
pub(crate) fn implementation_count() -> usize {
    IMPLS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implementation_order_matches_catalog() {
        assert_eq!(implementation_count(), ksl_catalog::builtin::len());
        for (index, (entry, spec)) in
            IMPLS.iter().zip(ksl_catalog::builtin::specs()).enumerate()
        {
            assert_eq!(entry.0, spec.name, "第 {index} 项");
            assert!(resolve(BuiltinId::from_index(u16::try_from(index).unwrap())).is_some());
        }
    }
}
