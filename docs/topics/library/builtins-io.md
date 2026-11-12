---
updated: 2026-11-21
---

# ksl 输入输出

ksl 的输出, 标准输入, 终端按键, 文件, 目录与环境相关函数.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
格式化字符串的规则见 `docs/topics/language/syntax.md` 第 4.4 节.
I/O 的执行模型见 `docs/topics/implementation/architecture.md` 第 5 节.

## 1. 输出

输出统一为格式化字符串, 不提供按类型的打印函数, 不提供 `Inspect`.
两个函数经 I/O 服务异步且有序写出, 服务在应答输入请求前先冲刷已有输出,
先提示再读输入的程序能立刻看到提示.
同一发送者的输出顺序由信箱语义保证.

- `Print[text : string] : nil`
  - 原样写到标准输出, 不追加换行.
    需要换行时在字符串中写 `\n`; 原始模式下写 `\r\n`.
- `PrintError[text : string] : nil`
  - 原样写到标准错误, 不追加换行.

内置行编辑期间, `Print` 与 `PrintError` 先清除编辑区, 写出文本, 再重绘提示符与当前行.
异步文本须以行首结束; 原始模式下写 `\r\n`, I/O 服务不补换行.
标准错误不是终端时直接写出, 不参与重绘.

## 2. 标准输入

提示先原样写出到标准输出并冲刷, 不补换行; 提示为空串时只读.
标准输入与标准输出都是终端时复用内置行编辑,
支持行内移动, 删除, 剪切, 粘贴, Ctrl-C 与 Ctrl-D; 否则按行读取, 不做行编辑.

- `ReadLine[prompt : string] : Result[Option[string], InputError]`
  - 提示后读一行. `Ok[Some[line]]` 为一行且不含行尾换行, `Ok[None[]]` 为 EOF.
    终端行编辑下 Ctrl-C 为 `Err[InputError/Interrupted[]]`, 其他读取失败为 `Err[InputError]`.
    标准输出不是终端时按行读取, 不做行编辑; 该路径要求非原始模式, Ctrl-C 交给终端处理.
    编辑期间其他 Actor 的输出显示在提示符上方, 当前行保留并重绘.
- `ReadKey[] : Result[KeyEvent, InputError]`
  - 读一个按键. 标准输入不是终端时返回 `Err[InputError/NotATerminal[]]`.
    读取期间按第 3 节的规则开启或借用原始模式.
    只返回按下与重复的按键; 松开事件, 鼠标, 焦点, 尺寸与未支持的按键被跳过.
    原始模式下 Ctrl-C 作为 `Ctrl['c']` 返回, 不再产生中断信号.

`ReadKey` 与 `ReadLine` 在终端模式下共用 crossterm 事件源, 可以交替调用.

## 3. 终端原始模式

原始模式同一时刻最多有一个持有者. 持有期间终端按键即时送达, 不被行规程回显, 也不按行缓冲.
单次 `ReadKey` 与 `ReadLine` 在无持有者时临时进入原始模式, 返回前恢复;
有持有者时借用当前模式, 不负责恢复. 两者可由任意 Actor 调用, 输入服务串行处理.

- `EnterRawMode[] : Result[nil, InputError]`
  - 申请原始模式持有. 空闲时登记当前 Actor; 已由同一持有者持有时为空操作;
    由其他持有者持有时返回 `Err[InputError/RawModeHeld[]]`.
    标准输入不是终端时返回 `Err[InputError/NotATerminal[]]`;
    开启失败返回 `Err[InputError/Other[message]]`.
- `ExitRawMode[] : Result[nil, InputError]`
  - 释放自己持有的原始模式, 恢复进入前的终端状态; 没有持有者时为空操作.
    由其他持有者持有时返回 `Err[InputError/NotRawModeHolder[]]`.
    恢复失败返回 `Err[InputError/Other[message]]`.

持有者终止时自动释放. 释放时有其他 Actor 正在借用原始模式读取,
该读取以 `Err[InputError/RawModeReleased[]]` 结束.
原始模式下终端不回显, 也不把换行转换为回车换行, 输出须自行写出 `\r\n`.
程序结束时宿主清理持有者并恢复终端状态, 覆盖未调用 `ExitRawMode` 的路径.

## 4. 按键表示

