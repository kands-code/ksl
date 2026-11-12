---
updated: 2026-11-25
---

# ksl 字符串与字符

ksl 的字符串操作, 字符谓词, 编解码与解析函数.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
字面值形态见 `docs/topics/language/syntax.md` 第 4.2 节.

## 1. 字符串操作

字符串操作以 `String` 前缀命名.
需要分隔符的拼接先用 `@ListRiffle[list, separator, 1u64]` 交替插入.
字符列表往返适合字符级处理; 普通文本处理优先使用 `StringSplit`, `StringLines` 与 `StringWords`.

- `StringLength[text : string] : u64`
  - 返回 Unicode 标量数量.
- `StringSplit[text : string, separator : string] : List[string]`
  - 按非空分隔符拆分, 保留空片段; 分隔符为空时返回只含原串的单元素列表.
- `StringLines[text : string] : List[string]`
  - 按 `\n` 与 `\r\n` 分行, 不产生末尾空行; 空串返回空列表.
- `StringWords[text : string] : List[string]`
  - 按 Unicode 空白拆词, 丢弃空片段.
- `StringTrim[text : string] : string`
  - 去除两端 Unicode 空白.
- `StringTrimStart[text : string] : string`
  - 去除开头 Unicode 空白.
- `StringTrimEnd[text : string] : string`
  - 去除结尾 Unicode 空白.
- `StringStartsWith[text : string, prefix : string] : bool`
  - 是否以指定前缀开头.
- `StringEndsWith[text : string, suffix : string] : bool`
  - 是否以指定后缀结尾.
- `StringContains[text : string, needle : string] : bool`
  - 是否含指定子串.
- `StringIndexOf[text : string, needle : string] : Option[u64]`
  - 返回首个匹配的 Unicode 标量下标; 未命中返回 None.
    空子串按空匹配处理, 返回 Some[0].
- `StringUppercase[text : string] : string`
  - 返回 Unicode 大写转换结果.
- `StringLowercase[text : string] : string`
  - 返回 Unicode 小写转换结果.
- `StringJoin[parts : List[string]] : string`
  - 连接字符串列表.
- `StringReplace[text : string, from : string, replacement : string] : string`
  - 将第一处字面量 from 替换为 replacement; 替换串按字面处理, 不解析捕获组.
    空 from 按空匹配处理, 在开头插入 replacement.
- `StringReplaceAll[text : string, from : string, replacement : string] : string`
  - 替换全部非重叠的字面量匹配; 替换串按字面处理.
    空 from 按空匹配处理, 在每个字符边界插入 replacement.
- `StringSlice[text : string, start : u64, end : u64] : string`
  - 按字符取子串; 区间左闭右开, 越界处截到串长.
    起点不小于终点时返回空串.
- `StringToChars[text : string] : List[char]`
  - 拆分为字符列表.
- `CharsToString[chars : List[char]] : string`
  - 合并为字符串.
- `EqualString[left : string, right : string] : bool`
  - 判断两个字符串是否相等.

## 2. 字符

- `EqualChar[left : char, right : char] : bool`
  - 判断两个字符是否相等.
- `CharToU64[value : char] : u64`
  - 返回 Unicode 标量值.
- `U64ToChar[value : u64] : char`
  - 由标量值构造字符; 非法标量值报错.
- `CharToU32[value : char] : u32`
  - 返回 Unicode 标量值.
- `U32ToChar[value : u32] : char`
  - 由标量值构造字符; 非法标量值报错.

需要比较字符大小时, 用 `LessU64` 等比较转换后的标量值; 不单设 char 排序函数.

## 3. 字符谓词

- `IsWhitespace[value : char] : bool`
  - 按 Unicode White_Space 判定.
- `IsAsciiDigit[value : char] : bool`
  - 判定 ASCII 的 0 到 9.
- `IsLetter[value : char] : bool`
  - 按 Unicode Alphabetic 属性判定.
- `IsUppercase[value : char] : bool`
  - 按 Unicode Uppercase 属性判定.
- `IsLowercase[value : char] : bool`
  - 按 Unicode Lowercase 属性判定.
- `IsAlphanumeric[value : char] : bool`
  - 按 Alphabetic 或 Numeric 属性判定.

`IsAsciiDigit` 只认 ASCII 数字, 与整数解析函数一致, 不提供 Unicode 数字谓词.
Unicode 数字对 `IsAlphanumeric` 为真.

## 4. 编解码

编解码函数只处理 UTF-8, 不做其他编码的隐式转换.

- `EncodeUtf8[text : string] : List[u8]`
  - 编码为 UTF-8 字节; 不会失败.
- `DecodeUtf8[bytes : List[u8]] : Result[string, Utf8Error]`
  - 解码 UTF-8; 非法序列返回 `Utf8Error/Invalid[offset]`,
    结尾不完整返回 `Utf8Error/Incomplete[offset]`, 偏移按字节计.

## 5. 解析

解析错误是内置 Data 类型 `NumberParseError`, 变体为:
`Empty[]`, `Invalid[]`, `OutOfRange[]`.

- `ParseI64[text : string] : Result[i64, NumberParseError]`
  - 解析 i64; 空串为 `Empty`, 非法字符为 `Invalid`, 范围溢出为 `OutOfRange`.
- `ParseI32[text : string] : Result[i32, NumberParseError]`
  - 解析 i32; 空串为 `Empty`, 非法字符为 `Invalid`, 范围溢出为 `OutOfRange`.
- `ParseU32[text : string] : Result[u32, NumberParseError]`
  - 解析 u32; 空串为 `Empty`, 非法字符为 `Invalid`, 范围溢出为 `OutOfRange`.
- `ParseU64[text : string] : Result[u64, NumberParseError]`
  - 解析 u64; 空串为 `Empty`, 非法字符为 `Invalid`, 范围溢出为 `OutOfRange`.
- `ParseF64[text : string] : Result[f64, NumberParseError]`
  - 解析 f64; 空串为 `Empty`, 非法格式为 `Invalid`.
    接受 `inf`, `infinity` 与 `NaN`, 浮点溢出不是错误, 按 f64 规则解析为无穷.
- `ParseF32[text : string] : Result[f32, NumberParseError]`
  - 解析 f32; 空串为 `Empty`, 非法格式为 `Invalid`.
    接受 `inf`, `infinity` 与 `NaN`, 浮点溢出不是错误, 按 f32 规则解析为无穷.
