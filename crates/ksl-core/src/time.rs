//! 时间与日期.
//!
//! 时间值由 jiff 的 `Zoned` 承载: 绝对时刻加时区. 字段按自带时区解释;
//! 日历算术 (年, 月, 周, 日) 保持本地时间, 时长算术 (时, 分, 秒,
//! 毫秒) 按精确时长. 时区数据由 jiff 提供: Unix 上优先读系统 tzdb,
//! 其余情况回退到内嵌副本, 故各平台的命名时区都可用.
//!
//! 失败分两类: 解析字符串与时区名属不可信输入, 返回 `Result[_, TimeError]`;
//! 已定型值上的换算与算术失败属程序错误, 报运行时错误.

use std::sync::Arc;

use jiff::fmt::temporal::DateTimeParser;
use jiff::tz::TimeZone;
use jiff::{Span, Timestamp, Zoned, civil};

use crate::error::{ErrorKind, RuntimeError as Error};
use crate::source::Span as SourceSpan;
use crate::symbol::intern;
use crate::value::{Value, data_value, result_err, result_ok};

/// 解析时间字符串与推断时区的共用解析器.
static PARSER: DateTimeParser = DateTimeParser::new();

/// 取时间实参; 形态不符即内部错误.
fn date_time<'a>(args: &'a [Value], index: usize, what: &str) -> Result<&'a Zoned, Error> {
    match args.get(index) {
        Some(Value::DateTime(value)) => Ok(value),
        _ => Err(Error::internal(format!("{what}不是 DateTime"))),
    }
}

/// 取整数实参; 形态不符即内部错误.
fn int_arg(args: &[Value], index: usize, what: &str) -> Result<i64, Error> {
    match args.get(index) {
        Some(Value::I64(value)) => Ok(*value),
        _ => Err(Error::internal(format!("{what}不是 i64"))),
    }
}

/// 取 u64 实参; 形态不符即内部错误.
fn u64_arg(args: &[Value], index: usize, what: &str) -> Result<u64, Error> {
    match args.get(index) {
        Some(Value::U64(value)) => Ok(*value),
        _ => Err(Error::internal(format!("{what}不是 u64"))),
    }
}

/// 取字符串实参; 形态不符即内部错误.
fn string_arg<'a>(args: &'a [Value], index: usize, what: &str) -> Result<&'a Arc<str>, Error> {
    match args.get(index) {
        Some(Value::Str(text)) => Ok(text),
        _ => Err(Error::internal(format!("{what}不是字符串"))),
    }
}

fn date_time_value(value: Zoned) -> Value {
    Value::DateTime(Arc::new(value))
}

/// 构造 `TimeError` 值.
fn time_error(variant: &str, field: &str) -> Value {
    data_value(
        intern("TimeError"),
        intern(variant),
        vec![Value::Str(Arc::from(field))],
    )
}

/// 当前时刻, 按宿主本地时区; 精度裁剪到毫秒, 与语言的时间精度一致.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn now(_: &[String], _: &[Value], _: Option<SourceSpan>) -> Result<Value, Error> {
    Ok(date_time_value(truncate(&Zoned::now())))
}

/// 当前时刻, 按 UTC; 精度裁剪到毫秒.
#[allow(clippy::unnecessary_wraps)]
pub(crate) fn now_utc(
    _: &[String],
    _: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let utc = Zoned::now().with_time_zone(TimeZone::UTC);
    Ok(date_time_value(truncate(&utc)))
}

/// 把时刻裁剪到毫秒: 时钟的微秒以下部分是噪声, 且语言只承诺毫秒精度.
fn truncate(value: &Zoned) -> Zoned {
    let millis = value.timestamp().as_millisecond();
    Timestamp::from_millisecond(millis).map_or_else(
        |_| value.clone(),
        |timestamp| timestamp.to_zoned(value.time_zone().clone()),
    )
}

/// 宿主本地时区; 首次使用时读取系统状态, 之后复用.
///
/// 每次调用 `TimeZone::system` 都可能重读 `TZ` 与 `/etc/localtime`, 构造与
/// 解析时间是常规操作, 故缓存一份.
fn local_zone() -> TimeZone {
    static LOCAL: std::sync::OnceLock<TimeZone> = std::sync::OnceLock::new();
    LOCAL.get_or_init(TimeZone::system).clone()
}

/// 由字段取值的公共骨架.
fn field(args: &[Value], what: &str, take: impl Fn(&Zoned) -> i64) -> Result<Value, Error> {
    let value = date_time(args, 0, what)?;
    Ok(Value::I64(take(value)))
}

pub(crate) fn date_year(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "DateYear 的实参", |d| i64::from(d.year()))
}

pub(crate) fn date_month(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "DateMonth 的实参", |d| i64::from(d.month()))
}

pub(crate) fn date_day(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "DateDay 的实参", |d| i64::from(d.day()))
}

/// 星期几, 按 ISO 8601: 1 为周一, 7 为周日.
pub(crate) fn date_weekday(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "DateWeekday 的实参", |d| {
        i64::from(d.weekday().to_monday_one_offset())
    })
}

pub(crate) fn time_hour(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "TimeHour 的实参", |d| i64::from(d.hour()))
}

pub(crate) fn time_minute(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "TimeMinute 的实参", |d| i64::from(d.minute()))
}

pub(crate) fn time_second(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "TimeSecond 的实参", |d| i64::from(d.second()))
}

pub(crate) fn time_millisecond(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    field(a, "TimeMillisecond 的实参", |d| i64::from(d.millisecond()))
}

