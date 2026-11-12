---
updated: 2026-11-26
---

# ksl 时间与日期

ksl 的日期时间函数.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.

## 1. 类型与语义

日期时间以不透明类型 `DateTime` 表示, 形态是绝对时刻加时区, 不可打印.
时刻精度为毫秒; `Now` 与 `NowUtc` 裁剪到毫秒, 解析与输出保留输入的小数秒.
命名上, 日期组件用 `Date` 前缀, 时刻组件用 `Time` 前缀,
整体值操作用 `DateTime` 前缀, 时区视图用 `TimeZone` 前缀.
`TimeError` 是时间域的错误类别, 不是函数前缀.

时刻自带命名时区或固定偏移, 字段按自带时区解释.
Unix 平台优先读取系统 tzdb, 其余情况使用内嵌副本.
夏令时会使本地时间不存在或出现两次;
构造与解析按兼容策略处理: 空洞顺延一个空洞长度, 重叠取较早的偏移.

解析字符串与时区名属于外部输入, 返回 `Result[_, TimeError]`.
已构造的 `DateTime` 值在换算或算术中失败属于程序错误, 报运行时错误.

相对时间原语 `@Sleep`, `@SetTimer` 与 `@Receive` 的时长以毫秒计,
基于单调时钟, 不换算为日历时间; `@CancelTimer` 只按编号取消.

## 2. 取当前时刻

- `Now[] : DateTime`
  - 返回宿主本地时区的当前时刻.
- `NowUtc[] : DateTime`
  - 返回 UTC 的当前时刻.

返回值裁剪到毫秒.

## 3. 字段

- `DateYear[value : DateTime] : i64`
  - 年.
- `DateMonth[value : DateTime] : i64`
  - 月.
- `DateDay[value : DateTime] : i64`
  - 日.
- `DateWeekday[value : DateTime] : i64`
  - 星期; 按 ISO 8601, 1 为周一, 7 为周日.
- `TimeHour[value : DateTime] : i64`
  - 时.
- `TimeMinute[value : DateTime] : i64`
  - 分.
- `TimeSecond[value : DateTime] : i64`
  - 秒.
- `TimeMillisecond[value : DateTime] : i64`
  - 毫秒.

## 4. 时区

- `DateTimeInZone[value : DateTime, zone : string] : Result[DateTime, TimeError]`
  - 换时区而不改时刻; 时区名不存在时报 `TimeError/UnknownZone`.
- `TimeZoneName[value : DateTime] : string`
  - 返回 IANA 名; 固定偏移时区显示为偏移文本, 例如 `+05:30`.
- `TimeZoneOffsetMinutes[value : DateTime] : i64`
  - 返回相对 UTC 的偏移分钟数.

## 5. 构造与解析

- `DateTimeFromParts[year : i64, month : i64, day : i64, hour : i64, minute : i64, second : i64] : Result[DateTime, TimeError]`
  - 按宿主本地时区构造; 参数依次为年, 月, 日, 时, 分, 秒.
    字段越界报 `TimeError/InvalidField`.
    需要按指定时区构造时, 用带 `[时区]` 后缀的 `ParseDateTime`.
- `ParseDateTime[text : string] : Result[DateTime, TimeError]`
  - 解析日期时间字符串. 只有合法的 `[时区]` 后缀按该时区解释,
    其余输入取民用时间并按宿主本地时区解释, 无时区名的偏移被忽略;
    `Z` 与其他形态错误报 `TimeError/InvalidFormat`.

## 6. 输出与换算

- `DateTimeToString[value : DateTime] : string`
  - 按 ISO 8601 扩展格式输出; 与 `ParseDateTime` 可互相往返.
- `DateTimeToEpochMillis[value : DateTime] : i64`
  - 返回 Unix epoch 毫秒; 1970 之前为负.
- `EpochMillisToDateTime[millis : i64] : DateTime`
  - 由 Unix epoch 毫秒构造, 返回 UTC 时刻; 超出范围报 `DateTimeOutOfRange`.

## 7. 比较

按绝对时刻比较, 显示时区不参与判断; 同一时刻的不同时区表示相等.

- `LessDateTime[left : DateTime, right : DateTime] : bool`
  - 小于.
- `LessOrEqualDateTime[left : DateTime, right : DateTime] : bool`
  - 小于等于.
- `EqualDateTime[left : DateTime, right : DateTime] : bool`
  - 相等.
- `GreaterDateTime[left : DateTime, right : DateTime] : bool`
  - 大于.
- `GreaterOrEqualDateTime[left : DateTime, right : DateTime] : bool`
  - 大于等于.

## 8. 算术

计数为 u64, 结果超出可表示范围报运行时错误 `DateTimeOutOfRange`.
年月日周按日历解释: 结果的本地日期前进或后退 count 个单位, 时钟字段保持不变.
年月运算遇到目标月份没有对应日期时, 取该月最后一天.
一周按七天计算, `AddWeek[d, 1u64]` 与 `AddDay[d, 7u64]` 结果相同.

时分秒毫秒按精确时长计算.
跨夏令时的差异: 01:30 加一小时得 03:30, 加一天得次日 01:30.

- `AddYear[value : DateTime, count : u64] : DateTime`
  - count 年后的日期.
- `SubYear[value : DateTime, count : u64] : DateTime`
  - count 年前的日期.
- `AddMonth[value : DateTime, count : u64] : DateTime`
  - count 个月后的日期.
- `SubMonth[value : DateTime, count : u64] : DateTime`
  - count 个月前的日期.
- `AddWeek[value : DateTime, count : u64] : DateTime`
  - count 周后的日期.
- `SubWeek[value : DateTime, count : u64] : DateTime`
  - count 周前的日期.
- `AddDay[value : DateTime, count : u64] : DateTime`
  - count 天后的日期.
- `SubDay[value : DateTime, count : u64] : DateTime`
  - count 天前的日期.
- `AddHour[value : DateTime, count : u64] : DateTime`
  - count 小时后的时刻.
- `SubHour[value : DateTime, count : u64] : DateTime`
  - count 小时前的时刻.
- `AddMinute[value : DateTime, count : u64] : DateTime`
  - count 分钟后的时刻.
- `SubMinute[value : DateTime, count : u64] : DateTime`
  - count 分钟前的时刻.
- `AddSecond[value : DateTime, count : u64] : DateTime`
  - count 秒后的时刻.
- `SubSecond[value : DateTime, count : u64] : DateTime`
  - count 秒前的时刻.
- `AddMillisecond[value : DateTime, count : u64] : DateTime`
  - count 毫秒后的时刻.
- `SubMillisecond[value : DateTime, count : u64] : DateTime`
  - count 毫秒前的时刻.

## 9. 时间错误

时间错误是内置 Data 类型 `TimeError`, 变体为 `InvalidFormat[message]`,
`UnknownZone[name]` 与 `InvalidField[description]`.
