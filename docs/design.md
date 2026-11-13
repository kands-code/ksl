---
updated: 2026-11-14
---

# ksl 语言设计

本文档描述 ksl 的设计. 未实现的方向与待确认问题不在本文档中, 见 `notes/roadmap.md`.

全文沿用的缩写: AST 为抽象语法树, CPS 为续延传递风格, IR 为中间表示, LSP 为 Language Server Protocol.

## 1. 定位

- 个人娱乐项目, 以作者需求为最高标准.
- 通用语言, 定位参照 Python 式命令式脚本语言, 不追求函数式特征.
- 运行模式以脚本为主, REPL 为辅, 不以交互为主要使用方式.
- 采用静态类型, 用户书写类型标注, 全部单态, 不允许多态, 不引入 trait. Data 类型覆盖联合类型.
- 实现使用 Rust, 使用第三方库.

## 2. 语法核心

- 统一前缀调用: 一律写作 `f[x, y]`, 方括号是唯一调用括号.
- 后缀调用: 任意表达式后可跟任意多 `[参数]` 对方括号, 每对都是完整调用, 不是柯里化; 被调先求值, 参数从左到右. 只有函数类型的值可调用, 非函数与 never 被调在类型检查期报 `NotCallable`, 语法层不做限制, 由检查器把关.
- 不保留中缀表达式, 无优先级与结合性, 无分组括号; 圆括号只作配对记号的定界符.
- 特殊形式以 `@` 前缀区分, 例如 `@If[cond, then, else]`.
- 特殊形式由解释器硬编码分派, 不引入属性机制.
- 不支持元编程, 不提供 quote.
- 注释: `#` 引导行注释, 至行尾结束, 不引入块注释.
- 顶层形式以 `;` 分隔; 最后一条形式可省略分号, 其值作为程序值, REPL 直接展示, `run` 静默丢弃. 声明形式产生 nil. `@Block` 内的语句以逗号分隔, 块的值取最后一条语句. `run` 丢弃尾表达式时仍按常规类型检查, 只是不保存结果.
- 逗号分隔列表不接受尾逗号; 元素之间才允许逗号, 最后一个元素后直接写闭符号.
- 标识符: 首字符为 Unicode XID_Start, 或一个以上下划线后接 XID_Start, 后续字符为 XID_Continue; 纯下划线序列不是标识符, 而是特殊的丢弃记号 `_`. 内置函数名, 特殊形式名, 字面关键字与内置类型名保留. 内置 Data 类型的变体名不保留, 通过类型限定名区分.
- 类型提升记号: `^类型名` 把类型引入值位置, 用于 `@Export` 与 `@Use` 的类型条目; 重命名条目写作配对形式. `@Spawn` 的信箱类型由 Handler 的 Pid 类型决定, 不使用类型提升.
- 配对记号: `(a, b)` 为通用配对记号, 表达一个二元值; `@Match` 分支与 `@Use` 重命名同用此形.
- 条件分支: `@If[cond, then, else]`, 条件必须为 bool; 以 Haskell 记号表示为 `If :: bool -> a -> a -> a`, 两分支与结果共享同一类型变量 `a`. 有期望时结果先与期望统一; 两分支体统一到 `a`; never 不约束 `a`; 统一失败或检查结束仍有未定变量时报错.
- 匹配分支: `@Match[值, {(模式, 结果), ...}]`; 字段类型由被匹配值的类型决定. 各分支体统一到同一结果类型变量; 有期望时结果先与期望统一; never 不约束结果; 分支顺序不影响类型; 统一失败或仍有未定变量时报错.
- 映射与集合由 `Dict` 与 `Set` 承担, 见 4.6 节.
- 模块采用文件式组织, 通过 `@Export` 与 `@Use` 导出导入; 具体语义见第 14 节.

## 3. 求值模型

- 常规调用采用传值调用, 即 call-by-value, 参数从左到右求值后进入函数.
- 特殊形式直接接收抽象语法树的节点, 由其实现内部决定参数求值时机.
- 用户层面没有阻止求值的手段, 未求值参数的能力完全封闭在解释器内部.
- 不允许引用未定义内容, 不支持递归值定义.
- 全语言值语义: 绑定不可变, 绑定一旦建立不再改变, 状态的更新一律以生成新值表达, 语言不提供修改已绑定值的手段; 由此结构上不可能出现别名, 只有一种拷贝规则, 不存在浅拷贝与深拷贝的区分; 实现层使用持久化数据结构.
- 效果与数据的界限: 不可变只约束用户数据, 系统提供的效果原语, 包括输出, 文件与消息, 不受此约束.
- 不提供共享可变状态原语: 多个 actor 共享同一份可变状态的需求, 以单一 actor 持有状态, 其余 actor 发消息查询与更新实现, 即单点所有者.
- 值层面无环: 列表与 Data 值在类型约束与值语义下不可能成环. 闭包自引用与 Actor 句柄可能形成引用环; 运行时采用 Arc 引用计数加显式环回收, 只回收引用计数无法释放的环.

## 4. 数据层

### 4.1 类型与字面值

- 数值类型: i64, u8, u64, f64.
- 字符遵循 Unicode 标量值, 编码为 UTF-8, 对应 Rust 的 char. 字符不直接参与数值运算.
- 布尔值: true 与 false.
- string 类型: 独立类型, 区别于 `List[char]`; 由 `StringToChars` 函数拆分为字符列表, 由 `CharsToString` 函数合并.
- 字符串操作: `StringLength` 返回 Unicode 标量数量; `StringSplit[s, sep]` 按非空分隔符拆分并保留空片段, 分隔符为空时返回只含原串的单元素列表; `StringLines` 按 `\n` 与 `\r\n` 分行且不产生末尾空行; `StringWords` 按 Unicode 空白拆词并丢弃空片段; `StringTrim`, `StringTrimStart`, `StringTrimEnd` 去除 Unicode 空白; `StringStartsWith`, `StringEndsWith`, `StringContains` 做前后缀与子串判断; `StringUppercase`, `StringLowercase` 做 Unicode 大小写转换; `StringJoin` 连接 `List[string]`; 需要分隔符时先用 `@ListRiffle[list, sep, 1U]` 插值.
- UTF-8 编解码: `EncodeUtf8[string] -> List[u8]` 不会失败; `DecodeUtf8[List[u8]] -> Result[string, string]` 在非法 UTF-8 时返回错误消息. 命名明确绑定 UTF-8, 不提供其他编码的隐式转换.
- 列表类型 `List[T]`, 字面量使用花括号 `{1, 2, 3}`, 元素必须同构, 异质列表在类型检查时报错.
- 字典类型 `Dict[K, V]` 与集合类型 `Set[T]`; 键与集合元素限 i64, u8, u64, char, bool 与 string, 不支持 f64, Data 或容器类型.
- 整数字面值默认为 i64; 带 `u` 后缀且值在 u8 范围内为 u8, 带 `U` 后缀且值在 u64 范围内为 u64, 超出对应范围报错. 期望类型不改变无后缀整数的类型, u8 与 u64 位置必须显式写对应后缀.
- 十六进制与二进制字面值带 `0x`/`0X`, `0b` 前缀, 十六进制数字大小写均可; 允许 `u` 与 `U` 后缀, 无符号类型无负值, 负号配无符号后缀非法, 值域检查与前述规则相同.
- 一元正负号为字面值前缀且必须紧贴数字, `-12` 合法, `- 12` 非法.
- 指数允许小写 `e` 与大写 `E`; 含小数点或指数的数值字面值为 f64.
- 类型标注是期望类型: 站在表达式位置先看它应该是什么类型, 再看它实际是什么类型. 期望来源: `@Let` 标注, `@Fn` 与 `@Def` 的参数与返回标注, 内置与用户函数的形参类型, `Data` 字段声明; 期望穿透进复合表达式, 包括 `@If` 两支, `@Match` 各分支体, `@Block` 末句, 列表元素与调用实参.
- 期望的具体作用: 整数字面量的类型由后缀决定, 不随期望改变; 浮点字面量只在 f64 期望下成立; 列表字面量的元素按期望的元素类型检查; Pair 字面量的两个分量按期望的分量类型解释; 内置泛型构造器从期望取未定的类型参数.
- 字面量的类型由后缀与形态直接确定; AST 节点保留字面量形态, 不因期望改写. 运行时只消费降级后的 IR 与类型制导信息, 不携带按输入保存的类型表.
- 无期望且无上下文时值必须自证其型: 整数字面量默认 i64, 写 `u` 后缀即 u8, 写 `U` 后缀即 u64; `Option/Some` 的元素类型由实参决定; 类型无法由值自身确定者, 包括 `Option/None[]`, `Result/Ok[x]` 的错误类型, `Result/Err[e]` 的成功类型与空列表, 若无上下文定型则报错并要求补标注. 上下文包括标注, 调用实参类型, 列表元素与其他分支的结果类型.
- `@Let[op : Option[i64], Option/None[]]` 是显式标注方式; 有上下文时类型也可由上下文确定, 仍无法定型时报错并要求补标注.
- nil 是正常值, 可以绑定, 存入列表与 Data, 作为参数, 作为返回值; 不提供 `EqNil` 或其他专门处理 nil 的函数.

### 4.2 数值运算与比较

- 运算函数全部单态化, 按类型独立命名, 例如 `AddI64`, `AddF64`, `AddU8`, `AddU64`.
- 不允许任何隐式类型转换, 数值提升与数值到文本的转换一律要求显式.
- 整数运算溢出, 无论上溢还是下溢, 一律报运行时错误; u8 与 u64 的下溢同样报错.
- f64 运算遵循 IEEE 754: 溢出得无穷, 除零得无穷或 NaN, 不报运行时错误; NaN 的相等比较为假.
- `DivideI64` 为向下取整除法, `QuotientI64` 为向零截断除法; 无符号类型只有 `Divide` 与 `Modulo`.
- `Round` 采用银行家舍入.
- 数值补充函数: `AbsI64 :: i64 -> i64` 与 `AbsF64 :: f64 -> f64` 按类型区分; `Power :: f64 -> f64 -> f64` 支持小数指数, `Sqrt :: f64 -> f64`; 三角函数与双曲函数仅提供 f64 版本, 不加类型后缀: `Sin`, `Cos`, `Tan`, `ArcSin`, `ArcCos`, `ArcTan`, `ArcTan2`, `Sinh`, `Cosh`, `Tanh`, `ArcSinh`, `ArcCosh`, `ArcTanh`. `ArcTan2[x, y]` 首参为 x, 次参为 y, 等价于常规 `atan2(y, x)`. 定义域外的 f64 结果遵循 IEEE 754, 生成 NaN 或无穷而不报运行时错误; `AbsI64` 溢出仍报整数溢出.
- 移位为全函数且满足结合律: 位数达到或超过字宽时结果为 0; `ShiftRightI64` 为逻辑右移, 高位补 0, 实现上把位模式重解释为无符号数再移回; 挤出的位按移位的固有语义静默丢弃, 不属于溢出报错范围.
- 相等性按类型提供: `EqualU8`, `EqualU64`, `EqualI64`, `EqualF64`, `EqualChar`, `EqualString`, `EqualBool`; 列表与 Data 不提供内置相等性, 由用户自行定义.
- 大小比较按类型提供, 覆盖 u8, u64, i64, f64, 命名为 `Less`, `LessOrEqual`, `Greater`, `GreaterOrEqual` 加类型后缀, 例如 `LessI64`, `GreaterOrEqualF64`.
- 布尔函数 `Not`, `And`, `Or`, 均为严格函数, 参数与返回值均为 bool; 无短路语义.
- `CharToU64[char] -> u64` 取标量值; `U64ToChar[u64] -> char` 构造字符, 非法标量值直接报错. 标量值为非负整数, 使用 u64; char 的大小于要时用 `LessU64` 等比较转换后的标量值, 不单设 char 排序函数.
- 字符谓词: `IsWhitespace[char] -> bool` 按 Unicode White_Space 判定; `IsAsciiDigit[char] -> bool` 判定 ASCII 的 0-9, 与 `ParseI64` 只认 ASCII 数字一致, 不提供 Unicode 数字谓词; `IsLetter[char] -> bool` 按 Unicode Alphabetic 属性判定, `IsUppercase[char] -> bool` 与 `IsLowercase[char] -> bool` 按 Uppercase 与 Lowercase 属性判定, `IsAlphanumeric[char] -> bool` 按 Alphabetic 或 Numeric 属性判定. 因此 Unicode 数字对 `IsAsciiDigit` 为假, 对 `IsAlphanumeric` 为真.

