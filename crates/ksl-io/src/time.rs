//! 时间与日期.
//!
//! 时间值以 jiff 的 `ZonedDateTime` 保存: 绝对时刻加时区.
//! 字段按自带时区解释.
//! 日历算术保持本地时间, 涉及年, 月, 周, 日.
//! 时长算术按精确时长, 涉及时, 分, 秒, 毫秒.
//! 时区数据由 jiff 提供: Unix 平台优先读系统 tzdb, 其余情况回退到内嵌副本.
//!
//! 失败分两类.
//! 解析字符串与时区名属不可信输入, 返回 `Result[_, TimeError]`.
//! 已定型值上的换算与算术失败属程序错误, 报运行时错误.

use std::sync::Arc;

use arcstr::ArcStr;
use jiff::fmt::temporal::DateTimeParser;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, ZonedDateTime, civil};
use ksl_base::name::Name;
use ksl_base::source::Span as SourceSpan;
use ksl_diagnostics::error::{ErrorKind, RuntimeError as Error};
use ksl_value::args::{i64_arg, str_arg, u64_arg};
use ksl_value::value::{Value, data_value, result_err, result_ok};

use crate::host;

/// 解析时间字符串与推断时区的共用解析器.
static PARSER: DateTimeParser = DateTimeParser::new();

/// 取时间实参; 形态不符即内部错误.
fn date_time<'a>(
    args: &'a [Value],
    index: usize,
    what: &str,
) -> Result<&'a ZonedDateTime, Error> {
    match args.get(index).and_then(host::as_date_time) {
        Some(value) => Ok(value),
        None => Err(Error::internal(format!("{what} 不是 DateTime"))),
    }
}

fn date_time_value(value: ZonedDateTime) -> Value {
    host::date_time(Arc::new(value))
}

fn time_error(variant: &str, field: &str) -> Value {
    data_value(
        Name::new("TimeError"),
        Name::new(variant),
        [Value::Str(ArcStr::from(field))],
    )
}

/// 当前时刻, 按宿主本地时区; 精度为毫秒.
///
/// # Errors
///
/// 本函数不会失败.
pub fn now(_: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    Ok(date_time_value(truncate(&ZonedDateTime::now())))
}

/// 当前时刻, 按 UTC; 精度为毫秒.
///
/// # Errors
///
/// 本函数不会失败.
pub fn now_utc(_: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let utc = ZonedDateTime::now().with_time_zone(TimeZone::UTC);
    Ok(date_time_value(truncate(&utc)))
}

/// 把时刻裁剪到毫秒; 语言只承诺毫秒精度.
fn truncate(value: &ZonedDateTime) -> ZonedDateTime {
    let millis = value.timestamp().as_millisecond();
    Timestamp::from_millisecond(millis).map_or_else(
        |_| value.clone(),
        |timestamp| timestamp.to_zoned_datetime(value.time_zone().clone()),
    )
}

/// 宿主本地时区缓存.
///
/// `TimeZone::system` 会重读 `TZ` 与 `/etc/localtime`.
fn local_zone() -> TimeZone {
    static LOCAL: std::sync::OnceLock<TimeZone> = std::sync::OnceLock::new();
    LOCAL.get_or_init(TimeZone::system).clone()
}

/// 由字段取值的通用实现.
fn field(
    args: &[Value],
    what: &str,
    take: impl Fn(&ZonedDateTime) -> i64,
) -> Result<Value, Error> {
    let value = date_time(args, 0, what)?;
    Ok(Value::I64(take(value)))
}

/// 取年份.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_year(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "DateYear 的实参", |d| i64::from(d.year()))
}

/// 取月份.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_month(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "DateMonth 的实参", |d| i64::from(d.month()))
}

/// 取日.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_day(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "DateDay 的实参", |d| i64::from(d.day()))
}

/// 星期几, 按 ISO 8601: 1 为周一, 7 为周日.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_weekday(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "DateWeekday 的实参", |d| {
        i64::from(d.weekday().to_monday_one_offset())
    })
}

/// 取小时.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn time_hour(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "TimeHour 的实参", |d| i64::from(d.hour()))
}

/// 取分钟.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn time_minute(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "TimeMinute 的实参", |d| i64::from(d.minute()))
}

/// 取秒.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn time_second(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "TimeSecond 的实参", |d| i64::from(d.second()))
}

/// 取毫秒.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn time_millisecond(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    field(a, "TimeMillisecond 的实参", |d| i64::from(d.millisecond()))
}

