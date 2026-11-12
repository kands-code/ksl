---
updated: 2026-11-27
---

# ksl 类型系统

ksl 的类型体系与类型检查规则.
语法与字面值形态见 `docs/topics/language/syntax.md`, 求值模型见 `docs/topics/language/evaluation.md`.
类型检查实现见 `docs/topics/implementation/architecture.md` 第 3 节,
降级形态见 `docs/topics/implementation/ir.md`.

## 1. 总则

ksl 采用静态类型, 检查先于求值; 求值器按检查后的类型执行, 不做运行时类型检查.
类型标注由用户书写; 绑定标注可以省略, 类型由检查器推断.
标注用于类型检查, 并为零值, 空容器与未定类型参数提供上下文.

- 全部单态, 不允许多态, 不引入用户可见的类型变量与语言泛型.
  检查器内部可以使用类型变量作为实现细节, 但它们不会出现在用户类型中,
  也不跨检查单元保留.
- 不引入 trait, 无重载, 无子类型.
- 不引入 Atom 类型.
- 不引入联合类型; 联合由 Data 类型的多个变体表达.
- 不引入类型别名.
- never 是检查器内部的底类型, 表示不返回的表达式, 例如 `@Halt[]` 与 `@Exit[]`.
  它与任何类型兼容, 参与类型合并时取另一侧.
  用户不能书写 never, 它也不能作为子表达式的期望类型.

## 2. 类型清单

### 2.1 基础类型

- 数值类型: i64, i32, u8, u32, u64, f32, f64.
- char: Unicode 标量值, 编码为 UTF-8, 对应 Rust 的 char. 字符不直接参与数值运算.
- bool.
- string: 独立类型, 区别于 `List[char]`.
- nil: 独立类型, 仅 nil 与 nil 兼容. nil 是正常值, 可以绑定, 存储, 传递与返回.

### 2.2 容器类型

- `List[T]`: 列表.
- `Dict[K, V]`: 字典. 键限 i64, i32, u8, u32, u64, char, bool 与 string,
  不支持 f32, f64, Data 或容器类型.
- `Set[T]`: 集合. 元素限 i64, i32, u8, u32, u64, char, bool 与 string,
  不支持 f32, f64, Data 或容器类型.
- `Pair[A, B]`: 配对.
- `Option[T]`: 可选值.
- `Result[T, E]`: 可失败结果.
- 映射与集合由 `Dict` 与 `Set` 承担, 操作见 `docs/topics/library/builtins-dict-set.md`.

### 2.3 内置参数化类型构造器

内置参数化类型构造器是正式类别, 含 `List[T]`, `Pid[T0, ..., Tn-1, R]`,
`Handler[Pid[T0, ..., Tn-1, R], A1, ..., Ak]`, `Option[T]`, `Result[T, E]`,
`Pair[A, B]`, `Dict[K, V]` 与 `Set[T]`.
参数在使用点必须全部具体化; 用户 Data 类型不可参数化.
实现上, 内置参数化类型与用户 Data 统一为类型构造器应用,
不透明类型统一为具名不透明类型.

- `Pid[T0, ..., Tn-1, R]`: 前 n 个参数依次为信箱 0..n-1 的消息类型,
  最后一个参数 R 为 Actor 返回值类型.
  n 至少为 1, 至多 16; 信箱数由类型参数个数确定.
  参数个数与信箱数相差一, 因此至少要写两个参数才有一个信箱;
  只写一个参数表示零个信箱, 会被检查器拒绝.
- `Handler[Pid[T0, ..., Tn-1, R], A1, ..., Ak]`: Actor 蓝图类型.
  第一个参数必须是 `Pid` 类型, 后续参数是 Handler 的状态与启动参数类型.
  Handler 与 `Fn` 是不同类别, 不能作为函数调用.
- `Option[T]` 与 `Result[T, E]` 都是内置 Data 类型.
  构造器 `Option/Some`, `Option/None`, `Result/Ok`, `Result/Err` 可用于 `@Match`.
  模式匹配规则见 `docs/topics/language/data.md` 第 4 节.

### 2.4 不透明类型与内置 Data 类型

- 不透明类型为 `File`, `CommandOutput`, `DateTime`, `Socket`, `TcpListener`,
  `Regex` 与 `RegexMatch`.
- 内置 Data 类型有 `ExitReason`, `FileError`, `InputError`, `NumberParseError`, `Utf8Error`,
  `SocketError`, `TimeError`, `KeyEvent`, `RegexError`, `CommandInput`, `CommandOption`,
  `CommandStatus` 与 `CommandError`.
- 内置名保留: 内置函数名, 特殊形式名, 字面关键字与内置类型名
  不得被用户绑定或声明为 Data 类型与变体.
  内置 Data 类型的变体名不保留, 通过类型限定名区分. 中文及其他 Unicode 标识符不受影响.

## 3. 函数类型

- 函数为原生多参, 无柯里化; 实参个数不符即报错.
- 函数类型表示为 `Fn[[参数类型, ...] : 返回类型]`, 与 `@Fn` 形态同构.
- 普通函数值按具体函数类型传递与调用; 函数层面不允许多态.

## 4. 期望类型

类型标注是期望类型: 先看表达式应该是什么类型, 再看它实际是什么类型.
期望来源包括 `@Let` 标注, `@Fn` 与 `@Def` 的参数与返回标注,
内置与用户函数的形参类型, 以及 `Data` 字段声明.
期望会进入复合表达式, 包括 `@If` 两支, `@Which` 各分支与 fallback,
`@Match` 各分支体, `@Block` 末句, 列表元素与调用实参.

