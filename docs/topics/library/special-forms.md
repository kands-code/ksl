---
updated: 2026-11-26
---

# ksl 特殊形式

ksl 的核心与 Actor 特殊形式. 操作数形状与结果类型在此.

容器相关的特殊形式见 `docs/topics/library/builtins-list.md` 与 `docs/topics/library/builtins-dict-set.md`;
Option 与 Result 的操作见 `docs/topics/library/builtins-option-result.md`;
类型记号见 `docs/topics/library/builtins.md` 第 3 节.

## 1. 绑定与作用域

- `@Let[target, value] : nil`
  - 绑定变量. 绑定目标形如 `名字 : 类型`, `名字` 或 `_`; 标注可省略.
    目标为 `_` 时显式丢弃值. 新绑定遮蔽外层同名绑定.
- `@Block[statements...] : T`
  - 作用域块. 块内新定义对外层不可见; 空块为 nil.
    T 为最后一条语句的类型.

## 2. 条件分支

- `@If[condition : bool, then, else] : T`
  - 条件分支. 条件必须为 bool; 两个分支与结果共享同一类型变量.
    有期望类型时, 结果先与期望统一.
- `@Which[{(condition : bool, value), ...}, fallback] : T`
  - 多路条件分支. 条件从左到右求值, 命中第一项为真的分支;
    没有命中时求值 fallback. fallback 必填.
    各分支值与 fallback 共享同一类型变量; 有期望类型时, 结果先与期望统一.
    空分支表结果为 fallback; 条件为字面量 `true` 或 `false` 时报错.

## 3. 函数

- `@Fn[[parameters...] : R, body] : Fn[[...] : R]`
  - 匿名函数. 参数类型与返回类型必须标注; 不自递归.
- `@Def[name, [parameters...] : R, body] : nil`
  - 具名函数定义. 值为 nil; 默认自递归.
- `@Fix[definitions...] : nil`
  - 互递归定义组. 只接受具名 `@Def`; 组内名字相互可见; 空组视为 nil.

## 4. 数据类型与匹配

- `@Data[name, variants] : nil`
  - 声明 Data 类型. 仅限顶层; 至少要有一个变体.
- `@Match[value, branches] : T`
  - 按变体分派. 分支形如 `(模式, 结果)`; 匹配必须穷尽.
- `@DataGet[value, index : u8] : T`
  - 按位置读取单变体 Data 的字段. 下标必须是 u8 字面量; 多变体报错.

## 5. 退出与主动中断

- `@Exit[code : u8] : never`
  - 终止程序并以指定码退出. 只能出现在程序根或 `@Main` 体内.
- `@Assert[condition : bool, message : string] : nil`
  - 条件为真则通过, 为假则断言失败.
- `@Panic[message : string] : never`
  - 主动失败.
- `@Todo[message : string] : never`
  - 主动失败, 标记未完成.
- `@Unreachable[message : string] : never`
  - 主动失败, 标记不可达.

## 6. 模块

- `@Export[items...] : nil`
  - 声明导出条目. 只能出现在顶层; 条目为名字或 `^类型`; 重命名配对只用于 `@Use`.
- `@Use[{items...}, path : string] : nil`
  - 导入模块条目. 只能出现在顶层; 模块路径相对当前模块文件所在目录.
- `@Link[plugin : string, {^types...}, {name : type, ...}] : nil`
  - 声明插件提供的不透明类型与外来值. 只能出现在模块中.
    名字, 签名与加载规则见 `docs/topics/language/modules.md` 第 2.1 节,
    原生插件接口见 `docs/topics/implementation/plugins.md`.

## 7. Actor 蓝图与模块入口

- `@Handler[[self : Pid[...], states...] : R, body] : Handler[Pid[...], ...]`
  - Actor 蓝图. 表达式; 求值只构造蓝图, 不执行体.
- `@Main[[self : Pid[...]] : nil, body] : nil`
  - 模块入口声明. 不是值; 不能绑定, 传递或导出.
  - 参数表只有 self, 返回类型固定为 nil; 体是单个表达式, 多条语句用 `@Block[...]` 包裹.
  - 只有根模块的入口由加载器内部构造并启动; 依赖模块的入口只检查.
- `@Continue[states...] : never`
  - 以同一个 self 重新进入当前 Handler; 参数对应 Handler 除 self 外的状态参数.

## 8. Actor 原语

Actor 原语只允许出现在 `@Handler` 体与 `@Main` 体内.
信箱索引必须是 u8 整数字面量, 必须写 `u8` 后缀, 例如 `1u8`;
非字面值报 `U8LiteralRequired`.

- `@Spawn[handler : Handler[Pid[...], ...], args...] : Pid[...]`
  - 启动 Actor. 隶属于父 Actor.
- `@SpawnDetached[handler : Handler[Pid[...], ...], args...] : Pid[...]`
  - 启动 Actor. 不隶属于父.
- `@Send[pid : Pid[...], index : u8, message] : bool`
  - 向指定信箱投递消息; 目标已终止时返回 false 并丢弃消息.
- `@Receive[index : u8, timeout : Option[u64]] : Option[T]`
  - 从指定信箱接收消息; T 为该信箱的消息类型.
    超时为 None 表示无限等待, Some[0] 为非阻塞轮询.
- `@Join[pid : Pid[...]] : Result[R, ExitReason]`
  - 等待目标 Actor 终止; 正常返回 Ok[返回值], 异常终止返回 Err[ExitReason].
- `@Kill[pid : Pid[...]] : bool`
  - 强制终止目标 Actor; 返回目标本来是否存活.
- `@PidSerial[pid : Pid[...]] : u64`
  - 返回调度器内的 Actor 编号; 仅在当前会话内稳定, 不可持久化.
- `@SetTimer[pid : Pid[...], index : u8, message, delay : u64] : Option[u64]`
  - 延迟 delay 毫秒后向指定信箱投递消息; 返回定时编号, 目标已终止时返回 None.
- `@CancelTimer[id : u64] : bool`
  - 取消尚未触发的定时消息; 仅创建者可取消, 已投递的无法撤回.
- `@Halt[] : never`
  - 终止自身; 仅限 Handler 体内.
- `@Sleep[millis : u64] : nil`
  - 挂起当前 Actor 指定毫秒; 仅限 Handler 与 `@Main` 体内.

无 `@Self` 原语, pid 即 Handler 的首参数.