### 4.3 文本与输出

- 输出统一为格式化字符串, 不提供按类型的打印函数, 不提供 `Inspect`.
- 格式化字符串: `f"..."` 形式, 花括号内仅允许变量或字面值, 不允许其他表达式; 转义使用 `{{` 与 `}}`; 其余细节与 Rust 相同.
- 格式参数文法为 `[[fill]align][sign][#][0][宽度][.精度][类型]`. 宽度上限为 256, 精度上限为 24, 单位均为十进制字符数, 超限报 `InvalidFormatSpec`.
- 类型字符按值类型限定: i64, u8 与 u64 支持 `x`, `X`, `b`; f64 支持 `e`, `E`; string 与 char 支持 `?`. 不支持八进制. 显式写冒号但参数为空, 例如 `f"{s:}"`, 报错; 默认输出写 `f"{s}"`.
- 可打印类型为基础值: i64, u8, u64, f64, bool, char, string, nil. Pid 可作为顶层值打印, 但不作为 List 与 Pair 的基础元素. List 与 Pair 仅当元素或分量为上述基础值时允许打印, 且不支持嵌套, 例如 `List[i64]` 与 `Pair[i64, string]` 允许, `List[List[i64]]` 与 `List[Pair[i64, string]]` 不允许. List 与 Pair 不接受格式参数.
- Data, Option, Result, Dict, Set, File 与 DateTime 不可打印. 需要打印这些类型时, 用户自行实现格式化函数.
- 函数, Handler 与闭包可显示签名: 匿名闭包为 `Fn[[参数类型] : 返回类型]`, 具名函数为 `函数名 : Fn[[参数类型] : 返回类型]`, 内置函数同理, Handler 为 `Handler[[参数类型] : 返回类型]`. 特殊形式不是值, 不参与打印.
- 嵌入规则: f-string 中顶层 string 默认省略引号, `{s:?}` 显式要求引号; 顶层 char 默认不带单引号, `{c:?}` 显示单引号; bool 原样打印; List 与 Pair 内的字符串和字符始终带引号.
- 默认渲染: u8 输出带 `u` 后缀, 例如 `42u`; u64 输出带 `U` 后缀, 例如 `42U`; f64 默认保证带小数点或指数, 例如 `1.0`; Pid 输出 `<Pid 1>`, 其中的编号与 `@PidSerial` 一致.
- REPL 结果展示中 string 与 char 始终带引号, 例如 `1= "hello"` 与 `2= 'a'`; f-string 的 raw 规则不受影响.
- REPL 遇到不可打印类型时使用尖括号摘要, 例如 `1= <Shape>` 或 `1= <lib/math/Shape>`; 类型名按打印位置的名义名显示. 同一真实类型有多个可见别名时, 取声明或导入顺序中的第一个.
- 原始字符串: 与 Rust 相同, 使用 `#` 界定, 例如 `r"32131"`, `r##"12#121"##`.
- 输出: `Print[string]` 写出到标准输出并补一个换行, 返回 nil; `PrintRaw[string]` 写出到标准输出且不补换行, 返回 nil, 用于终端控制序列一类需要精确控制落笔位置的情形; `PrintError[string]` 写出到标准错误, 返回 nil, 不自动追加换行. 三者经 IO 服务异步且有序写出, 服务在应答输入请求前先冲刷已有输出, 因此先提示再读输入的程序能立刻看到提示; 同一发送者的输出顺序由信箱语义保证, 终端上不滞留.
- 标准输入: `ReadLine[提示 : string] -> Result[Option[string], IOError]`; 提示先原样写出到标准输出并冲刷, 不补换行, 提示为空串时只读. 标准输入与标准输出都是终端时复用内置行编辑, 支持行内移动, 删除, 杀/粘贴与 Ctrl-C/D; 否则按行读取, 不做行编辑. `ReadLinePlain[提示 : string]` 使用相同的结果类型与提示语义, 但始终按行读取, 不做行编辑, 适用于同时接收异步输出或希望交给终端自身行编辑的程序; 其 Ctrl-C 交给终端行规程处理, 不返回 `Interrupted`. `Ok[Some[line]]` 为一行且不含行尾换行, `Ok[None[]]` 为 EOF, 终端行编辑下 Ctrl-C 为 `Err[IOError/Interrupted[]]`, 其他读取失败为 `Err[IOError]`.
- 终端按键: `ReadKey[] -> Result[KeyEvent, IOError]`; 标准输入不是终端时返回 `Err[IOError/NotATerminal[]]`. 读取期间在必要时临时开启终端原始模式, 返回前恢复, 调用方不管理终端模式. 只返回按下与重复的按键, 松开事件, 鼠标, 焦点, 尺寸与未建模的按键被跳过. 原始模式下 Ctrl-C 作为 `Ctrl['c']` 返回, 不再产生中断信号. `ReadKey` 与 `ReadLine` 在终端模式下共用 crossterm 事件源, 可以交替调用; 标准输入不是终端时 `ReadKey` 返回 `NotATerminal`.
- 按键表示: 内置 Data 类型 `KeyEvent`, 变体为 `Char[char]`, `Ctrl[char]`, `Alt[char]`, `F[u8]`, `Enter[]`, `Escape[]`, `Backspace[]`, `Tab[]`, `BackTab[]`, `Up[]`, `Down[]`, `Left[]`, `Right[]`, `Home[]`, `End[]`, `PageUp[]`, `PageDown[]`, `Delete[]`, `Insert[]`. 修饰键折进变体, Ctrl 与 Alt 同时按下时按 Ctrl 处理; 字母的 Shift 已体现在字符大小写中, 其余 Shift 组合不单独区分.
- 文件 I/O: 提供整体文本操作 `ReadFile[string] -> Result[string, IOError]`, `WriteFile[string, string] -> Result[nil, IOError]` 创建或截断后写入, `AppendFile[string, string] -> Result[nil, IOError]` 创建或追加; UTF-8 无效由 `IOError/InvalidUtf8[]` 表达.
- 二进制整体 I/O: `ReadFileBytes[string] -> Result[List[u8], IOError]` 读取完整文件字节, `WriteFileBytes[string, List[u8]] -> Result[nil, IOError]` 创建或截断后写入, `AppendFileBytes[string, List[u8]] -> Result[nil, IOError]` 创建或追加; 二进制路径不做 UTF-8 校验.
- 目录 I/O: `ListDirectory[string] -> Result[List[string], IOError]` 返回按名字排序的条目名, 不含 `.` 与 `..`, 不递归; `CreateDirectory[string]` 创建单层目录, `CreateDirectoryAll[string]` 递归创建; `RemoveDirectory[string]` 只删除空目录, `RemoveFile[string]` 删除文件; `RenamePath[string, string] -> Result[nil, IOError]` 与 `CopyFile[string, string] -> Result[nil, IOError]` 按宿主文件系统语义执行; `JoinPath[string, string] -> string` 拼接路径.
- 路径谓词: `IsPathExists[string] -> bool`, `IsFile[string] -> bool`, `IsDirectory[string] -> bool` 只做判断, 路径不存在或查询失败时返回 false; 谓词不保证后续操作成功, 路径可能在此期间变化, 实际操作仍须处理 `IOError`.
- 当前目录: `GetCurrentDirectory[] -> Result[string, IOError]` 返回宿主当前工作目录的绝对路径; 查询失败返回 `IOError/Other[]`. 不提供 `SetCurrentDirectory`.
- 文件流: 内置不透明类型 `File`; `OpenRead[string] -> Result[File, IOError]`, `OpenWrite[string] -> Result[File, IOError]` 截断或创建, `OpenAppend[string] -> Result[File, IOError]` 追加或创建; `Close[File] -> Result[nil, IOError]`, `Flush[File] -> Result[nil, IOError]`.
- 流式读写: `ReadLineFrom[File] -> Result[Option[string], IOError]` 按行读取文本, `ReadChunk[File, u64] -> Result[Option[List[u8]], IOError]` 按字节块读取, 块大小为正数; `WriteString[File, string] -> Result[nil, IOError]` 与 `WriteBytes[File, List[u8]] -> Result[nil, IOError]` 写入; EOF 为 `Ok[None[]]`.
- 文件句柄方向在打开时确定; 读句柄写入返回 `IOError/NotWritable[]`, 写句柄读取返回 `IOError/NotReadable[]`; 关闭或无效句柄上的操作返回 `IOError/InvalidHandle[]`. 句柄可复制, 关闭影响所有副本; 忘记 `Close` 时由宿主释放资源, 但写缓冲的刷新时机不保证, 长流程应显式 `Flush` 或 `Close`.
- 空文件句柄: `ClosedFile[] -> File` 给出处于关闭态的句柄, 任何读写都报 `IOError/InvalidHandle[]`. 供必须给出 `File` 值而手上没有可用句柄的场合使用, 例如 `ResultGet` 的默认值.
- I/O 错误: 内置 Data 类型 `IOError`, 变体为 `NotFound[path]`, `PermissionDenied[path]`, `AlreadyExists[path]`, `InvalidUtf8[path]`, `NotADirectory[path]`, `IsADirectory[path]`, `DirectoryNotEmpty[path]`, `InvalidHandle[]`, `NotReadable[]`, `NotWritable[]`, `NotATerminal[]`, `Interrupted[]`, `Other[message]`. 文件与标准输入的可恢复失败统一走 `Result[T, IOError]`.
- I/O 路径: 文件与目录操作按宿主当前工作目录解释相对路径; 模块 `@Use` 路径仍相对模块文件目录, 二者不混用.
- I/O 执行: 输入输出由全局唯一的系统 Actor 承担, 见第 13 节; 文件, 目录与其他阻塞操作由固定四个工作线程执行, 当前任务挂起, 完成后由调度器唤醒, 不会阻塞其他 Actor. 标准输入由服务的读取线程持续读取并缓冲: 终端模式走 crossterm 事件, 管道与文件模式按行缓冲, 缓冲上限 4 KB, 按未消费字节计, 单行自身超过上限时允许超限读完; 输入请求从缓冲消费, 取消不丢行, 已消费未交付的输入退回缓冲. 终端所有权归服务单点: raw mode, crossterm 事件源与 bracketed paste 均由服务管理.
- 流与迭代器: 提供 `File` 句柄与按行/按块读取.
- 环境: `Args[] -> List[string]` 取命令行参数, 无参数时为空列表; 启动后参数由全部任务共享, Actor 内读取结果与程序根一致; `GetEnv[name] -> Option[string]` 取环境变量, 未设置时返回 `Option/None[]`.
- 解析: `ParseI64[string] -> Result[i64, string]`, `ParseF64[string] -> Result[f64, string]`, 错误负载为消息原文; 解析错误无结构化需求时直接用 string. I/O 使用独立的 `IOError` Data 类型.
- 程序退出: `@Exit[code : u8]` 是特殊形式而不是表达式, 类型为 `Never`, 终止程序并以指定码退出; 退出码写作 u8, 例如 `@Exit[0u]`; 退出码 0 表示正常, 1 表示错误终止, 其余由程序自定义, Unix 约定为 0 命中, 1 未命中, 2 出错, 便于脚本参与管道与条件执行; 退出消息仍由 `Print` 负责, 两者职责分离.
- `@Exit` 允许作为顶层形式或出现在 `@Main` 体内, 可以作为最后一条形式而省略分号; 不得出现在其他嵌套位置, 包括 `@Block`, 函数体, `@Def`/`@Fix` 与 `@Handler`. 普通 Actor 终止自身使用 `@Halt`; `@Main` 禁止 `@Halt`, 正常结束或 `@Exit` 结束程序.