按键事件是内置 Data 类型 `KeyEvent`, 变体为:
`Char[char]`, `Ctrl[char]`, `Alt[char]`, `F[u8]`,
`Enter[]`, `Escape[]`, `Backspace[]`, `Tab[]`, `BackTab[]`,
`Up[]`, `Down[]`, `Left[]`, `Right[]`, `Home[]`, `End[]`,
`PageUp[]`, `PageDown[]`, `Delete[]`, `Insert[]`.

修饰键合并到变体; Ctrl 与 Alt 同时按下时按 Ctrl 处理;
字母的 Shift 已体现在字符大小写中, Shift+Tab 单独表示为 `BackTab[]`.

## 5. 文件整体读写

UTF-8 无效由 `FileError/InvalidUtf8[path]` 表达; 字节接口不做 UTF-8 校验.

- `ReadFile[path : string] : Result[string, FileError]`
  - 读整个文本文件; 读取规模由文件大小决定, 不设语言级上限.
- `WriteFile[path : string, text : string] : Result[nil, FileError]`
  - 创建或截断后写入文本.
- `AppendFile[path : string, text : string] : Result[nil, FileError]`
  - 创建或追加文本.
- `ReadFileBytes[path : string] : Result[List[u8], FileError]`
  - 读整个文件字节; 读取规模由文件大小决定, 不设语言级上限.
- `WriteFileBytes[path : string, bytes : List[u8]] : Result[nil, FileError]`
  - 创建或截断后写入字节.
- `AppendFileBytes[path : string, bytes : List[u8]] : Result[nil, FileError]`
  - 创建或追加字节.

## 6. 目录与路径

路径谓词只做判断, 路径不存在或查询失败时返回 false.
谓词不保证后续操作成功, 路径可能在此期间变化, 实际操作仍须处理 `FileError`.

- `ListDirectory[path : string] : Result[List[string], FileError]`
  - 返回按名字排序的条目名; 不含 `.` 与 `..`, 不递归.
    任一条目名不是 UTF-8 时报 `FileError/Other[message]`.
- `CreateDirectory[path : string] : Result[nil, FileError]`
  - 创建单层目录.
- `CreateDirectoryAll[path : string] : Result[nil, FileError]`
  - 递归创建目录.
- `RemoveDirectory[path : string] : Result[nil, FileError]`
  - 只删除空目录.
- `RemoveFile[path : string] : Result[nil, FileError]`
  - 删除文件.
- `RenamePath[from : string, to : string] : Result[nil, FileError]`
  - 重命名路径.
- `CopyFile[from : string, to : string] : Result[nil, FileError]`
  - 复制文件.
- `JoinPath[left : string, right : string] : string`
  - 拼接路径; `right` 为绝对路径时结果为 `right`.
- `IsPathExists[path : string] : bool`
  - 路径存在时返回 true.
- `IsFile[path : string] : bool`
  - 路径是文件时返回 true.
- `IsDirectory[path : string] : bool`
  - 路径是目录时返回 true.
- `GetCurrentDirectory[] : Result[string, FileError]`
  - 返回宿主当前工作目录的绝对路径.

`GetCurrentDirectory` 查询失败返回 `FileError/Other[message]`. 不提供 `SetCurrentDirectory`.
文件与目录操作按宿主当前工作目录解释相对路径;
模块 `@Use` 路径仍相对模块文件目录, 二者不混用.

## 7. 文件流

文件流以不透明类型 `File` 表示, 句柄方向在打开时确定.
读句柄写入返回 `FileError/NotWritable[]`, 写句柄读取返回 `FileError/NotReadable[]`;
关闭或无效句柄上的操作返回 `FileError/InvalidHandle[]`.
句柄可复制, 关闭影响所有副本; 忘记 `Close` 时由宿主释放资源,
但写缓冲的刷新时机不保证, 长流程应显式 `Flush` 或 `Close`.

- `OpenRead[path : string] : Result[File, FileError]`
  - 打开读句柄.
- `OpenWrite[path : string] : Result[File, FileError]`
  - 截断或创建后打开写句柄.
- `OpenAppend[path : string] : Result[File, FileError]`
  - 追加或创建后打开写句柄.
- `Close[file : File] : Result[nil, FileError]`
  - 关闭句柄.
- `Flush[file : File] : Result[nil, FileError]`
  - 刷新写缓冲.
- `ReadLineFrom[file : File] : Result[Option[string], FileError]`
  - 按行读取文本; EOF 为 `Ok[None[]]`.
- `ReadChunk[file : File, size : u64] : Result[Option[List[u8]], FileError]`
  - 按字节块读取; size 必须为正数.
