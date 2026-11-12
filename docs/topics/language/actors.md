---
updated: 2026-11-26
---

# ksl Actor 运行时

ksl 的 Actor 模型, 调度器语义与原语规则.
原语签名与清单见 `docs/topics/library/special-forms.md`,
执行模型, 效果边界与环回收见 `docs/topics/implementation/scheduler.md` 第 1 节至第 6 节.

## 1. 模型

Actor 行为由 Handler 定义, 挂起状态是续延帧栈.
调度器按固定指令预算协作推进任务, 同一时刻只有一个片段在执行;
单 Actor 内的输出顺序可复现, Actor 之间按就绪与消息到达顺序交错.
I/O 与线程池调用不阻塞其他 Actor, 长原生原语会独占调度线程.

- 顶层语句是 Actor 之外的 Prelude, 与 Actor 一起调度.
- REPL 会话不使用 Actor: 交互输入禁止 Actor 原语,
  失败的输入回滚全局绑定, 且不提交本次的类型声明与模块缓存.

## 2. 蓝图与类型

`@Handler[[self : Pid[...], 状态...] : R, 体]` 是表达式, 值类型为 `Handler[...]`.
求值只构造蓝图, 不执行体.

- Handler 与 `Fn` 是不同类别, 不能作为函数调用.
- Handler 可以像匿名函数一样被 `@Let` 绑定, 作为参数, 返回值与消息字段传递,
  但只能由 `@Spawn` 启动.
- 需要名字时用 `@Let[name, @Handler[...]]`, 不设专门的具名 Handler 语句.
- Handler 循环: Handler 体内用 `@Continue[新状态...]` 以同一个 self 重新进入当前 Handler.
  `@Continue` 类型为 never, 参数对应 Handler 除 self 外的状态参数.
- Handler 不通过名字自递归, 因此不同 Handler 之间无法直接相互引用,
  只能通过 `@Spawn` 与消息通信.

Actor 的类型是 `Pid[T0, ..., Tn-1, R]`, 前 n 个参数依次为信箱 0..n-1 的消息类型,
R 为返回类型; n 至少为 1, 至多 16.
`Handler[Pid[T0, ..., Tn-1, R], A1, ..., Ak]` 是蓝图类型, 状态参数为 A1..Ak.
`@Spawn[handler, 实参...]` 的 handler 必须是 Handler 类型, 实参匹配 A1..Ak,
返回 `Pid[T0, ..., Tn-1, R]`. 信箱数由 handler 的 Pid 类型推断, 不在调用点重复书写.

## 3. 信箱

每个 Actor 有 n 个信箱, n 为 Pid 类型参数个数减一.
`@Spawn[handler, ...]` 直接使用 handler 的 Pid 类型中的 n.
各信箱独立排队; 同一发送者到同一信箱的消息严格保序;
不同发送者之间的交错由实际发送时刻决定; 跨信箱无序.

- `@Send` 与 `@Receive` 的信箱索引必须是 u8 整数字面量, 按位置选择消息类型.
- 信箱布局由用户决定, 没有保留的控制信箱.
  只有一个信箱时无需区分索引; 需要区分数据与控制消息时, 由调用方约定不同索引.
- 信箱索引按 u8 字面值处理, 必须写 `u8` 后缀, 例如 `1u8`.
  超出 u8 范围报字面量越界; 非字面值报 `U8LiteralRequired`.
- 信箱消息不做挑选: 不提供按变体选择消息的机制, 投递顺序即到达顺序.
  需要取消某类消息时, 用多信箱与接收超时实现.
- `@Send` 的消息类型必须匹配该索引对应的信箱类型; 索引越界在类型检查阶段报 `MailboxIndexOutOfRange`.
  返回值表示消息是否被目标信箱接受; 目标已终止时返回 false 并丢弃消息,
  但 true 不保证目标已经处理.
- `@CancelTimer` 只允许创建者取消尚未触发的定时消息, 已投递的无法撤回.
- `@Kill` 返回目标本来是否存活.
- 超时与 Sleep 时长不设语言级上限; 调度器若无法表示截止时间, 报运行时错误.
- 没有 `@Self` 原语, pid 即 Handler 的首参数.

## 4. 词法位置

Actor 原语族只允许出现在 `@Handler` 体与 `@Main` 体内,
包括 `@Spawn`, `@SpawnDetached`, `@Send`, `@Join`, `@Kill`,
`@PidSerial`, `@SetTimer`, `@CancelTimer`,
`@Receive`, `@Sleep`, `@Halt` 与 `@Continue`.
模块顶层, 函数体, 交互输入与命令行代码顶层一律禁止;
嵌套匿名 `@Fn` 或 `@Def` 不继承该上下文, 嵌套 `@Handler` 有自己的上下文.

`@Handler` 蓝图表达式是普通值, 任意位置可用.
`@Spawn` 的 handler 实参必须是 `Handler[...]` 类型.
库模块需要 Actor 时导出 Handler, 由使用方 Spawn.
`@Sleep` 仅限 Handler 与 `@Main` 体内使用; 顶层语句不提供 Sleep.