### 4.4 列表操作

- 下列操作全部为特殊形式, 其类型含类型变量, 无法在语言内书写为函数类型. 文档中允许以泛型记号描述, 例如 `Filter :: List[a] -> fn[[a], bool] -> List[a]`, 仅作文档用途, 实际使用时类型必须具体.
- 列表类型记号 `List[a]`, 例如 `@Let[a : List[char], {'a', 'b'}]`; 列表操作与配对访问的类型含类型变量, 以文档泛型记号描述.
- 元素访问: `@ListGet[list, index]`, `@ListUpdate[list, index, value]`, 下标类型为 u64, 越界报错, 无负索引.
- 变更: `@ListUpdate[list, index, value]`, `@ListPrepend[list, value]`, `@ListAppend[list, value]` 均返回新列表, 类型为 `List[T] -> List[T]`; 绑定不可变, 故不存在原地修改与回存, 旧值不受影响.
- 拆分: `@ListHead[list]` 对空列表报错, `@ListTail[list]` 对空列表返回空列表, `@ListTake[list, n]`, `@ListDrop[list, n]`, `@ListTakeWhile[list, fn]`, `@ListDropWhile[list, fn]`.
- 遍历: `@ListMap[list, fn]`, `@ListMapIndexed[list, fn]` 以 u64 下标从 0 开始映射, `@ListFilter[list, fn]`, `@ListFold[list, fn, init]`, `@ListPrefixFold[list, fn, init]` 返回含初始值的前缀累积列表, `@ListAny[list, fn]`, `@ListAll[list, fn]`.
- 组合: `@ListRiffle[list, elem, n]` 每隔 n 个元素插入一次元素, n 为 1 时即相邻元素间插入, 间隔超过列表长度返回原列表, 间隔为 0 报错; `@ListWindow[list, n]` 滑动窗口分组, 窗口大于长度返回空列表, 窗口为 0 报错; `@ListZip[list1, list2]` 配对压缩, 长度不一致时截短, 返回 `List[Pair[K, V]]`; `@ListZipWith[list1, list2, fn]` 按函数合并两个列表, 长度不一致时截短.
- 构造与重排: `@ListReverse[list]` 反转; `@ListRepeat[value, n]` 重复构造, n 为 0 返回空列表; `@ListFlatten[list]` 把 `List[List[T]]` 摊平为 `List[T]`; `@ListChunk[list, n]` 按 n 个元素分组, n 为 0 报错, 最后一块可不足 n.
- 配对访问: `@Fst[pair] -> K`, `@Snd[pair] -> V`, 为特殊形式, 因 `@Match` 限定于 Data.
- 度量: `@ListLength[list] -> u64`.
- 拼接: `@ListConcatenate[list1, list2]`.
- 语义与实现分离: 变更走持久化结构的路径复制, 新旧列表共享大部分节点; 由于绑定不可变, 这种共享对程序不可见.
- 参数个数固定, 不足或多余报 `ArityMismatch`.

### 4.5 Option 与 Result 的便捷操作

- `Option` 与 `Result` 的消费以 `@Match` 为主, 见第 8 节; 便捷操作是特殊形式, 其类型含类型变量, 无法在语言内书写为函数类型; 下文类型记号中的 `T`, `U`, `E` 仅作文档用途, 实际使用时类型必须具体.
- 判定: `@IsSome[opt] -> bool`, `@IsNone[opt] -> bool`, `@IsOk[res] -> bool`, `@IsErr[res] -> bool`.
- 映射: `@OptionMap[opt, fn] -> Option[U]`, 其中 fn 为 `Fn[[T] : U]`; `@ResultMap[res, fn] -> Result[U, E]`, 其中 fn 为 `Fn[[T] : U]`; `None` 与 `Err` 原样传回.
- 绑定: `@OptionAndThen[opt, fn] -> Option[U]`, 其中 fn 为 `Fn[[T] : Option[U]]`; `@ResultAndThen[res, fn] -> Result[U, E]`, 其中 fn 为 `Fn[[T] : Result[U, E]]`; `None` 与 `Err` 原样传回.
- 取值: `@OptionGet[opt, 默认值] -> T`, 无值时取默认值; `@ResultGet[res, 默认值] -> T`, 错误负载被丢弃. 默认值惰性求值, 仅在无值或结果为错误时执行.
- 边界: `@Match` 是通用消解器, 用于解构多个变体, 读取错误负载或组合多个 Option/Result; 便捷操作覆盖判定, 映射, 绑定与默认取值. `ResultGet` 丢弃错误负载, 语义同 `unwrap_or`; 需要错误内容时必须用 `@Match`. 映射与绑定接收 `@Fn`, 而 `@Fn` 体内不能使用 Actor 原语, 分支中需要 `@Spawn`/`@Join` 等 Actor 原语时仍用 `@Match`. 不提供 `MapErr`, `OrElse`, `Filter`.
- 参数个数固定, 不足或多余报 `ArityMismatch`.

- 内置函数命名约定: 数值与比较为动词加类型后缀, 例如 `AddI64`, `SubI64`, `MulI64`, `LessF64`, `EqualString`; 数学与编程中作为记号存在者用短名, 例如 `Add`, `Sub`, `Mul`; 承载语义区分者用全称, 例如 `Divide` 向下取整除, `Quotient` 向零截断, `Remainder` 带号余数, `Modulo` 取模; 容器与字符串操作为 `<族><动词>` 前缀, 例如 `StringSplit`, `StringJoin`, `ListMap`, `DictLookup`; 编解码为 `Encode` 与 `Decode`, 例如 `EncodeUtf8`; 值转换为 `FromTo`, 例如 `U64ToChar`; I/O 为动词开头, 例如 `ReadFile`, `WriteString`. 数学标准名 `Abs`, `Sqrt`, `Sin`, `Cos`, `Tan`, `ArcSin` 等保留. `u` 后缀为 u8, `U` 后缀为 u64, 无后缀为 i64. 不使用 `Div`, `Eq`, `Lt`, `Chars`, `Concat` 一类缩写.

### 4.6 Dict 与 Set

- 表示: `Dict[K, V]` 与 `Set[T]` 是不可变持久化有序映射与集合, 内部使用 imbl 的 `OrdMap` 与 `OrdSet`; 遍历按键排序, 结果稳定. 键限制为 i64, u8, u64, char, bool 与 string; 不支持 f64, Data, List, Pair 等.
- 字面量: `@Dict[(键, 值), ...]` 与 `@Set[值, ...]`; 空字面量需要期望类型或标注, 例如 `@Let[d : Dict[string, i64], @Dict[]]`; 重复键在 Dict 中后写覆盖先写.
- Dict 操作: `@DictLookup[dict, key] -> Option[V]`, `@DictGet[dict, key, default] -> V` 在缺键时才求值默认表达式, `@DictPut[dict, key, value] -> Dict[K, V]`, `@DictUpdate[dict, key, fn, default] -> Dict[K, V]` 在缺键时才求值默认值, 命中时以旧值调用 `fn`, `@DictFilter[dict, pred] -> Dict[K, V]` 以 `Fn[[K, V]: bool]` 过滤, `@DictRemove[dict, key] -> Dict[K, V]`, `@DictHas[dict, key] -> bool`, `@DictLength[dict] -> u64`, `@DictEntries[dict] -> List[Pair[K, V]]`, `@DictKeys[dict] -> List[K]`, `@DictValues[dict] -> List[V]`, `@DictMerge[first, second] -> Dict[K, V]` 中后者覆盖同名键, `@DictFromEntries[entries] -> Dict[K, V]`.
- Set 操作: `@SetInsert[set, value] -> Set[T]`, `@SetRemove[set, value] -> Set[T]`, `@SetHas[set, value] -> bool`, `@SetLength[set] -> u64`, `@SetItems[set] -> List[T]`, `@SetFilter[set, pred] -> Set[T]` 以 `Fn[[T]: bool]` 过滤, `@SetUnion[a, b]`, `@SetIntersect[a, b]`, `@SetDifference[a, b]` 返回新集合, `@SetFromList[list] -> Set[T]`.
- Dict 与 Set 不可打印; 需要展示时用 `@DictEntries` 或 `@SetItems` 转为列表, 再按需格式化.

### 4.7 时间与日期

- 不透明类型 `DateTime`: 绝对时刻加时区, 与 `File` 同类, 不可打印, REPL 显示 `<DateTime>`. 精度为毫秒, 与相对时间原语一致; `Now[]` 与 `NowUtc[]` 的返回裁剪到毫秒.
- 命名: 类型为 `DateTime`; 日期组件用 `Date*`, 时刻组件用 `Time*`, 整体值操作用 `DateTime*`, 时区视图用 `TimeZone*`. `TimeError` 是时间域的错误类别, 不是函数前缀.
- 时区语义: 时刻自带命名时区或固定偏移, 字段按自带时区解释. 时区数据由 jiff 提供, Unix 上优先读系统 tzdb, 其余情况回退内嵌副本. 夏令时会使本地时间不存在或出现两次; 构造与解析按兼容策略处理, 空洞前移到第一个有效时刻, 重叠取较早的偏移.
- 取当前时刻: `Now[] -> DateTime` 按宿主本地时区, `NowUtc[] -> DateTime` 按 UTC.
- 字段: `DateYear`, `DateMonth`, `DateDay`, `TimeHour`, `TimeMinute`, `TimeSecond`, `TimeMillisecond` 返回 i64; `DateWeekday` 按 ISO 8601, 1 为周一, 7 为周日.
- 时区操作: `DateTimeInZone[DateTime, string] -> Result[DateTime, TimeError]` 换时区而不改时刻, 时区名不存在报 `TimeError/UnknownZone`; `TimeZoneName[DateTime] -> string` 取 IANA 名, 固定偏移的时区退化为偏移文本例如 `+05:30`; `TimeZoneOffsetMinutes[DateTime] -> i64` 给相对 UTC 的偏移分钟数.
- 构造与解析: `DateTimeFromParts[年, 月, 日, 时, 分, 秒] -> Result[DateTime, TimeError]` 按宿主本地时区构造, 字段越界报 `TimeError/InvalidField`; 需要按指定时区构造时用带 `[时区]` 后缀的 `ParseDateTime`. `ParseDateTime[string] -> Result[DateTime, TimeError]` 对带偏移或 `[时区]` 后缀的输入按其解释, 对不带时区的输入按宿主本地时区解释, 形态不符报 `TimeError/InvalidFormat`.
- 输出与换算: `DateTimeToString[DateTime] -> string` 按 ISO 8601 扩展格式输出, 与 `ParseDateTime` 可互相往返; `DateTimeToEpochMillis[DateTime] -> i64` 取 Unix epoch 毫秒, 1970 年之前为负; `EpochMillisToDateTime[i64] -> DateTime` 返回 UTC 时刻, 超出可表示范围报运行时错误 `DateTimeOutOfRange`. 不提供亚毫秒的取值与构造.
- 比较: `LessDateTime`, `LessOrEqualDateTime`, `EqualDateTime`, `GreaterDateTime` 与 `GreaterOrEqualDateTime` 按绝对时刻比较, 显示时区不参与判断; 同一瞬间的不同时区表示相等.
- 算术: `AddYear`, `SubYear`, `AddMonth`, `SubMonth`, `AddWeek`, `SubWeek`, `AddDay`, `SubDay`, `AddHour`, `SubHour`, `AddMinute`, `SubMinute`, `AddSecond`, `SubSecond`, `AddMillisecond`, `SubMillisecond`, 计数为 u64, 合计超出可表示范围报运行时错误 `DateTimeOutOfRange`. 年月周日按日历解释, 保持本地时间; 时分秒毫秒按精确时长. 二者跨夏令时时不同: 01:30 加一小时得 03:30, 加一天得次日 01:30. 周与日等价, `AddWeek[d, 1U]` 与 `AddDay[d, 7U]` 结果相同.
- 失败分类: 解析字符串与时区名属不可信输入, 返回 `Result[_, TimeError]`; 已定型值上的换算与算术失败属程序错误, 报运行时错误. `TimeError` 为内置 Data 类型, 变体为 `InvalidFormat[消息]`, `UnknownZone[名字]`, `InvalidField[说明]`.
- 相对时间原语: `@Sleep`, `@SetTimer`, `@Receive` 超时与 `@CancelTimer` 以毫秒计, 基于单调时钟, 不换算为日历时间.