- `WriteString[file : File, text : string] : Result[nil, FileError]`
  - 写入文本.
- `WriteBytes[file : File, bytes : List[u8]] : Result[nil, FileError]`
  - 写入字节.
- `ClosedFile[] : File`
  - 返回处于关闭态的句柄; 任何读写都报 `FileError/InvalidHandle[]`.
    当前没有可用句柄而又必须给出 `File` 值时使用, 例如 `@ResultGet` 的默认值.

## 8. 错误

文件, 目录与文件句柄操作使用内置 Data 类型 `FileError`, 变体为:
`NotFound[path]`, `PermissionDenied[path]`, `AlreadyExists[path]`, `InvalidUtf8[path]`,
`NotADirectory[path]`, `IsADirectory[path]`, `DirectoryNotEmpty[path]`,
`InvalidHandle[]`, `NotReadable[]`, `NotWritable[]`, `Other[message]`.

标准输入与终端操作使用内置 Data 类型 `InputError`, 变体为:
`NotATerminal[]`, `Interrupted[]`, `RawModeActive[]`, `RawModeHeld[]`,
`NotRawModeHolder[]`, `RawModeReleased[]`, `Other[message]`.

## 9. 环境与参数

- `Args[] : List[string]`
  - 返回命令行参数; 无参数时为空列表.
    启动后参数由全部任务共享, Actor 内读取结果与程序根一致.
- `GetEnv[name : string] : Option[string]`
  - 返回环境变量; 未设置时返回 `Option/None[]`.

## 10. 本机命令

一次性启动本机程序并捕获输出, 不经过 shell.
`program` 与 `args` 分别作为程序名与参数向量传入, `PATH` 从宿主进程继承.
子进程的 stdin 默认关闭; 不提供 PTY 或交互式终端支持.
`stdout` 与 `stderr` 分开捕获, 每路最多 8 MiB.

- `RunCommand[program : string, args : List[string], options : List[CommandOption]] : Result[CommandOutput, CommandError]`
  - 启动并等待程序结束. 非零退出码不是 `Err`, 由 `CommandOutputStatus` 返回.
  - `options` 为空时写 `{}`.
- `CommandOutputStatus[output : CommandOutput] : CommandStatus`
  - 返回退出状态. 正常退出为 `CommandStatus/Exit[code]`, 信号终止为 `CommandStatus/Signal[signal]`.
    Windows 不产生 `Signal`.
- `CommandOutputStdout[output : CommandOutput] : List[u8]`
  - 返回标准输出的原始字节. 需要文本时使用 `DecodeUtf8`.
- `CommandOutputStderr[output : CommandOutput] : List[u8]`
  - 返回标准错误的原始字节. 需要文本时使用 `DecodeUtf8`.

`CommandOutput` 是不透明类型, 不能由用户构造; REPL 只显示 `<CommandOutput>`.

命令选项为内置 Data 类型 `CommandOption`:

- `CommandOption/Cwd[string]`
  - 子进程工作目录. 不存在或不是目录返回 `CommandError/InvalidCwd[path]`.
- `CommandOption/Env[List[Pair[string, string]]]`
  - 在继承宿主环境的基础上覆盖同名变量; 列表内出现同名变量返回 `CommandError/DuplicateEnv[name]`.
- `CommandOption/Input[CommandInput]`
  - 写入标准输入后关闭; 未提供时标准输入关闭.

同一选项只能出现一次; 重复时返回 `CommandError/DuplicateOption[option]`.
`CommandInput` 的变体为 `CommandInput/Text[string]` 与 `CommandInput/Binary[List[u8]]`.
子进程提前退出导致写入标准输入的 `BrokenPipe` 被忽略; 其他写入错误返回 `CommandError/Other[message]`.

命令失败使用内置 Data 类型 `CommandError`, 变体为:
`NotFound[program]`, `PermissionDenied[program]`, `InvalidCwd[path]`,
`DuplicateOption[option]`, `DuplicateEnv[name]`, `OutputTooLarge[stream]`, `Other[message]`.
`OutputTooLarge` 的 `stream` 为 `"stdout"` 或 `"stderr"`; 两路同时超限时报先检测到超限的一路.
超过输出上限时终止子进程, 不返回部分输出.
子进程与读取线程不设独立健康检查; 孙进程长期持有输出管道时, 对应读取线程会持续占用一个阻塞线程.
