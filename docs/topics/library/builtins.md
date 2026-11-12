---
updated: 2026-11-24
---

# ksl 内置能力总览

内置函数与特殊形式的总入口. 命名约定, 保留名与类型记号在此.
语言语义见 `docs/topics/language/` 下的各篇.

## 1. 命名约定

- 数值运算与比较按 `<操作><类型>` 命名.
  类型取 `I64`, `I32`, `U8`, `U32`, `U64`, `F32`, `F64`,
  例如 `AddI64`, `AddI32`, `SubI64`, `NegI64`, `MulI64`, `LessF64`, `EqualString`.
- 操作部分取短名, 例如 Add, Sub, Neg, Mul, Abs, Bit.
- 数学函数同时提供 f32 与 f64 版本, 以 `F32` 或 `F64` 结尾,
  例如 `SinF32`, `CosF64`, `ArcTan2F64`, `PowerF32`.
  只接受浮点输入并返回 i64 的取整函数同样带后缀, 例如 `RoundF64`, `TruncateF32`.
- 需要区分语义的操作使用全称, 例如 `Divide` 向下取整除, `Quotient` 向零截断,
  `Remainder` 带号余数, `Modulo` 取模.
- 位运算以 `Bit` 开头, 例如 `BitOrI64`, `BitReverseU8`; 移位以 `Shift` 开头, 例如 `ShiftLeftU8`.
- 容器与字符串操作为 `<族><动词>` 前缀, 例如 `StringSplit`, `StringJoin`, `StringTrim`, `StringLength`.
  返回 bool 的空判定为 `Is<容器>Empty`, 例如 `IsListEmpty`, `IsDictEmpty`, `IsSetEmpty`.
- 编解码为 `Encode` 与 `Decode`, 例如 `EncodeUtf8`.
- 值转换为 `<源类型>To<目标类型>`, 例如 `U64ToChar`, `I64ToF32`, `F64ToF32`.
- I/O 为动词开头, 例如 `ReadFile`, `WriteString`, `Print`.
- 不使用 `Div`, `Eq`, `Lt`, `Concat` 一类缩写.

## 2. 保留名

- 内置函数名, 特殊形式名, 字面关键字与内置类型名都是保留名,
  在值位置与类型位置都不允许用户覆盖. 中文及其他 Unicode 标识符不受影响.
- 内置 Data 类型的变体名不保留, 通过类型限定名区分, 例如 `FileError/NotFound`.
- 内置类型名: i64, i32, u8, u32, u64, f32, f64, char, bool, string, List, Dict, Set, Pair, Option,
  Result, Pid, Handler, File, CommandOutput, DateTime, Socket, TcpListener, Regex, RegexMatch.
- 内置 Data 类型名: ExitReason, FileError, InputError, NumberParseError, Utf8Error,
  SocketError, TimeError, KeyEvent, RegexError, CommandInput, CommandOption, CommandStatus,
  CommandError.
- 字面关键字: true, false, nil.

## 3. 类型记号

- 签名写作 `名字[参数名 : 类型, ...] : 返回类型`.
  内置函数不加 `@`, 特殊形式加 `@`.
- 参数名只用于文档, 不是语言的一部分.
- 无参数时写作 `名字[] : 返回类型`.
- 可重复参数写作 `参数名... : 类型`; 类型各不相同时只写 `参数名...`.
- 特殊形式的个别操作数只表示语法形状, 可以省略类型.
- 类型变量使用大写单字母: `T`, `U` 表示通用类型;
  `K`, `V` 在字典语境表示键与值;
  `A`, `B` 表示通用配对的两个分量; `E` 与 `F` 表示错误; `R` 表示 Actor 返回值;
  `T0..Tn-1` 表示信箱类型; `A1..Ak` 表示 Handler 状态参数.
  类型变量仅用于文档, 实际使用时类型必须具体.
- 内置函数的签名由实现注册表导出.
- 特殊形式不是函数, 其签名是约定写法,
  表达操作数形状与结果类型, 不由实现导出.
  例如 `@Let[target, value] : nil`.

## 4. 目录索引

### 4.1 数值

- 算术与取整: `docs/topics/library/builtins-arithmetic.md`.
- 数学函数: `docs/topics/library/builtins-math.md`.
- 位运算与移位: `docs/topics/library/builtins-bitwise.md`.
- 数值转换: `docs/topics/library/builtins-convert.md`.
- 数值比较与布尔: `docs/topics/library/builtins-compare.md`.

### 4.2 文本

- 字符串与字符: `docs/topics/library/builtins-string.md`.
- 正则: `docs/topics/library/builtins-regex.md`.

### 4.3 容器

- 列表与配对: `docs/topics/library/builtins-list.md`.
- 字典与集合: `docs/topics/library/builtins-dict-set.md`.
- Option 与 Result: `docs/topics/library/builtins-option-result.md`.

### 4.4 系统

- 输入输出: `docs/topics/library/builtins-io.md`.
- 时间与日期: `docs/topics/library/builtins-time.md`.
- 网络: `docs/topics/library/builtins-network.md`.

### 4.5 语法

- 特殊形式: `docs/topics/library/special-forms.md`.