## 5. 类型系统

- 静态类型, 检查先于求值, 求值器可信赖类型, 无需运行时类型检查.
- 用户书写类型标注; 绑定标注可省略, 由值的形态确定类型, 标注用于消歧, 例如 u8 与 i64.
- 标注可省略的场合限于值自身类型确定, 或有同一表达式内的上下文可定型的场合: 字面值, `@Fn` 表达式, 构造器调用, 列表元素, 分支结果以及上下文确定类型的特殊形式如 `@Receive`; 不做任何从后续语句出发的推断.
- 全部单态, 不允许多态; 不引入用户可见的类型变量. 检查器内部可以使用类型变量作为实现细节, 但不得泄漏到用户类型或跨检查单元保留. 不引入语言泛型.
- 不引入 trait, 无重载, 无子类型.
- 不引入 Atom 类型.
- 不引入联合类型, 联合由 Data 类型的多个变体表达.
- 不引入类型别名.
- 函数为原生多参, 无柯里化, 实参个数不符即报错; 函数类型表示为 `Fn[[参数类型, ...] : 返回类型]`, 与 `@Fn` 形态同构.
- 一等函数值按具体函数类型传递与调用; 函数层面不允许多态.
- 内置参数化类型构造器为正式类别, 含 `List[a]`, `Pid[T0, ..., Tn-1, R]`, `Handler[Pid[T0, ..., Tn-1, R], A1, ..., Ak]`, `Option[T]`, `Result[T, E]`, `Pair[K, V]`, `Dict[K, V]` 与 `Set[T]`; 内置 Data 类型有 `ExitReason`, `IOError`, `SocketError`, `TimeError`, `KeyEvent` 与 `IORequest`; 不透明类型为 `File`, `DateTime`, `Socket` 与 `TcpListener`; 参数在使用点必须全部具体化; 用户 Data 类型不可参数化. 实现上, 内置参数化类型与用户 Data 统一为类型构造器应用, 不透明类型统一为具名不透明类型, 但仍是全单态, 不引入用户泛型.
- `Pid[T0, ..., Tn-1, R]`: 前 n 个参数依次为信箱 0..n-1 的消息类型, 最后一个参数 R 为 actor 返回值类型; n 至少为 1, 至多 16. 信箱数由类型参数个数确定.
- `Handler[Pid[T0, ..., Tn-1, R], A1, ..., Ak]`: actor 蓝图类型, 第一个参数必须是一个 `Pid` 类型, 后续参数是 Handler 的状态/启动参数类型; Handler 与 `Fn` 是不同类别, 不可作为函数调用; Handler 本身是一等值, 可绑定, 传递, 作为消息字段, 但只能由 `@Spawn` 启动.
- 内置名保留: 内置函数名, 特殊形式名, 字面关键字与内置类型名不得被用户绑定或声明为 Data 类型/变体. 内置 Data 类型的变体名不保留, 通过类型限定名区分. 中文及其他 Unicode 标识符不受影响.
- `Option[T]` 与 `Result[T, E]` 名义上同为 Data, 构造器 `Option/Some`, `Option/None`, `Result/Ok`, `Result/Err` 可用于 `@Match`, 这是消费二者的主要方式.
- nil 为独立类型, 仅 nil 与 nil 兼容; nil 是正常值, 可以绑定, 存储, 传递与返回; 不提供 `EqNil` 或其他专门处理 nil 的函数.
- 底类型 never 为检查器内部形态, 即不返回的表达式, 例如 `@Halt[]` 与 `@Exit[]` 的类型: 与任何类型兼容, 汇合时让位于另一侧; 用户不可书写; never 不得作为子表达式的期望类型.
- 检查器内部可使用类型变量统一未定类型; 类型变量不对用户暴露, 不做泛化, 不改变字面量的后缀规则. 统一表采用 ena; `Type` 只表示具体类型, 推断期的类型变量由独立的 `InferTy` 承载, 绑定环境与函数签名不保留类型变量; 每个顶层输入, 函数体与模块检查结束时必须把类型变量解析为具体类型, 仍未解析则报错.
- 列表元素共享一个元素类型变量: 有外层期望时按期望检查元素; 无外层期望时由元素共同统一. 因此 `{Option/None[], Option/Some[1]}` 与 `{Option/Some[1], Option/None[]}` 都得到 `List[Option[i64]]`; `{1, 2u}` 与 `{1u, 2}` 因元素类型不一致报错. 空列表只能由期望或上下文提供元素类型; 无上下文的 `{}` 与 `{Option/None[], Option/None[]}` 报错.
- 列表操作把外层期望传播到结果形状: `@ListTail`/`@ListTake`/`@ListDrop` 约束列表实参, `@ListHead` 约束 `List[T]`, `@ListMap` 约束函数返回, `@ListFold` 约束初始值与函数返回, `@ListZip` 约束两个列表, `@ListWindow` 约束源列表; 严格字面量规则不受影响.
- 类型变量与期望类型携带源码区间: 未解析变量在创建位置报告, 冲突在当前约束位置报告; 跨函数或模块复用时保留原始区间, 避免定位丢失.
- 检查器为源码节点生成类型表, 值为解析后的具体类型; 类型表供 LSP 与 IR 降级使用, 不进入运行时. 覆盖表达式, 绑定名, 函数名与参数, 模式字段, Data 类型/变体/字段, 类型表达式中的具名类型分量, 以及导入导出条目; Data 变体名记录为构造器签名 `Fn[[字段类型...] : Data]`; REPL 类型表按成功输入合并, 不同源码的 SourceId 不会冲突; 模块类型表随 `LinkedModule` 与模块缓存保存; 失败输入的类型表随会话回滚.
- 类型表以 `(SourceId, NodeId)` 为键, 节点身份在解析期分配, 同一区间的不同节点各自保留身份; `Span` 只用于诊断与定位. 类型表不保留反向查询与源码文本; 源码生命周期由调用方管理, 调用方为 REPL 或 LSP. 参数化类型整体不单独记录, 以内部具名类型分量与表达式类型覆盖. 类型表与检查环境的作用域使用持久化映射: REPL 每轮合并历史类型表, 克隆会话副本或逐条绑定只做结构共享, 不复制已有绑定.
- 类型信息保留在检查器与 IR 中, 用于 LSP, 错误消息与 IR 降级; 运行时 `Value` 不携带完整静态类型, 只保留执行必需的信息, Data 值保留真实类型身份用于匹配. typed IR 采用类型制导降级, 形态见第 10 节.

## 6. 绑定与作用域

- `@Let[名字 : 类型, 值]` 绑定变量; 标注可省略; 新绑定遮蔽外层同名绑定; 顶层形式以分号分隔, 最后一条可省略分号.
- 语句值的处置: 隐式丢弃仅对 nil 类型放行; 非 nil 值的语句必须显式绑定, 或以 `@Let[_, 值]` 显式丢弃; 尾表达式不受此约束, 其值即程序值. `@Block` 内的非末句同样适用本条.
- `@Let[_, 值]` 是显式丢弃: 值按常规检查并执行, 结果值直接丢弃; 空列表与未定类型参数不因丢弃而豁免, 例如 `@Let[_, {}]` 与 `@Let[_, Option/None[]]` 仍报错.
- `_` 为特殊丢弃记号, 不可作为普通标识符引用; 可出现在 `@Let` 的绑定位与 `@Match` 的字段忽略位.
- 绑定只有单绑定一种形态; 并行求值的需求以多值构造与 `@Let` 逐条表达.
- 值绑定, 即 `@Let` 与命名 `@Def`, 可以遮蔽外层同名绑定; 类型声明不可覆盖, 重复声明或使用内置名直接报错.
- `@Block[语句, 语句, ...]`: 作用域块, 语句以逗号分隔, 块的值为最后一条语句的值; 块内的新定义不泄漏到外层. 空 `@Block[]` 的值为 nil.
- 定义与绑定只产生 nil, 可出现在顶层或 `@Block` 的语句位置; `@Data` 仅限顶层; 普通表达式位置不可定义或绑定. 顶层最后一条声明可以省略分号, 值仍为 nil.
- 迭代不使用循环原语: 无 `@While`, 迭代一律以递归表达, 尾递归在 CPS 变换后不增深度.
- 绑定不可变, 故无修改绑定的原语, `@Mutate` 不存在; 状态更新以生成新值并重新绑定表达, 重新绑定即遮蔽, 新旧绑定并存于不同层.

## 7. 函数

- 匿名函数: `@Fn[[x : i64] : i64, 体]`; 参数类型与返回类型必须标注; 多语句体使用 `@Block`; 匿名函数是表达式, 值是函数本身, 不自递归.
- 具名函数定义: `@Def[名字, [x : i64] : i64, 体]`; 它是语句, 在当前作用域绑定函数名, 值为 nil, 默认自递归; 签名本身就是类型来源, 无需额外标注.
- 具名函数与匿名函数只在值位置与递归能力上不同, 类型均为 `Fn[[参数类型...] : 返回类型]`; 同一函数的参数名必须互不相同.
- 允许 `@Let[f, @Fn[[...] : ..., 体]]` 绑定匿名函数; 该函数不能引用 `f`, 因此不自递归. 对非递归函数, `@Let[f, @Fn[...]]` 与 `@Def[f, ...]` 得到相同的函数值与函数类型, 区别仅在名字是否在函数体内可见; 递归定义只有 `@Def`, 独立定义或在 `@Fix` 中, 这一种方式.
- Actor 蓝图: `@Handler[[self : Pid[...], 状态...] : R, 体]` 是表达式, 值类型为 `Handler[...]`; 求值只构造蓝图, 不执行体; 自循环用 `@Continue[新状态...]`, 主动停止用 `@Halt[]`, 不需要 Handler 名字; Handler 不可作为函数调用, 只能由 `@Spawn` 启动. 详细语义见第 13 节.
- 函数默认不允许前向引用; 互递归由 `@Fix` 表达.
- `@Fix[定义, 定义, ...]`: 仅接受具名 `@Def`, 不产生新作用域; 组内名字相互可见, 定义本身进入当前作用域, 在 `@Fix` 之后的同块语句中可用, 随所在块结束而消失. 空 `@Fix[]` 视为 nil. 运行时机制为 `FixCell`: 预分配写一次的共享单元, 闭包捕获单元, 组求值完毕后回填终值.
- `@Fix` 只接受函数定义, 递归值在语法上不可能出现, 不设运行时的未初始化值检查.
- Data 类型的相互引用由声明预收集解决; 值构造保持自底向上, Data 值不可能成环.
- 命名示例: `@Def[addOne, [x : i64] : i64, AddI64[x, 1]];`.
- 互递归示例: `@Fix` 内定义 `isEven` 与 `isOdd`, 彼此在函数体中引用.
- 闭包按值捕获自由变量, 即复制定义时刻的值进入闭包环境, 不共享引用. `@Fix` 使用写一次单元回填; 自引用闭包会形成引用环, 由运行时的显式环回收处理.

## 8. Data 类型与匹配

