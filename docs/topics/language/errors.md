---
updated: 2026-11-27
---

# ksl 错误处理

ksl 的错误目录, 错误作用域与主动中断机制.
内置领域错误类型见 `docs/topics/library/builtins-io.md` 与 `docs/topics/library/builtins-time.md`,
退出原因见 `docs/topics/language/actors.md` 第 6 节.

## 1. 错误目录

词法, 语法, 模块, 类型检查与运行时五类错误归入同一目录, 每种错误有编号与规范标题.
编号分段为 `E01xx` 词法, `E02xx` 语法, `E03xx` 模块, `E04xx` 类型检查, `E05xx` 运行时.
内部错误即实现缺陷, 不编号, 单独渲染.
错误目录与渲染由 `ksl-diagnostics` 维护, 不依赖前端或运行时.

- 编号由过程宏按段基数与枚举声明顺序生成; 段与标题由人工维护.
- 变体在枚举中按逻辑归类排列. 插入新变体时后续编号顺延; 编号不跨版本保持稳定.
- 编号只用于报告与编辑器诊断, 不是稳定契约; 测试按错误种类断言, 不比较编号.
- 词法段按字符, 字符串, 数字与格式化字符串分组.
- 语法段按记号与结构, 特殊形式, 位置规则分组.
- 模块段按类别分组: 路径, 读取与加载, 导入, 导出, 模块级形式, 模块内置, 外来库.
- 类型检查段按类别分组: 类型, 名字与绑定, 模式与分支, Data 访问与声明, Actor 与 Pid, 值与显示, 容器, 插件.
- 运行时段按类别分组: 用户主动中断, 数值与算术, 容器与索引, 调度与等待, 时间与日期, 插件.
- 类型检查阶段区分具体类型冲突与推断期冲突:
  具体类型冲突报 `TypeMismatch`, 标题为类型不符;
  推断期冲突报 `InferMismatch`, 标题为推断类型不符, 消息中的 `_` 表示未能确定的类型.
  二者同在类型检查段并按逻辑顺序相邻.

报告形态为 `error[EXXXX]: 标题` 加具体消息与源码片段.
渲染由 annotate-snippets 完成, 实现只提供位置与消息.
标题说明错误类别; 具体消息只写与标题不同的差异, 不重复标题,
建议性内容使用诊断的 note 行.

类型名按出错模块的可见名显示; 没有可见名时使用真实身份名.
类型不可见是例外: 只显示声明名, 不显示模块路径.
具体消息中的类型名使用源码写法, 例如 `string`, `List[u8]` 与 `Shape`,
中文只用于解释语义.

## 2. 失败表达

内置错误, 包括类型错误, 溢出与未绑定名字等, 不可恢复, 也不能由语言机制捕获.
可能的失败用 `Result` 表示, 可能的缺失用 `Option` 表示.
不设异常机制.

领域错误用 Data 值表示, 可失败操作的返回类型使用 `Result[T, E]`.

- 文件, 目录与文件句柄的失败用内置 Data 类型 `FileError`.
- 标准输入与终端操作的失败用内置 Data 类型 `InputError`.
- 数字解析的失败用内置 Data 类型 `NumberParseError`; UTF-8 解码的失败用 `Utf8Error`.
- 网络与时间族的失败原因分别使用 `SocketError` 与 `TimeError`,
  见 `docs/topics/library/builtins-network.md` 与 `docs/topics/library/builtins-time.md`.
- 正则编译的失败使用 `RegexError`, 见 `docs/topics/library/builtins-regex.md` 第 5 节.
- 本机命令执行使用 `CommandError`, 见 `docs/topics/library/builtins-io.md` 第 10 节.
- `@Link` 函数的返回类型为 `Result[T, E]` 且 `E` 不是 `string` 时, 插件失败无法映射为宿主值,
  在类型检查阶段报 `PluginUnsupportedSignature`.
- `@Link` 的动态库加载, 描述符校验与插件内部错误是宿主级运行时错误, 不返回 `Result`.
  这类错误覆盖插件版本, 插件名, 导出缺失, 签名不符, 类型名重复, 初始化失败,
  插件 panic 与内部错误.
- 声明为 `Result[T, string]` 的插件函数返回业务失败时, 宿主给出 `Result/Err[string]`,
  不产生运行时错误.
- `Option` 与 `Result` 的构造器, 即 `Option/Some`, `Option/None`, `Result/Ok`, `Result/Err`,
  可以在源码中构造, 属内置泛型构造器, 与用户 `Data` 构造器同形.

## 3. 错误作用域

普通 Actor 内的运行时错误只终止该 Actor; 未被 `@Join` 观察时输出诊断.
程序根内的运行时错误终止整个程序. 程序根包括 Actor 之外的 Prelude 与 `@Main` 根 Actor.

所有任务都在等待, 且没有 I/O, 定时器或待处理事件时, 调度器报告 `Deadlock` 运行时错误.

## 4. 主动中断

四个特殊形式供程序显式声明失败, 信息字符串不可省略.

- `@Assert[条件 : bool, 信息 : string]` 条件为真则通过并给出 nil, 为假则断言失败.
  通过时执行继续, 因此其类型不是 never; 不可达分支的意图用 `@Unreachable` 表达.
- `@Panic[信息 : string]`, `@Todo[信息 : string]` 与 `@Unreachable[信息 : string]` 类型为 never,
  执行到即失败.
- 后三者在错误目录中各自独立成类, 与断言分开.
- 断言的信息字符串只在失败路径求值.

失败不可恢复, 作用域与其他运行时错误一致:
Actor 内只终止该 Actor, 退出原因为 `Failed[编号, 说明]`, 说明即错误标题;
程序根内终止程序.

## 5. 程序退出

`@Exit[code : u8]` 是特殊形式, 类型为 never, 终止程序并以指定码退出.

- 退出码写作 u8, 例如 `@Exit[0u8]`.
- 0 表示正常, 1 表示错误终止, 其余由程序自定义.
- 退出消息由 `Print` 写出.
- 程序根内可位于任意表达式位置, 包括 `@Block` 与 `@Match` 分支;
  作为最后一条形式时可以省略分号.
- `@Main` 体属于程序根, 同样允许任意位置.
- 函数体与 `@Handler` 体内禁止, 包括 `@Fn`, `@Def` 与 `@Fix` 的定义体.
- 普通 Actor 终止自身使用 `@Halt`; `@Main` 禁止 `@Halt`, 以正常结束或 `@Exit` 结束程序.