## 5. 生命周期

由 `@Spawn` 启动的 Actor 隶属于父 Actor. 父 Actor 终止时子 Actor 一并终止,
包括父终止时尚未启动的子 Actor; 这是单向归属, 子 Actor 终止不影响父.
需要长驻且独立于父的 Actor 时用 `@SpawnDetached`.

由 `@Main` 生成的 Actor 在程序退出时统一终止.
Actor 终止时清理以它为目标的未触发延迟消息.

终止原语:

- `@Halt[]` 无参, 仅用于 Handler 终止自身, `@Main` 中禁止.
  从任意深度展开至 Handler 入口结束.
- 外部有序停止不专设原语, 以约定消息表达; Actor 收到后在消息分支内完成清理,
  再以 `@Halt[]` 结束.
- `@Kill[pid]` 为外部强制终止, 调度器在下一个让出点唤醒并终止目标.
  目标为存活的根 Actor 时报 `KillRoot`;
  对已终止或不存在的 Pid, `@Kill` 无副作用.

Actor 失败只终止该 Actor. 批处理退出码只由程序根决定;
失败通过 `@Join` 观察. 已观察的失败不再重复诊断;
观察以 Join 结果实际交付为准; 只登记 Join 但未完成交付的失败仍按未观察处理.
未观察的失败以 `note: Actor 失败, 不影响退出码` 加原诊断输出到标准错误.

恢复: 监督者通过 `@Join` 取得退出原因后按策略重新启动,
状态从初值重建. 策略, 包括何时重启与重启次数上限, 属于库层;
语言只提供 `@Join` 与 `@Spawn`.
需要非阻塞监督时, 由库层为每个目标启动 watcher Actor, watcher 内 `@Join`,
结束后再以消息通知监督者.

## 6. 退出原因与 Join

退出原因是内置 Data 类型 `ExitReason`, 变体为 `Halted[]`,
`Killed[]` 与 `Failed[编号 : i64, 说明 : string]`.
正常返回与 `@Halt[]` 都记为 `Halted`; 需要返回值时用 `@Join`.
退出原因保存在 Pid 值中; Pid 值释放后, 记录也随之释放.

Actor 内运行时错误终止该 Actor 并产生 `Failed`; 程序根错误终止整个程序.
`@Exit` 不允许出现在普通 Actor 内; `@Main` 根 Actor 是唯一例外,
其 `@Exit` 映射为程序退出码.

Join 原语: `@Join[pid] : Result[R, ExitReason]`, 挂起调用方直至目标 Actor 终止.
正常返回为 `Result/Ok[返回值]`, 异常终止为 `Result/Err[退出原因]`; 目标已终止时立即返回它当初的结果.
多个 Join 等待者共享同一结果, 并在目标终止时一并唤醒.
自 Join 与 Join 根 Pid 都报运行时错误.

Join 只等待不通知; 需要在处理其他消息的同时察觉目标终止时,
由库层启动 watcher Actor 代为 Join.

Pid 编号: 调度器内单调递增分配, 不重用.
`@PidSerial[pid] : u64` 返回该编号, 仅在当前会话内稳定, 不可持久化.

消息携带 Pid: 不提供 `@Request`. 需要回复时,
在消息 Data 类型的字段里写入发送方的具体 `Pid` 类型, 接收方按该类型 `@Send` 回复.
单态类型下 Pid 类型必须完整写出, 不能从消息内容反推;
不同 Pid 类型的客户端使用不同变体或不同协议 Data 类型.
递归 Data 类型与互相引用的 Pid 类型由声明预收集支持.

需要把 Pid 作为键或日志标识时用 `@PidSerial`.

## 7. 主 Actor 与标准 I/O 服务

主 Actor: 模块可以 `@Main[[self : Pid[T0, ..., nil]] : nil, 体]` 声明模块入口.
语法与 `@Handler` 同构, 但语义是声明, 不产生值; 参数表只有 self, 返回类型固定为 nil,
self 的名字由用户选择. 体是单个表达式; 多条语句用 `@Block[...]` 包裹.

- `@Main` 体内可用除 `@Halt` 与 `@Continue` 外的全部 Actor 原语;
  `@Exit` 允许出现在 `@Main` 体内并映射为程序退出码.
- 直接运行时, 根模块的 `@Main` 作为程序入口执行, 依赖模块的 `@Main` 跳过.
- 顶层语句是先于根 Actor 的 Prelude, 只负责绑定与 I/O, 不含 Actor 原语.
- `@Main` 根 Actor 走普通 Actor 任务路径, 正常返回 nil, 运行时错误终止程序,
  子 Actor 随根结束终止.
- 没有 `@Main` 的模块不运行根 Actor.

标准 I/O 服务由运行时拥有, 不是语言层 Actor, 不暴露 Pid 与 Handler.
输出顺序, 输入请求与原始模式的行为见 `docs/topics/library/builtins-io.md` 第 1 节至第 3 节.