- 定义: `@Data[Shape, {Point[], Circle[f64], Rectangle[f64, f64]}]`.
- Data 类型至少有一个变体; `@Data[D, {}]` 无法实例化, 报错.
- 类型声明允许前向引用与相互引用, 类型检查阶段先行收集全部声明再做检查, 与语句求值顺序无关.
- 构造与使用: `Shape/Circle[10.2]`; 变体不得与类型重名; 字段类型必须显式书写.
- 单一变体示例: `@Data[Person, {Init[string, u8]}]`, 使用 `Person/Init["Kevin", 24]`.
- 模式匹配: `@Match[值, {(模式, 结果), ...}]`; 每个分支为圆括号包裹的模式与结果对.
- 模式形态: `[类型/]构造器[字段绑定, ...]`; 写明类型时直接写 `(Job/Stop[], 结果)`, 省略类型时以 `/` 开头由被匹配值的类型推断, 例如 `(/Stop[], 结果)`; 两种形态用于区分不同类型同名构造器; 字段绑定为变量名或 `_`.
- 通配模式: `_` 匹配任意变体且不绑定字段, 作为兜底分支; 满足穷尽性, 但其后不可再有分支, 其后分支不可达.
- Data 不提供原地修改, 变更一律由 `@Match` 解构后重建: 复用字段绑定原名, 新字段直接构造; 语义为复制而非引用, 不会构造环. 不设 `@With` 一类的更新语法.
- 示例: 令 `l` 为 `LL/Node[1, LL/Node[2, LL/Nil[]]]`, 则 `@Match[l, {(/Node[x, _], LL/Node[x, l])}]` 的值为 `LL/Node[1, LL/Node[1, LL/Node[2, LL/Nil[]]]]`, `l` 本身不变.
- 匹配必须穷尽, 漏写变体在类型检查阶段报错; 不允许嵌套解构; 不引入守卫模式.
- 示例: 对 `Shape/Circle[3.2]` 的 `@Match` 按 `Point[]`, `Circle[r]`, `Rectangle[w, h]` 分派并绑定字段.
- Data 类型不可打印; 需要输出时由用户实现格式化函数.

## 9. 错误处理

- 统一错误目录: 词法, 语法, 类型检查, 运行时四阶段的错误归入单一目录, 每种错误有稳定编号与规范标题; 编号分段为 `E01xx` 词法, `E02xx` 语法, `E03xx` 类型检查, `E04xx` 运行时; 内部错误, 即实现缺陷, 不编号, 单独渲染.
- 编号按逻辑归类生成, 退役的编号不再复用. 编号由过程宏按阶段基数与声明顺序生成; 阶段与标题由人工维护. 变体在枚举中按逻辑归类排列, 插入新变体时后续编号顺延. 运行时段按类别分组: 用户主动中断, 数值与算术, 容器与索引, 调度与等待, 时间与日期.
- 类型检查阶段区分具体类型冲突与推断期冲突: 具体类型冲突报 `TypeMismatch`, 承载具体类型; 推断期冲突报 `InferMismatch`, 允许把未定类型变量显示为 `_`. 二者同在类型检查段并按逻辑顺序相邻.
- 报告形态: `error[EXXXX]: 标题` 加具体消息与源码片段, 经 annotate-snippets 渲染; 渲染由库保证, 实现只须提供位置与消息.
- 内置错误, 包括类型错误, 溢出与未绑定名字等, 不可恢复, 不可由语言机制捕获; 可能的失败以 `Result` 表达, 可能的缺失以 `Option` 表达. 不设异常机制, `Catch` 与 `Throw` 不存在.
- 错误作用域: 普通 Actor 内的运行时错误只终止该 Actor, 诊断照常输出; 程序根内的运行时错误终止整个程序. 顶层语句是 Actor 之外的 Prelude, `@Main` 是程序的根 Actor, 两者出错都终止程序.
- 退出原因: 内置 Data 类型 `ExitReason`, 变体为 `Halted[]` 主动停止, `Killed[]` 被强杀, `Failed[编号 : i64, 说明 : string]` 运行时错误; 编号为稳定身份, 供程序判别, 说明为给人看的文本, 说明不作为判别依据.
- 领域错误由 Data 值承担, 可失败操作的返回类型使用 `Result[T, E]`. 内置 `IOError` 覆盖文件, 目录与标准输入的常见失败; 其余操作按实际种类扩充. 解析类错误的负载直接用 string, 不设 `ParseError`; 时间族的失败原因另设内置 Data 类型 `TimeError`, 见 4.7 节. `Option` 与 `Result` 的构造器, 即 `Option/Some`, `Option/None`, `Result/Ok`, `Result/Err`, 可在源码中构造, 属内置泛型构造器, 与用户 `Data` 构造器同形.
- 所有任务都在等待且无 I/O 与定时器时, 调度器报告 `Deadlock` 运行时错误.
- 用户主动中断: 四个特殊形式供程序显式声明失败, 信息字符串不可省略. `@Assert[条件 : bool, 信息 : string]` 条件为真则通过并给出 nil, 为假则断言失败; 通过时执行继续, 故其类型不是 never, "不可达分支" 的意图用 `@Unreachable` 表达; `@Panic[信息 : string]`, `@Todo[信息 : string]` 与 `@Unreachable[信息 : string]` 类型为 never, 执行到即失败. 后三者同属"用户主动中断"一族, 但在错误目录里各自独立, 报告时能看出类别; 断言单独成类. 断言的信息字符串只在失败路径求值. 失败不可恢复, 作用域与其它运行时错误一致: actor 内只终止该 actor, 退出原因为 `Failed[编号, 说明]`, 说明即错误标题; 程序根内终止程序.

## 10. 实现架构

- 流水线: 词法分析, 语法分析, 类型检查, 降级为 CPS IR 与线性 IR, 由线性 IR 抽象机执行; 不生成机器码.
- 执行层把 CPS IR 线性化为基本块后执行: 控制状态加去函数化的续延帧栈; 无操作数栈, 无返回指令; 续延链即栈.
- 函数调用直接替换控制状态, 尾调用不增长续延链; 非尾调用把续延帧压入堆上帧栈, 不消耗 Rust 原生栈.
- 闭包按值捕获自由变量; `@Def` 与 `@Fix` 用写一次单元打结. 抽象机支持自递归, 互递归, 10 万层尾递归与 10 万层非尾递归.
- 运行时错误的位置由 IR 调用/原语节点与续延帧携带的源码区间给出; 错误不可捕获; `@Exit` 走专门通道.
- 抽象机的执行状态为纯数据, actor 挂起即保存控制状态, 续延帧, 槽位与捕获; 恢复即把消息值交回续延; 这是绿色线程调度的基础.
- 执行路径: 降级为 CPS IR 与线性 IR 后解释执行; `ksl cps` 输出树形 IR, `ksl linear` 输出线性化的基本块 IR. 对外稳定契约是 `Value`, 闭包, 续延与 actor 运行时, 不是 AST 节点.
- 类型检查单趟顺序进行: 环境随语句增长, 函数体在定义处按其签名在当前环境中检查.
- 类型变量的身份由 ena 的并查集承担, 结构化的类型统一由检查器自行分派: 变量与变量合并走并查集, 变量与具体类型绑定时检查已定类型, 具体类型之间按 List, Dict, Set, Option, Result, Pair, Pid, Handler 与 Fn 各自分支递归, 并做 occurs 检查以拒绝递归类型. 统一器绑定存入 ena 值槽, 由 `InferTy` 实现其值 trait; occurs 检查由检查器维护. `shallow` 先于绑定求值, 故绑定只作用于未定变量.
- 类型层收敛: `InferTy` 只存在于检查器内部, `Type` 只表示具体类型; 统一器绑定存入 ena 值槽, `VarInfo` 按 `VarId` 索引为稠密数组; 类型表以 `(SourceId, NodeId)` 为键; 降级器消费类型表, 进行类型制导降级. 值类型环境与类型别名表由 `TypeContext` 统一承载.
- 符号驻留: 标识符与类型名在词法层驻留为进程内唯一的 u32, 形态为 Symbol; 下游统一消费 `Symbol`. 内表用字符串到编号的哈希表与编号到字符串的向量维护双向 1:1 查找; 自写薄封装承担索引分配, get_or_intern, resolve 与全局锁. 查找表按用途选择 HashMap 或 imbl 持久化映射.
- 全局变量按名存取于持久化全局环境, 类型检查与 IR 降级无状态, REPL 跨行增量定义成立. 顶层绑定统一按全局存储处理, 批处理与交互共用一条路径.
- 宿主生命周期由 `ksl-core::host::Host` 收拢: 宿主持有源码, 类型会话, 调度器与模块缓存, 批处理与交互共用加载, 检查, 执行与回滚流程; 命令行只负责参数解析, 诊断渲染与 REPL 展示. 交互输入在宿主内按两阶段提交, 类型检查在会话副本上进行, 运行成功后才提交类型环境与模块缓存.
- 运行时表示: `Value` 体积不超过 24 字节, 大于指针的变体一律 `Arc` 包裹; 列表用 imbl 的持久化向量并以 `Arc` 包裹, `List[i64]`, `List[u8]`, `List[u64]`, `List[f64]`, `List[char]` 与 `List[bool]` 使用窄元素专用存储, string 用 `Arc<str>`, Data 值与闭包用 `Arc` 包裹; 闭包按值捕获自由变量, 普通捕获只克隆 `Value`, 递归定义使用写一次单元打结, 用户数据不使用内部可变性; Actor, File 与 Socket 句柄由运行时内部同步. `Value` 是 `Send + Sync`; 值拷贝为 O(1); imbl 的节点级结构共享; 续延帧为堆上结构, ksl 递归不消耗 Rust 原生栈. 变体载荷以 8 字节瘦指针为主, string 的 `Arc<str>` 是胖指针占 16 字节, 加判别位即 24 字节.
- CPS IR 携带绑定者类型与类型制导节点的类型; 抽象机执行时大多忽略这些字段, 闭包签名显示与命名注册原语使用其中的类型. 类型表覆盖 LSP 所需的类型与位置, LSP 不依赖 IR 级信息.
- typed IR 采用类型制导降级: 检查器产出以 `(SourceId, NodeId)` 为键的类型表, 降级器消费该表, 产出只保留表示决策与函数签名的 IR; 运行时 `Value` 只承载表示类别, 不承载完整静态类型. 类型信息落在绑定者, 即函数签名, `Let` 绑定目标, 续延参数与模式字段绑定, 不给每个值槽重复标注; IR 采用 CPS 形态, 复合值都经绑定或续延参数, 绑定者带类型即可覆盖, 标量字面值则由枚举变体自描述. 类型制导操作, 例如列表操作, 在操作节点自身携带完整 `Type`. NodeId 在解析期为每个 `Spanned` 节点分配, 只承担类型表的节点身份, 用户可见诊断仍只用 `Span`.
- 尾调用由 CPS 直接表达.
- CPS IR: `cps/ir.rs` 把检查后的 AST 降级为函数级 CPS 形式, 每个函数带续延参数 `$k`, 尾调用, 普通调用, 续延绑定, `receive` 挂起点与 `continue` 均显式可见. 该树形形态是线性化的输入, `ksl cps` 输出它供调试. IR 节点保留源码区间供运行时错误定位.
- 线性 IR: `cps/linear` 把 CPS 树线性化为基本块, 每个块带参数, 顺序指令与唯一终止指令; `Instr` 覆盖绑定, 顶层绑定与递归绑定, `Terminator` 覆盖返回, 跳转, 调用, 原语, 接收, 分支与 Actor 终止. 线性化后完成闭包转换: 分析函数自由变量, 计算基本块活跃变量, 把变量引用解析为槽位或捕获下标, 跳转与调用显式传递活跃参数; 续延是目标基本块, 运行时以槽位与捕获下标取值, 不维护名字环境与续延表. 自由变量分析对函数引用取传递闭包: 函数值的自由变量在被引用处展开, 再按该处的绑定集合过滤, 引用链成环时迭代到不动点. 块内指令连续执行, 到终止指令或预算耗尽时回到调度器; 超长基本块不会独占调度. `ksl linear` 输出该形态. 线性执行器是唯一后端.
- 编译期栈安全: CPS 降级按语句序列回卷, 自由变量分析与线性化使用显式工作栈, 释放 CPS 树时逐节点展开; 长列表字面量与长语句序列不随长度加深调用栈.
- IR 命名: 源码标识符, 函数名, Data 类型与变体名, 模式字段直接使用驻留 `Symbol`; 合成临时值与续延使用单调编号 `TempId` 与 `ContId`, 不进入驻留表; 函数表以 `Symbol` 为键, 显示时解析回文本. 入口函数使用源码不可表达的 `$main`, 避免与用户函数同名. 运行时全局环境仍只为源码名建索引, 合成临时值存放在执行作用域的独立持久化映射中.
- 环回收: 常规释放由 `Arc` 引用计数完成, 显式标记清扫只处理不可达环. 依据是不可变容器值与标量无法独立成环, 任何环都经过 `Closure` 的捕获向量或 `ActorHandle` 的信箱和结果; 清扫对不可达闭包清空捕获, 对不可达 Actor 句柄清空信箱与结果, 环被截断后由引用计数释放. 捕获向量用可清空的内部结构保存, Actor 句柄的信箱与结果本身已同步. 候选对象以弱引用登记, 标记从调度器状态, 模块缓存导出值与宿主额外根出发. REPL 每次输入开始时收集; 调度器循环顶部在新登记候选对象达到默认 4096 时收集, 阈值可调, 0 关闭自动收集. 执行中途收集使用额外根栈: 回滚快照, 模块基环境与已执行模块的导出值由执行器成对压入与弹出, 避免只存在于 Rust 栈上的值被误清. 不做移动或压缩, `Value` 表示不变; I/O 任务只携带内部结果与字节数据, 不捕获任意用户 `Value`.
- 网络 I/O: 单个网络事件线程持有全部 TCP 套接字, 语言层句柄只保存令牌, 对端地址与关闭标志; 会等待的网络原语提交操作后挂起 Actor, 完成经既有 I/O 完成通道返回. 事件线程用非阻塞套接字与轮询器处理监听, 接受, 连接, 读写与截止时刻; `SocketClose` 与句柄释放提交关闭命令, 注销注册并以 `SocketError/Closed` 唤醒挂起操作; 调度器终止等待中的 Actor 时提交取消命令, 注销对应操作. 监听器只接受显式 IP 字面量; 主机名解析与 TLS 不在当前范围内.