/// 换成指定时区, 时刻不变; 时区名不存在时返回 `TimeError/UnknownZone`.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_time_in_zone(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let value = date_time(a, 0, "DateTimeInZone 的实参")?;
    let name = str_arg(a, 1, "DateTimeInZone 的时区名")?;
    match TimeZone::get(name) {
        Ok(zone) => Ok(result_ok(date_time_value(value.with_time_zone(zone)))),
        Err(_) => Ok(result_err(time_error("UnknownZone", name))),
    }
}

/// 时区名; 固定偏移的时区没有名字, 退化为偏移文本, 如 `+05:30`.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn time_zone_name(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let value = date_time(a, 0, "TimeZoneName 的实参")?;
    let zone = value.time_zone();
    let name = zone
        .iana_name()
        .map_or_else(|| value.offset().to_string(), str::to_string);
    Ok(Value::Str(ArcStr::from(name)))
}

/// 当前时区相对 UTC 的偏移分钟数; 非整小时时区也能精确表示, 如 +05:30.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn time_zone_offset_minutes(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let value = date_time(a, 0, "TimeZoneOffsetMinutes 的实参")?;
    Ok(Value::I64(i64::from(value.offset().seconds()) / 60))
}

/// 按字段构造时间, 时区取宿主本地时区.
///
/// 夏令时空洞顺延一个空洞长度, 重叠取较早的偏移.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_time_from_parts(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let fields = ["年", "月", "日", "时", "分", "秒"];
    let mut parts = [0_i64; 6];
    for (index, name) in fields.iter().enumerate() {
        parts[index] = i64_arg(a, index, &format!("DateTimeFromParts 的{name}"))?;
    }
    let (year, month, day, hour, minute, second) = (
        i16::try_from(parts[0]),
        i8::try_from(parts[1]),
        i8::try_from(parts[2]),
        i8::try_from(parts[3]),
        i8::try_from(parts[4]),
        i8::try_from(parts[5]),
    );
    let (Ok(year), Ok(month), Ok(day), Ok(hour), Ok(minute), Ok(second)) =
        (year, month, day, hour, minute, second)
    else {
        return Ok(result_err(time_error("InvalidField", "字段超出可表示范围")));
    };
    let civil = match civil::DateTime::new(year, month, day, hour, minute, second, 0) {
        Ok(civil) => civil,
        Err(error) => {
            return Ok(result_err(time_error("InvalidField", &error.to_string())));
        },
    };
    match civil.to_zoned_datetime(local_zone()) {
        Ok(value) => Ok(result_ok(date_time_value(value))),
        Err(error) => Ok(result_err(time_error("InvalidField", &error.to_string()))),
    }
}

/// 解析时间字符串.
///
/// 只有合法的 `[时区]` 后缀按该时区解释, 其余输入取民用时间并按宿主本地时区解释.
/// 输出与 `DateTimeToString` 的形态可互相往返.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn parse_date_time(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let text = str_arg(a, 0, "ParseDateTime 的实参")?;
    if let Ok(value) = PARSER.parse_zoned_datetime(text.as_bytes()) {
        return Ok(result_ok(date_time_value(value)));
    }
    match PARSER.parse_datetime(text.as_bytes()) {
        Ok(civil) => match civil.to_zoned_datetime(local_zone()) {
            Ok(value) => Ok(result_ok(date_time_value(value))),
            Err(error) => Ok(result_err(time_error("InvalidField", &error.to_string()))),
        },
        Err(error) => Ok(result_err(time_error("InvalidFormat", &error.to_string()))),
    }
}

/// 按 ISO 8601 扩展格式输出, 与 `ParseDateTime` 可互相往返.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_time_to_string(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let value = date_time(a, 0, "DateTimeToString 的实参")?;
    Ok(Value::Str(ArcStr::from(value.to_string())))
}

/// 取 Unix epoch 毫秒; 1970 年之前为负.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn date_time_to_epoch_millis(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    let value = date_time(a, 0, "DateTimeToEpochMillis 的实参")?;
    Ok(Value::I64(value.timestamp().as_millisecond()))
}

/// 由 Unix epoch 毫秒构造 UTC 时刻.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误; 结果超出日期时间范围时返回 `DateTimeOutOfRange`.
pub fn epoch_millis_to_date_time(a: &[Value], at: Option<SourceSpan>) -> Result<Value, Error> {
    let millis = i64_arg(a, 0, "EpochMillisToDateTime 的实参")?;
    let timestamp = Timestamp::from_millisecond(millis)
        .map_err(|_| Error::runtime(ErrorKind::DateTimeOutOfRange, at))?;
    Ok(date_time_value(timestamp.to_zoned_datetime(TimeZone::UTC)))
}