/// 换成指定时区, 时刻不变; 时区名不存在时返回 `TimeError/UnknownZone`.
pub(crate) fn date_time_in_zone(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let value = date_time(a, 0, "DateTimeInZone 的实参")?;
    let name = string_arg(a, 1, "DateTimeInZone 的时区名")?;
    match TimeZone::get(name) {
        Ok(zone) => Ok(result_ok(date_time_value(value.with_time_zone(zone)))),
        Err(_) => Ok(result_err(time_error("UnknownZone", name))),
    }
}

/// 时区名; 固定偏移的时区没有名字, 退化为偏移文本, 例如 `+05:30`.
pub(crate) fn time_zone_name(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let value = date_time(a, 0, "TimeZoneName 的实参")?;
    let zone = value.time_zone();
    let name = zone
        .iana_name()
        .map_or_else(|| value.offset().to_string(), str::to_string);
    Ok(Value::Str(Arc::from(name)))
}

/// 当前时区相对 UTC 的偏移分钟数; 非整小时时区 (如 +05:30) 也能精确表示.
pub(crate) fn time_zone_offset_minutes(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let value = date_time(a, 0, "TimeZoneOffsetMinutes 的实参")?;
    Ok(Value::I64(i64::from(value.offset().seconds()) / 60))
}

/// 按字段构造时间, 时区取宿主本地时区.
///
/// 夏令时空洞按兼容策略前移, 重叠取较早的偏移.
pub(crate) fn date_time_from_parts(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let fields = ["年", "月", "日", "时", "分", "秒"];
    let mut parts = [0_i64; 6];
    for (index, name) in fields.iter().enumerate() {
        parts[index] = int_arg(a, index, &format!("DateTimeFromParts 的{name}"))?;
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
    match civil.to_zoned(local_zone()) {
        Ok(value) => Ok(result_ok(date_time_value(value))),
        Err(error) => Ok(result_err(time_error("InvalidField", &error.to_string()))),
    }
}

/// 解析时间字符串.
///
/// 带偏移或 `[时区]` 后缀的按该时区解释; 不带时区的按宿主本地时区解释.
/// 输出与 `DateTimeToString` 的形态可互相往返.
pub(crate) fn parse_date_time(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let text = string_arg(a, 0, "ParseDateTime 的实参")?;
    if let Ok(value) = PARSER.parse_zoned(text.as_bytes()) {
        return Ok(result_ok(date_time_value(value)));
    }
    match PARSER.parse_datetime(text.as_bytes()) {
        Ok(civil) => match civil.to_zoned(local_zone()) {
            Ok(value) => Ok(result_ok(date_time_value(value))),
            Err(error) => Ok(result_err(time_error("InvalidField", &error.to_string()))),
        },
        Err(error) => Ok(result_err(time_error("InvalidFormat", &error.to_string()))),
    }
}

/// 按 ISO 8601 扩展格式输出, 与 `ParseDateTime` 可互相往返.
pub(crate) fn date_time_to_string(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let value = date_time(a, 0, "DateTimeToString 的实参")?;
    Ok(Value::Str(Arc::from(value.to_string())))
}

/// 取 Unix epoch 毫秒; 1970 年之前为负.
pub(crate) fn date_time_to_epoch_millis(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    let value = date_time(a, 0, "DateTimeToEpochMillis 的实参")?;
    Ok(Value::I64(value.timestamp().as_millisecond()))
}

/// 由 Unix epoch 毫秒构造 UTC 时刻.
pub(crate) fn epoch_millis_to_date_time(
    _: &[String],
    a: &[Value],
    at: Option<SourceSpan>,
) -> Result<Value, Error> {
    let millis = int_arg(a, 0, "EpochMillisToDateTime 的实参")?;
    let timestamp = Timestamp::from_millisecond(millis)
        .map_err(|_| Error::runtime(ErrorKind::DateTimeOutOfRange, at))?;
    Ok(date_time_value(timestamp.to_zoned(TimeZone::UTC)))
}

/// 比较两个 `DateTime` 的绝对时刻; 显示时区不参与判断.
fn compare_date_time(
    a: &[Value],
    name: &str,
    compare: impl Fn(&Zoned, &Zoned) -> bool,
) -> Result<Value, Error> {
    let left = date_time(a, 0, &format!("{name} 的第一个实参"))?;
    let right = date_time(a, 1, &format!("{name} 的第二个实参"))?;
    Ok(Value::Bool(compare(left, right)))
}

pub(crate) fn less_date_time(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    compare_date_time(a, "LessDateTime", |left, right| left < right)
}

pub(crate) fn less_or_equal_date_time(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    compare_date_time(a, "LessOrEqualDateTime", |left, right| left <= right)
}

pub(crate) fn equal_date_time(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    compare_date_time(a, "EqualDateTime", |left, right| left == right)
}

pub(crate) fn greater_date_time(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    compare_date_time(a, "GreaterDateTime", |left, right| left > right)
}

pub(crate) fn greater_or_equal_date_time(
    _: &[String],
    a: &[Value],
    _: Option<SourceSpan>,
) -> Result<Value, Error> {
    compare_date_time(a, "GreaterOrEqualDateTime", |left, right| left >= right)
}

/// 在时间上增减一个单位; `sign` 为 1 表示加, -1 表示减.
///
/// 年, 月, 周, 日按日历解释, 保持本地时间; 时, 分, 秒, 毫秒按精确时长.
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
        pub(crate) fn $add(
            _: &[String],
            a: &[Value],
            at: Option<SourceSpan>,
        ) -> Result<Value, Error> {
            shift(a, $what, Span::$unit, 1, at)
        }

        pub(crate) fn $sub(
            _: &[String],
            a: &[Value],
            at: Option<SourceSpan>,
        ) -> Result<Value, Error> {
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