## 11. 启动形态

- 命令行以子命令组织, 参数解析由 `ksl-cli` 自行实现, 帮助与错误信息使用中文: `ksl` 无参数显示帮助; `ksl repl [初始化文件]` 进入交互模式, 给定初始化文件时先加载该文件作为主模块再进入交互, 加载出错时报错并进入空环境的交互; `ksl repl --code <代码>` 在进入交互前执行一段代码; `ksl run <文件>` 执行脚本后退出, `ksl run --code <代码>` 直接执行代码; `ksl lex`, `ksl ast`, `ksl check`, `ksl cps`, `ksl linear` 输出各阶段中间结果; `ksl lsp` 启动语言服务器, 通过标准输入输出走 LSP; 源文件扩展名 `.ksl`. 不支持直接把文件作为首参数, 也不根据扩展名推断子命令. `ksl cps` 输出树形 CPS IR, `ksl linear` 输出线性化基本块 IR.
- 程序退出: 程序入口结束后, 未被显式 Join 的存活 Actor 由运行时强制终止. 入口是根模块的 `@Main`; 没有 `@Main` 时是顶层语句. 退出码 0 表示正常, 1 表示错误终止, `@Exit[code]` 可指定其他退出码; 命令行用法错误由参数解析器处理, 退出码 2, 与语言退出码无关.
- 错误报告: 各阶段错误带源码位置, 经 annotate-snippets 渲染输出到标准错误; 脚本遇首个错误即终止, REPL 出错不退出会话.
- LSP: `ksl lsp` 通过标准输入输出通信, 全量文本同步, 发布首个类型检查或加载错误; 位置按 UTF-16 code unit 换算, 并提供顶层 Data, 函数与绑定的文档符号.
- REPL: 交互编辑器是终端会话的客户端, 从服务接收按键, 粘贴与尺寸事件, 不自行管理 raw mode 与事件源; 环境跨输入行持久; 求值期间调用 `ReadLine` 时, `ReadLine` 的提示照常写出并从同一标准输入读取下一行, REPL 不在这期间显示下一轮提示符; 程序输出与后续提示符不重叠; 程序输入优先, REPL 不抢占进行中的程序读取; 每次输入按两阶段提交, 类型检查在会话副本上进行, 运行成功后才提交类型环境, 运行失败时抽象机回滚全局绑定, 出错不留下半程状态; 每次输入结束时类型变量必须全部解析为具体类型, 不允许把未定类型带入下一次输入; 结果统一以 `<编号>= ` 前缀显示, nil 也显示; 不可打印类型显示尖括号类型摘要, 例如 `<Shape>`; 顶层绑定存于全局环境, 批处理与交互共用一条执行路径.
- REPL 命令以 `%` 开头, 不属于语言语法, 只在交互模式生效; `%help` 显示命令, `%type <表达式>` 只做类型检查且不执行, `%bindings` 列出全局绑定, `%reset` 清空会话状态, `%quit` 退出. 命令不设简写. 命令输出与求值结果分开显示, `%type` 采用 `表达式 : 类型` 形式.
- REPL 使用内置行编辑器, 不依赖第三方行编辑库. 输入采用多行缓冲: 段内任意行可编辑, 编辑中以 `n$ ` 提示焦点行, 其余行按位置显示 `n> ` 与 `n| `, 接受或结束后回显固定回位置提示符, 不再保留编辑状态; 超出终端高度时按视口窗口显示, 焦点行始终可见. Enter 在输入完整时整段提交, 否则在光标处拆行续输; Backspace 在焦点行行首合并上一行, Ctrl-D 在焦点行行尾合并下一行, 末尾空行上的 Ctrl-D 删除该空行, 唯一空行上的 Ctrl-D 结束输入. 键位为 Emacs 子集: `Ctrl-A/E/B/F` 行内移动, `Ctrl-P/N` 在行间转移编辑焦点, `Ctrl-K/U/W/Y` 行内杀/粘贴, `Ctrl-D`, `Ctrl-C`, `Ctrl-L`, `Home/End`, 方向键, `Backspace/Delete`; 方向键上下选择历史, 仅单行输入时可用; 不提供补全, 灰字提示, 撤销与反向搜索. Tab 按键忽略: 语言没有缩进语义且补全尚未提供, 按下 Tab 不写入任何空白; 粘贴内容中的 tab 仍按文本处理, 绘制时展开为制表位. 高亮按语言词法器计算: 数值与字符串字面量, 内置函数, 保留名与注释各有配色; `@` 与 `^` 前缀记号随保留名配色, 受限名字按词法位置区分——斜杠前的类型限定部分随保留名配色, 斜杠同色以保持名字为一个视觉单元, 斜杠后的构造器与变体名单独配色, 模式中省略类型限定的 `/Variant` 同样命中. 光标落在括号上或紧邻其右侧时, 该括号与其配对一并强调, 没有配对时报错色; 配对查找只走词法记号, 字符串与注释里的括号不参与, 且只跟随焦点行的光标. 历史仅保存在内存, 整段输入登记为一条. 终端 `ReadLine` 复用单行编辑器 `LineEditor`, 键位处理与单行布局相同, 只去掉历史, 高亮, 多行与续行. `LineEditor` 与 REPL 的 `BufferEditor` 共享 `KeyTarget` 键位映射, 只在编辑目标上区分单行与多行; 多行拆分, 焦点转移, 历史与高亮不属于共享核心.
- 交互模式的提示符为 `n> `, 编辑中焦点行为 `n$ `, 其余续行为 `n| `, 结果为 `n= `; 接受后的回显统一显示 `n> ` 与 `n| `. `n` 在同一段输入中相同. 编号每次非空输入都前进, 失败与 REPL 命令同样占编号, 空行不占; 编号只用于标记, 不参与语义. 命令行代码算作一次输入, 占用第一个编号.
- 程序输出在交互模式的求值期间立即写出并冲刷, 终端上不滞留, 因此先提示再读输入的程序能立刻看到提示. 核心记录输出是否停在行首, 轮末若不在行首则补一个换行, 使 `n= ` 独占一行, 例如 `PrintRaw["B"]; 12` 显示为 `B` 与 `1= 12` 两行. 该记录是进程级的: 提示一类写出发生在 I/O 工作线程上, 同样计入. 失败的输入没有结果标签, 补的换行使错误报告另起一行.
- 交互模式以空白行分隔区块: 输入回显, 程序输出, 结果标签或诊断, 下一轮提示符两两之间各固定一个空白行; 标准输出与标准错误按到达顺序混排显示, 不分区; 空行输入同样只留一个空白行; 管道模式不加空白行, 输出保持干净.
- 非终端输入 (例如管道或测试脚本) 由编辑器的逐行读取路径处理: 不打印提示符, 不读键位, 仍按同一个 `complete` 判定把多行累加成一段完整输入; 管道与终端两条路径共用同一套求值与提交逻辑, 结果同样带 `<编号>= ` 标签.
- REPL 加载初始化文件时只执行顶层语句并保留主文件 `@Export` 的绑定, 不执行 `@Main`, 不显示主函数结果, `Print` 输出照常; 未导出的顶层名字只作为主文件内部实现. 加载与交互求值的显示策略分离. 加载出错只影响当次加载, 会话继续.
- REPL 可见的类型以会话的类型别名表为准: 主文件导出的类型与行内导入的类型都以导出名或导入名可见, 类型身份取真实类型. 本地 `@Data` 声明的类型以本地名为准, 与已有同名别名冲突时报错.
- REPL 的输入可以带 `@Use`, 按第 14 节的会话模块缓存规则处理; 输入内 `@Use` 在所在位置生效, 不做输入内提升; 脚本与模块文件中的 `@Use` 按第 14 节规则提升到模块顶部. 输入中的 `@Export` 报 `ExportOutsideModule`.
- REPL 的模块缓存, 失败回滚与重复副作用规则见第 14 节.

## 12. 工程约束

- Rust workspace 实现, 项目名 ksl, crate 命名使用连字符; 拆分为 `ksl-core`, `ksl-cli`, `ksl-macros` 与 `ksl-treesitter`.
- 默认工具链为最新 Nightly, MSRV 1.100.0, edition 2024.
- 依赖使用最新稳定版本, 允许使用第三方库. 采用: annotate-snippets 用于错误报告, imbl 用于持久化结构, crossbeam-channel 用于 I/O 请求队列, unicode-ident 用于 XID 判定, parking_lot 用于同步, indexmap 用于稳定顺序, ksl-macros 用于错误编号, lsp-server, lsp-types 与 serde_json 用于 LSP, percent-encoding 用于 file URI 编解码, jiff 用于时间与日期, crossterm 用于终端按键与 REPL 行编辑, unicode-segmentation 与 unicode-width 用于字素切分与显示宽度, proc-macro2, quote 与 syn 用于过程宏, tree-sitter 与 tree-sitter-language 用于编辑器语法, cc 用于构建脚本; 内部类型变量统一采用 ena; CLI 参数解析自行实现, 不依赖第三方库; CLI 默认使用 mimalloc v2 稳定分支作为全局分配器, 可用 `--no-default-features` 回退系统分配器; CLI 端到端测试使用 assert_cmd 与 predicates, 测试临时目录位于 Cargo 的 `target/tmp`.
- 有序映射使用 indexmap: Data 类型表与 Data 收集阶段按声明顺序保存, 使遍历顺序与首个错误选择可复现. 需要结构共享的会话状态使用 imbl 持久化映射: 类型表, 检查环境作用域与模块缓存外层; 其余只做查找或成员判断的表用 HashMap. 编译器保持 fail-fast.
- 许可证采用 AGPL-3.0-or-later.
- clippy 严格检查, clippy.toml 放宽对测试代码的约束, 问题优先自动修复.
- rustfmt 启用 Nightly 不稳定选项.
- 测试遵循最小原则, 仅作为排错手段, 不以数量作为指标.
- 代码优先级: 可维护性 > 可读性 > 性能, 非优化阶段不做性能取舍.
- 注释全中文, 遵循 `docs/中文科技文档协作规范.md`; 只写功能与契约, 不写项目变迁, 备选方案与讨论过程.
- Git 小步提交, 阶段完成后再压缩为版块提交, Conventional Commits 风格.
- 文档允许直接修改, Git 作为历史备份, 非必要不保留修改前内容.
- 不使用子代理.