/// 比较两个 `DateTime` 的绝对时刻; 显示时区不参与判断.
fn compare_date_time(
    a: &[Value],
    name: &str,
    compare: impl Fn(&ZonedDateTime, &ZonedDateTime) -> bool,
) -> Result<Value, Error> {
    let left = date_time(a, 0, &format!("{name} 的第一个实参"))?;
    let right = date_time(a, 1, &format!("{name} 的第二个实参"))?;
    Ok(Value::Bool(compare(left, right)))
}

/// 判断是否早于.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn less_date_time(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    compare_date_time(a, "LessDateTime", |left, right| left < right)
}

/// 判断是否不晚于.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn less_or_equal_date_time(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    compare_date_time(a, "LessOrEqualDateTime", |left, right| left <= right)
}

/// 判断是否相等.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn equal_date_time(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    compare_date_time(a, "EqualDateTime", |left, right| left == right)
}

/// 判断是否晚于.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn greater_date_time(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    compare_date_time(a, "GreaterDateTime", |left, right| left > right)
}

/// 判断是否不早于.
///
/// # Errors
///
/// 实参缺失或形态不符时返回内部错误.
pub fn greater_or_equal_date_time(a: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    compare_date_time(a, "GreaterOrEqualDateTime", |left, right| left >= right)
}

/// 在时间上增减一个单位; `sign` 为 1 表示加, -1 表示减.
///
/// 年, 月, 周, 日按日历解释, 保持本地时间.
/// 时, 分, 秒, 毫秒按精确时长.
fn shift(
    args: &[Value],
    what: &str,
    unit: fn(Span, i64) -> Result<Span, jiff::Error>,
    sign: i64,
    at: Option<SourceSpan>,
) -> Result<Value, Error> {
    let value = date_time(args, 0, what)?;
    let count = u64_arg(args, 1, what)?;
    let count =
        i64::try_from(count).map_err(|_| Error::runtime(ErrorKind::DateTimeOutOfRange, at))?;
    let count = count
        .checked_mul(sign)
        .ok_or_else(|| Error::runtime(ErrorKind::DateTimeOutOfRange, at))?;
    let span = unit(Span::new(), count)
        .map_err(|_| Error::runtime(ErrorKind::DateTimeOutOfRange, at))?;
    let shifted = value
        .checked_add(span)
        .map_err(|_| Error::runtime(ErrorKind::DateTimeOutOfRange, at))?;
    Ok(date_time_value(shifted))
}

/// 生成 Add 与 Sub 两个方向的内置函数.
macro_rules! shift_pair {
    ($add:ident, $sub:ident, $what:expr, $unit:ident) => {
        /// 调整时间.
        ///
        /// # Errors
        ///
        /// 实参缺失或形态不符时返回内部错误; 结果超出日期时间范围时返回 `DateTimeOutOfRange`.
        pub fn $add(a: &[Value], at: Option<SourceSpan>) -> Result<Value, Error> {
            shift(a, $what, Span::$unit, 1, at)
        }

        /// 调整时间.
        ///
        /// # Errors
        ///
        /// 实参缺失或形态不符时返回内部错误; 结果超出日期时间范围时返回 `DateTimeOutOfRange`.
        pub fn $sub(a: &[Value], at: Option<SourceSpan>) -> Result<Value, Error> {
            shift(a, $what, Span::$unit, -1, at)
        }
    };
}

shift_pair!(add_year, sub_year, "AddYear/SubYear 的实参", try_years);
shift_pair!(add_month, sub_month, "AddMonth/SubMonth 的实参", try_months);
shift_pair!(add_week, sub_week, "AddWeek/SubWeek 的实参", try_weeks);
shift_pair!(add_day, sub_day, "AddDay/SubDay 的实参", try_days);
shift_pair!(add_hour, sub_hour, "AddHour/SubHour 的实参", try_hours);
shift_pair!(
    add_minute,
    sub_minute,
    "AddMinute/SubMinute 的实参",
    try_minutes
);
shift_pair!(
    add_second,
    sub_second,
    "AddSecond/SubSecond 的实参",
    try_seconds
);
shift_pair!(
    add_millisecond,
    sub_millisecond,
    "AddMillisecond/SubMillisecond 的实参",
    try_milliseconds
);