标注可以省略的场合限于值自身类型确定, 或有同一表达式内的上下文可定型:
字面值, `@Fn` 表达式, 构造器调用, 列表元素, 分支结果,
以及上下文确定类型的特殊形式, 例如 `@Receive`.
不做任何从后续语句出发的推断.

期望的具体作用:

- 整数字面量的类型由后缀决定, 不随期望改变.
- 浮点字面量只在对应的 f32 或 f64 期望下成立.
- 列表字面量的元素按期望的元素类型检查.
- Pair 字面量的两个分量按期望的分量类型解释.
- 内置泛型构造器从期望取得未定的类型参数.

有期望时, 结果先与期望统一, 再统一分支体; 两个分支与结果共享同一类型变量 `T`.
条件必须为 bool; never 不约束 `T`.
统一失败或检查结束时仍有未定变量, 报错.

`@Match` 的各分支体统一到同一结果类型变量, 规则同 `@If`; 分支顺序不影响类型.

`@Which` 的各分支值与 fallback 统一到同一结果类型变量.
有期望时结果先与期望统一, never 不约束结果, 分支顺序不影响类型.
条件必须为 bool; 分支条件为字面量 `true` 或 `false` 时报错.
统一失败或仍有未定变量时报错.

## 5. 字面量的类型确定

字面量的类型由后缀与形态直接确定. AST 节点保留字面量形态, 不因期望改写.
运行时只消费降级后的 IR 与类型指导信息.

无期望且无上下文时, 值必须能由自身确定类型:

- 整数字面量默认 i64; 写 `i64` 后缀即 i64, `i32` 即 i32, `u8` 即 u8, `u32` 即 u32, `u64` 即 u64.
- `Option/Some` 的元素类型由实参决定.
- `Option/None[]`, `Result/Ok[x]` 的错误类型, `Result/Err[e]` 的成功类型与空列表
  无法由值自身确定, 若无上下文定型则报错并要求补标注.

上下文包括标注, 调用实参类型, 列表元素与其他分支的结果类型.
`@Let[op : Option[i64], Option/None[]]` 是显式标注方式.
有上下文时类型也可由上下文确定, 仍无法定型时报错.

## 6. 统一与类型变量

检查器内部可以使用类型变量统一未定类型; 类型变量对用户不可见,
不做泛化, 不改变字面量的后缀规则.
绑定环境与函数签名不保留类型变量.
每个顶层输入, 函数体与模块检查结束时都必须把类型变量解析为具体类型;
仍未解析则报错.

结构化类型统一由检查器分派:

- 变量与变量合并.
- 变量与具体类型绑定时检查已定类型.
- 具体类型之间按 List, Dict, Set, Option, Result, Pair, Pid, Handler 与 Fn 各自分支递归,
  并做 occurs 检查以拒绝递归类型.

类型变量的表示与统一器实现见 `docs/topics/implementation/architecture.md` 第 3.1 节.

类型变量与期望类型携带源码区间: 未解析变量在创建位置报告, 冲突在当前约束位置报告.
跨函数或模块复用时保留原始区间, 错误仍指向原始位置.

## 7. 列表元素的类型

列表元素共享一个元素类型变量: 有外层期望时按期望检查元素; 无外层期望时由元素共同统一.
`{Option/None[], Option/Some[1]}` 与 `{Option/Some[1], Option/None[]}` 都得到 `List[Option[i64]]`.
`{1, 2u8}` 与 `{1u8, 2}` 因元素类型不一致报错.
空列表只能由期望或上下文提供元素类型;
无上下文的 `{}` 与 `{Option/None[], Option/None[]}` 报错.

列表操作把外层期望传播到结果形状:

- `@ListTail`, `@ListTake`, `@ListDrop` 约束列表实参.
- `@ListHead` 约束 `List[T]`.
- `@ListLookup` 约束 `List[T]`, 结果固定为 `Option[T]`.
- `@ListFindIndex` 约束谓词参数, 结果固定为 `Option[u64]`.
- `@ListMap` 约束函数返回.
- `@ListFold` 约束初始值与函数返回.
- `@ListZip` 约束两个列表.
- `@ListWindow` 约束源列表.

字面量的类型仍由后缀与形态确定, 不因期望改变.

## 8. typed AST 与检查结果

检查器在检查过程中构建 typed AST, 检查结束时把推断类型解析为具体 `Type`,
产出 `CheckedProgram`.

- `CheckedProgram` 包含顶层语句, 尾表达式与可选的模块入口.
- 类型挂在表达式, 绑定目标, 参数, 模式字段, Data 声明与函数签名上.
- REPL 的类型环境跨输入保留; typed AST 只服务本次输入, 执行后丢弃.
- 模块的 typed 程序保存在 `CheckedModule` 中, 只服务本次模块图; 会话缓存保留接口与已执行导出值, 失败输入回滚.
- typed AST 不保留源码文本; 源码生命周期由调用方管理.
- 检查环境的作用域使用持久化映射; 克隆会话副本或逐条绑定只做结构共享.

类型信息保留在检查器与 IR 中, 用于错误消息与 IR 降级.
运行时 `Value` 不携带完整静态类型, 只保留执行必需的信息;
Data 值保留真实类型身份用于匹配.
typed IR 采用类型指导的降级, 形态见 `docs/topics/implementation/architecture.md` 第 3.2 节
与 `docs/topics/implementation/ir.md` 第 1 节.