## 13. Actor 运行时

- Actor 结构模仿 Erlang 的绿色线程与信箱通信. Actor 主体是用户写的递归函数, 挂起状态是续延帧栈.
- 调度器为单线程协作式: 在一个线程上维护就绪队列, Actor 表, 定时器堆与等待表, 顺序确定. 每任务按固定指令预算让出; 预算按 IR 指令与终止指令计数, 超长基本块也在预算边界让出; 单 Actor 内的输出顺序可复现.
- 调度器维护就绪队列, 信箱等待表, I/O 等待表与定时堆; Join 等待表按目标编号登记. 任务定时堆承担 Sleep/Receive 超时, 共享延迟投递堆承担 `@SetTimer`; 两者都按截止时刻顺序取出; 延迟消息在触发前作为环回收根, 目标终止或调度器关闭时清理. 顶层语句作为 Actor 之外的 Prelude 与 Actor 在同一线程上按预算轮转, `@Main` 根 Actor 与普通 Actor 同队; `@Receive`/`@Join`/`@Sleep`/I/O 操作保存机器状态并让出, `@Send`/超时/目标终止/I/O 完成唤醒对应任务; I/O 由固定工作线程与 IO 服务执行, 调度器以事件通道等待 I/O 完成与定时到达, 不轮询; 所有任务都在等待且无 I/O, 定时器与事件时报告运行时错误.
- 环回收在调度器安全点执行: 标记从调度器状态, 宿主额外根与模块缓存导出值出发, 清扫不可达闭包与 Actor 句柄. 收集只发生在安全点, 不进入常规调度路径. 全局环境无需读写锁: Actor 机器不共享全局环境, 全局环境只有 Prelude 与宿主访问.
- REPL 会话不使用 Actor: 交互输入禁止 Actor 原语, 失败的输入只回滚全局绑定, 不涉及 Actor 清理.
- 自 Join 与 Join 主 Pid 都报运行时错误: 等待自己或等待根都是永远等不到结果的等待, 主 Pid 的唯一来源是 `@Main` 体内的 self, 经消息, 注册表与 Spawn 实参传播. Actor 内运行时错误终止该 Actor 并转为 `Failed` 退出原因; `@Kill` 对等待中的目标立即终止, 对运行中的目标在下一个调度边界终止; Kill 根 Pid 报运行时错误. `@Sleep` 仅限 Handler 与 `@Main` 体内使用; 顶层语句不提供 Sleep.
- Actor 蓝图: `@Handler[[self : Pid[...], 状态...] : R, 体]` 是表达式, 值是 `Handler[...]` 类型; 求值只构造蓝图, 不执行体. Handler 与 `Fn` 是不同类别, 不可作为函数调用; 它可以像匿名函数一样被 `@Let` 绑定, 作为参数/返回值/消息字段传递, 但只能由 `@Spawn` 启动. 需要名字时用 `@Let[name, @Handler[...]]`, 不设专门的具名 Handler 语句.
- Handler 自循环: Handler 体内用 `@Continue[新状态...]` 以同一个 self 重新进入当前 Handler; `@Continue` 类型为 `Never`, 参数对应 Handler 除 self 外的状态参数. Handler 不通过名字自递归, 因此不同 Handler 之间无法直接相互引用, 只能通过 `@Spawn` 与消息通信.
- Actor 原语的词法位置: Actor 原语族只允许出现在 `@Handler` 体与 `@Main` 体内, 包括 `@Spawn`, `@SpawnDetached`, `@Send`, `@Join`, `@Kill`, `@Monitor`, `@Unmonitor`, `@Register`, `@WhereIs`, `@Unregister`, `@PidSerial`, `@SetTimer`, `@CancelTimer`, `@Receive`, `@Sleep`, `@Halt` 与 `@Continue`; 模块顶层, 函数体, 交互输入与命令行代码顶层一律禁止, 嵌套匿名 `@Fn` 或 `@Def` 不继承该上下文, 嵌套 `@Handler` 有自己的上下文. `@Handler` 蓝图表达式是纯值, 任意位置可用, `@Spawn` 的 handler 实参必须是 `Handler[...]` 类型. 库模块需要 Actor 时导出 Handler 由使用方 Spawn.
- Actor 的类型: `Pid[T0, ..., Tn-1, R]`, 前 n 个参数依次为信箱 0..n-1 的消息类型, R 为返回类型; n 至少为 1, 至多 16. `Handler[Pid[T0, ..., Tn-1, R], A1, ..., Ak]` 是蓝图类型, 状态参数为 A1..Ak; `@Spawn[handler, 实参...]` 的 handler 必须是 Handler 类型, 实参匹配 A1..Ak, 返回 `Pid[T0, ..., Tn-1, R]`; 信箱数由 handler 的 Pid 类型推断, 不在调用点重复书写.
- 信箱: 每个 Actor 有 n 个信箱, n 由 Pid 类型参数个数确定; `@Spawn[handler, ...]` 直接使用 handler 的 Pid 类型中的 n. 各信箱独立排队; 同一发送者到同一信箱的消息严格保序; 不同发送者之间的交错由实际发送时刻决定; 跨信箱无序. `@Send` 与 `@Receive` 的信箱索引必须是可以静态确定的整数字面量, 以便按位置选择消息类型. 信箱布局约定: 数据信箱从 0 开始, 需要独立的控制信箱时放在最后一个索引; 简单 Actor 可以只保留一个信箱. 监视通知属于控制消息, 固定投递到本 Actor 的最后一个信箱, 该信箱的类型必须为 `Pair[u64, ExitReason]`; 数据消息与监视通知不能共用同一信箱. 该约定是推荐规则, 检查器只在校验 `@Monitor` 时检查最后一个信箱的类型.
- 信箱索引: `@Send` 与 `@Receive` 的信箱索引按 u8 字面值处理, 必须写 `u` 后缀, 例如 `1u`; 超出 u8 范围报字面量越界; 非字面值报 `LiteralRequired`.
- 命名注册: `@Register[name, pid] -> bool` 在名字冲突或目标已终止时返回 false, 同名同 Pid 同类型幂等; `@WhereIs[name] -> Option[Pid[...]]` 必须由图上下文给出 Pid 类型, 名字缺失或类型标签不符时返回 None; `@Unregister[name] -> bool`; 一个 Pid 可有多个名字; 任意 Actor 可注销; 调度器退出时清空.
- Pid 编号: 调度器内单调递增分配, 不重用; `@PidSerial[pid] -> u64` 暴露该编号, 仅在当前会话内稳定, 不可持久化.
- 原语: `@Spawn[handler, 实参...] -> Pid[T0, ..., Tn-1, R]`; `@SpawnDetached[handler, 实参...]` 生成不隶属于父的 Actor; `@Send[pid, 字面索引, 消息] -> bool`, 消息类型须为该索引对应信箱的类型, 索引越界报 ksl 级错误; 返回值表示消息是否被目标信箱接受, 目标已终止时返回 false 并丢弃消息, 但 true 不保证目标已处理; `@Receive[字面索引, 超时] -> Option[T_index]`, 超时为 `Option[u64]`, None 表示无限等待, Some(0) 为非阻塞轮询; `@Join[pid] -> Result[R, ExitReason]`; `@Kill[pid] -> bool`, 返回目标本来是否存活; `@Monitor[pid] -> Option[u64]`, 监视通知投递到本 Actor 的最后一个信箱, 该信箱类型必须为 `Pair[u64, ExitReason]`, 目标终止时投递 `(监视编号, 退出原因)`, 目标已终止时立即投递, 监视自身返回 None; `@Unmonitor[监视编号] -> bool`, 仅登记监视的 Actor 可注销, 已投递的通知无法撤回; 监视通知只入队, 不唤醒阻塞在其他信箱上的 `@Receive`, 监督者须以有限超时轮询; `@PidSerial[pid] -> u64` 取当前会话内唯一的 Actor 编号; `@SetTimer[pid, 字面索引, 消息, 毫秒] -> Option[u64]` 延迟投递, 目标已终止时返回 None, 返回的编号用于取消; `@CancelTimer[定时编号] -> bool` 仅创建者可取消尚未触发的定时消息, 已投递的无法撤回; `@Halt[]` 自身终止; `@Sleep[毫秒 : u64]`. 超时与 Sleep 时长不设语言级上限; 调度器若无法表示截止时间, 报运行时错误, 不得 panic. 无 `@Self`: pid 即 Handler 的首参数.
- 退出原因: 内置 Data 类型 `ExitReason`, 变体为 `Halted[]`, `Killed[]`, `Failed[编号 : i64, 说明 : string]`; 正常返回与 `@Halt[]` 在监视通知中都记为 `Halted`, 需要返回值时再 `@Join`; join 一个已终止的 Actor 返回其当初的结局, 退出原因随 pid 值存活, 句柄释放则记录随之释放. Actor 内运行时错误终止该 Actor 并产生 `Failed`; 程序根错误终止整个程序. `@Exit` 不允许出现在普通 Actor 内; `@Main` 根 Actor 是唯一例外, 其 `@Exit` 映射为程序退出码.
- Actor 失败是监督事件, 不是进程错误: 批处理退出码只由程序根决定; 失败通过 `@Join` 或 `@Monitor` 观察. 已观察的失败不再重复诊断; 未观察的失败以 `note: Actor 失败, 不影响退出码` 加原诊断输出到标准错误.
- 消息携带 Pid: 不提供 `@Request`; 需要回复时, 在消息 Data 类型的字段里写入发送方的具体 `Pid` 类型, 接收方按该类型 `@Send` 回复. 单态类型下 Pid 类型必须完整写出, 不能从消息内容反推; 不同 Pid 形状的客户端使用不同变体或不同协议 Data 类型. 递归 Data 类型与互相引用的 Pid 类型由声明预收集支持. 需要把 Pid 作为键或日志标识时用 `@PidSerial`; 服务发现仍用 `@Register`.
- 生命周期: spawn 建立的 Actor 隶属于父 Actor, 父 Actor 终止时子 Actor 一并终止, 包括父终止时尚未启动的子 Actor; 此为单向归属, 子 Actor 终止不影响父; 需要长驻且独立于父的 Actor 时用 `@SpawnDetached`. 由 `@Main` 生成的 Actor 在程序退出时统一终止. Actor 终止时清理以它为目标的未触发延迟消息.
- 恢复: 监督者通过 `@Monitor` 或 `@Join` 取得死因后按策略重新 spawn, 状态从初值重建; 策略, 包括何时重启与重启强度上限, 属库层, 语言只提供监视, join 与 spawn.
- 信箱消息不做挑选: 不提供按变体选择消息的机制, 投递顺序即到达顺序; 控制类消息与数据类消息分属不同信箱, 取消一类需求由多信箱与接收超时承担.
- 主 Actor: 模块可以 `@Main[[self : Pid[T0, ..., nil]] : nil, 语句...]` 声明主入口, 形态与 `@Handler` 同构, 参数表只有 self, 返回类型固定为 nil, self 的名字由用户选择. `@Main` 体内可用除 `@Halt` 与 `@Continue` 外的全部 Actor 原语, `@Exit` 允许出现在 `@Main` 体内并映射为程序退出码. 直接运行时根模块的 `@Main` 作为程序入口执行, 依赖模块的 `@Main` 跳过. 顶层语句是先于根 Actor 的不含 Actor 原语的 Prelude, 只负责绑定与 I/O; `@Main` 根 Actor 走普通 Actor 任务路径, 正常返回 nil, 运行时错误终止程序, 子 Actor 随根结束终止. 没有 `@Main` 的模块不运行根 Actor, 但顶层语句同样不能使用 Actor 原语; 需要 Actor 的库导出 Handler, 由使用方 Spawn.
- 系统 Actor: 行为由宿主实现, 协议与语言 Actor 同构的 Actor, 拥有内置的 Pid 与 Handler 类型. 标准 IO 服务是第一个系统 Actor: 全局唯一, 运行时启动, 先于一切用户代码存在; 单信箱, 请求是内置 Data 类型 `IORequest` 的变体, 变体为 `Print`, `PrintRaw`, `PrintError`, `ReadLine` 与 `ReadKey`; 输出请求无回复, 异步且有序, 写出错误在下一个同步点报告, 属宿主级错误; 输入请求经每请求一个的运行时回复句柄等待回复, 句柄不对用户可见; 服务等待输入期间继续服务输出请求, 输出落在提示符上方. 标准 IO 服务由宿主线程承载, 不进入调度器就绪队列; 协议与调度器的提交/完成路径保持独立.
- 停止原语: `@Halt[]` 无参, 仅用于 Handler 终止自身, `@Main` 中禁止, 语义类似进程级 return, 从任意深处展开至入口结束; 外部优雅停止不专设原语, 以约定消息表达, Actor 收到后在消息分支内完成清理, 再以 `@Halt[]` 结束; `@Kill[pid]` 为外部强制终止, 调度器在协作边界唤醒并终止目标; 对已终止或不存在的 Pid 两者均无副作用.
- 汇合原语: `@Join[pid] -> Result[R, ExitReason]`, 挂起调用方直至目标 Actor 终止, 正常返回为 Ok(返回值), 异常终止为 Err(退出原因); 目标已终止时立即返回其当初的结局; 多个 Join 等待者共享同一结局并在目标终止时一并唤醒; 自 Join 与 Join 根 Pid 都报运行时错误. Join 只等待不通知, 适合一次性任务; 需要在处理其他消息的同时察觉目标终止时使用 `@Monitor`.
- Actor 监督: 调度器统一持有信箱与监视登记; 不引入 `@Select`, Link 或 trap exits.

## 14. 模块系统

- 文件即模块; 主文件所在目录为根目录; `@Use` 的字符串是模块路径, 相对于当前模块文件所在目录; 实现统一追加 `.ksl`, 因此 `"lib/a"` 指向 `lib/a.ksl`, `"lib/a.ksl"` 会指向 `lib/a.ksl.ksl`; 不提供搜索路径.
- 路径中的 `.` 与 `..` 按词法规范化; 规范化后不得越出根目录; 模块身份为规范化后的根相对路径, 完整名称不含 `.ksl`, 例如 `lib/a.ksl` 的模块名为 `lib/a`, 其中类型 `A` 的完整名称为 `lib/a/A`.
- `@Export[名字, ..., ^类型, ...]` 与 `@Use[{名字, (旧名, 新名), ^类型, ^(旧名, 新名)}, "路径"]` 只能出现在顶层. 类型条目使用 `^` 提升, 重命名条目使用配对记号. `@Export[]` 与 `@Use[{}, "路径"]` 合法但无意义, 视为 noop; 空条目 `@Use` 不加载目标模块.
- 位置不影响语义: `@Use` 在分析时视为位于模块顶部, `@Main` 视为位于顶层语句之后, `@Export` 视为位于模块末尾; 源码中的实际位置保留给错误定位, `Span` 不因逻辑移动而改变.
- 导出声明只允许出现在模块中: 交互输入与 `ksl run --code` 中的 `@Export` 报 `ExportOutsideModule`. 同一模块只允许一个导出声明, 多个导出声明报 `MultipleExportStatements`. `@Main` 同样只允许出现在模块中, 交互输入与 `ksl run --code` 中的 `@Main` 报 `MainOutsideModule`.
- 一个模块对同一个依赖模块最多写一次 `@Use`; 同一依赖的多个导入项放在同一个列表中. 按规范化路径判重, 重复出现报 `DuplicateUse`.
- 模块外部只能看到 `@Export` 列出的名字; 未导出的值, 类型与构造器是模块私有实现. `@Use` 导入的名字成为当前模块的本地名字, 可以再次 `@Export`, 即允许 re-export.
- 模块外部只能 `@Use` 显式导出的内容; 未导出的名字不可导入.
- 值导入与本地值: 导入内容视为位于模块顶部, 本地 `@Let`, `@Def` 与 `@Fix` 可以覆盖同名导入值, 与普通绑定遮蔽规则相同.
- 类型导入: 本地 `@Data` 声明与导入类型重名时报错, 不允许重复定义. 同一作用域内同一个类型可见名只允许出现一次; 两个不同可见名指向同一真实类型仍允许.
- 两个导入值同名时报错; 没有限定值名语法.
- 模块内使用简化名; 本地声明的完整名称为 `模块路径/名字`. 导入类型在导入方形成本地名, 其类型身份取真实类型 `导出方模块路径/名字`; 类型推断使用本地名, 类型等价, 构造器解析, 模式匹配与 re-export 均以真实类型为准.
- 导出值的类型必须可见: 导出值签名中出现的全部 Data 类型都必须已导出或为内置类型. 该检查递归覆盖函数参数与返回值, List, Dict, Set, Pair, Option, Result, Pid 的信箱与返回类型, Handler 的状态参数与 Data 字段. 未导出类型的值不得被外部使用, 也不得用无类型限定模式匹配其私有构造器.
- 模块顶层按依赖顺序执行一次, 尾表达式结果丢弃, 模块默认值为 nil; 顶层副作用, 包括 I/O, 允许, Actor 原语按第 13 节的词法位置规则禁止. 一次根加载内按规范化路径缓存, 同一模块只执行一次.
- 模块依赖图必须无环; 加载器用 DFS 标记检测, 遇到正在加载的模块时报 `CyclicImport`, 并输出完整依赖链.
- 主文件不是被导入模块: `ksl run` 执行完整依赖树; `ksl repl <初始化文件>` 进入 REPL 时只暴露主文件 `@Export` 的名字, 未导出的顶层绑定不进入交互环境. REPL 中的 `@Use` 相对初始化文件目录; 没有初始化文件时相对当前工作目录.
- REPL 与会话模块缓存: 会话内每个模块只执行一次; 新输入可以加载尚未加载的模块; 新模块依赖已加载模块时复用缓存导出与类型, 不重新执行; 直接再次加载已加载模块时报错并回滚该输入. 修改模块后重新启动 REPL. 导入的值并入会话的全局环境, 导入的类型以导入名可见, 类型身份取真实类型; 别名跨输入保留.
- 行内导入的失败处理与普通输入一致: 类型检查失败则不执行依赖模块; 运行失败则回滚全局绑定与该轮启动的 Actor, 本次导入的值一并撤销; 已经发生的 I/O 与 Print 副作用不回滚.
- `ksl run --code` 中的 `@Use` 以当前工作目录为根建立一次根加载, 依赖模块执行一次; 进程结束即丢弃缓存.
- 模块相关的加载, 导入, 导出, 循环与 `@Main` 位置错误加入 `E03xx` 段.

## 15. 网络

- TCP 以不透明句柄 `TcpListener` 与 `Socket` 表示. `Socket` 是已连接的字节流, 后续 TLS 连接复用同一句柄类型; 当前只支持明文 TCP 与 IPv4/IPv6 字面量地址, 不做主机名解析.
- 原语: `TcpListen[host, port]`, `TcpAccept[listener, timeout]`, `TcpConnect[host, port, timeout]`, `TcpCloseListener[listener]`, `TcpLocalPort[listener]`, `SocketPeer[socket]`, `SocketRead[socket, max, timeout]`, `SocketReadExact[socket, count, timeout]`, `SocketReadAll[socket, max, timeout]`, `SocketReadLine[socket, max, timeout]`, `SocketWrite[socket, bytes, timeout]`, `SocketWriteAll[socket, bytes, timeout]`, `SocketWriteLine[socket, text, timeout]` 与 `SocketClose[socket]`.
- `TcpListen` 的 `port` 为 0 时由宿主分配临时端口, `TcpLocalPort` 返回实际端口.
- 可失败的套接字操作返回 `Result[T, SocketError]`. `SocketError` 与 `IOError` 分开: 前者覆盖网络连接, 地址, 超时, 行协议与关闭, 后者保持文件与终端语义. 变体为 `TimedOut`, `InvalidArgument[string]`, `ConnectionRefused`, `ConnectionReset`, `ConnectionClosed`, `Closed`, `AddrInUse`, `AddrNotAvailable`, `PermissionDenied`, `InvalidUtf8`, `LineTooLong`, `TooLarge`, `UnexpectedEof` 与 `Other[string]`; `Closed` 表示本地句柄已关闭, `ConnectionClosed` 表示对端在写操作中关闭.
- 超时参数为 `u64` 毫秒, 0 表示立即尝试一次, 正数表示最多等待; 网络操作不提供无限等待. `SocketReadAll`, `SocketReadExact` 与 `SocketReadLine` 的超时按底层每次等待计算, 单次操作的总耗时不受固定上限约束. 不等待的操作, 即 `TcpListen`, `TcpCloseListener`, `TcpLocalPort`, `SocketPeer` 与 `SocketClose`, 不带超时参数.
- 读语义: `SocketRead` 返回最多 `max` 字节, `None` 表示对端正常关闭; `SocketReadExact` 读满 `count` 字节, 不足返回 `UnexpectedEof`; `SocketReadAll` 读到 EOF, 超过 `max` 返回 `TooLarge`; `SocketReadLine` 读取 UTF-8 行, 去掉 `\r\n`, 无数据时 `None`, 超长返回 `LineTooLong`, 非法 UTF-8 返回 `InvalidUtf8`. `SocketRead`, `SocketReadAll` 与 `SocketReadLine` 的 `max` 必须大于 0; `SocketReadExact` 的 `count` 可以为 0. 同一套接字上的读操作共享内部读缓冲; 超时后未完成的行, 精确读或全量读保留在缓冲中, 下一次继续.
- 写语义: `SocketWrite` 可能只写入一部分并返回写入字节数; `SocketWriteAll` 写完整段数据, 失败时套接字可能已处于部分写入状态, 调用方应关闭连接; `SocketWriteLine` 等价于写 UTF-8 文本加换行. 同一套接字上的写操作按提交顺序串行.
- 并发: 每个 Actor 同时只能挂起一个 I/O 操作, 因此同一 Actor 不能同时等套接字读与读信箱. 惯用法是读 Actor 与写 Actor 分离, 由协调者 Actor 接收事件; 套接字句柄可以在 Actor 之间传递, 读写可同时挂起.
- `@Main` 不能使用 `@Continue`, 而嵌套 `@Def` 不继承 Actor 上下文; 需要循环处理连接或消息时, 使用 `@Handler` 并在体内 `@Continue`, 由 `@Main` spawn 后 `@Join`.
- 内置只提供字节流与行原语; 长度前缀分帧, 聊天协议与未来的节点协议以 `.ksl` 模块或示例实现, 不引入独立标准库与模块搜索路径.
- 当前不支持 UDP, TLS, 主机名解析, 半关闭, `@Select` 以及透明跨进程 Pid; 这些能力在此层之上或之后的阶段扩展.
