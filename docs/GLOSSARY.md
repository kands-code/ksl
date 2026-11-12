---
updated: 2026-11-12
---

# 术语表

本文档是项目全部文档与代码注释用词的唯一依据, 新增术语须同步登记.

| 英文原词                    | 缩写 | 中文译名       | 允许英文原词 | 备注                                                              |
| --------------------------- | ---- | -------------- | ------------ | ----------------------------------------------------------------- |
| Abstract Syntax Tree        | AST  | 抽象语法树     | 是           | 解释器内部概念                                                    |
| call-by-value               | -    | 传值调用       | 是           | 求值策略                                                          |
| Continuation-Passing Style  | CPS  | 续延传递风格   | 是           | 执行模型, 正文直接使用缩写                                        |
| Intermediate Representation | IR   | 中间表示       | 是           | 降级目标, 正文直接使用缩写                                        |
| Typed IR                    | -    | 类型化中间表示 | 是           | 携带类型信息的 IR; 正文优先使用 typed IR                          |
| Unicode                     | -    | -              | 是           | 直接使用, 不译                                                    |
| UTF-8                       | -    | -              | 是           | 直接使用, 不译                                                    |
| IEEE 754                    | -    | -              | 是           | 浮点数标准, 直接使用                                              |
| Not a Number                | NaN  | -              | 是           | IEEE 754 特殊值, 直接使用                                         |
| Conventional Commits        | -    | -              | 是           | 提交信息规范, 直接使用                                            |
| REPL                        | -    | 交互式解释器   | 是           | 读取求值打印循环, 直接使用                                        |
| Shadowing                   | -    | 遮蔽           | 是           | 内层绑定遮蔽外层同名绑定                                          |
| Interning                   | -    | 驻留           | 是           | 字符串驻留, 标识符压为 u32                                        |
| Symbol                      | -    | 符号           | 是           | 驻留后的标识符形态                                                |
| String                      | -    | -              | 是           | 独立类型, 不译                                                    |
| Arc                         | -    | -              | 是           | Rust 的引用计数指针                                               |
| Actor                       | -    | -              | 是           | 并发原语, 不译                                                    |
| Process Identifier          | Pid  | Actor 句柄     | 是           | 内建参数化类型, 直接使用                                          |
| Handler                     | -    | Actor 蓝图     | 是           | 运行时类型, 直接使用                                              |
| Data                        | -    | -              | 是           | 用户定义的联合类型, 语言关键字                                    |
| Option                      | -    | -              | 是           | 内置容器, 不译                                                    |
| Result                      | -    | -              | 是           | 内置容器, 不译                                                    |
| List                        | -    | -              | 是           | 内置容器, 不译                                                    |
| Dict                        | -    | 字典           | 是           | 内置容器, 键限基础标量                                            |
| Set                         | -    | 集合           | 是           | 内置容器, 元素限基础标量                                          |
| Pair                        | -    | 配对           | 是           | 内置容器                                                          |
| I/O                         | -    | 输入输出       | 是           | 直接使用, 不译                                                    |
| IOError                     | -    | I/O 错误       | 是           | 内置 Data 类型                                                    |
| ExitReason                  | -    | 退出原因       | 是           | 内置 Data 类型                                                    |
| File                        | -    | 文件句柄       | 是           | 内置不透明类型                                                    |
| DateTime                    | -    | 日期时间       | 是           | 内置不透明类型, 绝对时刻加时区                                    |
| TimeError                   | -    | 时间错误       | 是           | 内置 Data 类型                                                    |
| KeyEvent                    | -    | 按键事件       | 是           | 内置 Data 类型, 终端按键的表示                                    |
| Raw Mode                    | -    | 原始模式       | 是           | 终端输入模式, 关闭行缓冲与回显                                    |
| Halt                        | -    | 停机           | 是           | Actor 主动停止的特殊形式                                          |
| Halted                      | -    | 已停机         | 是           | ExitReason 变体                                                   |
| Monitor                     | -    | 监视           | 是           | Actor 终止通知特殊形式                                            |
| Unmonitor                   | -    | 取消监视       | 是           | 注销监视的特殊形式                                                |
| PidSerial                   | -    | Pid 编号       | 是           | 调度器内 Actor 编号特殊形式                                       |
| SetTimer                    | -    | 延迟投递       | 是           | 延迟发送消息的特殊形式                                            |
| CancelTimer                 | -    | 取消定时       | 是           | 取消未触发延迟消息的特殊形式                                      |
| Type Erasure                | -    | 类型擦除       | 是           | 运行时值不携带完整静态类型                                        |
| LSP                         | -    | -              | 是           | Language Server Protocol                                          |
| XID                         | -    | -              | 是           | Unicode 标识符字符分类                                            |
| Module                      | -    | 模块           | 是           | 文件即模块                                                        |
| Export                      | -    | 导出           | 是           | @Export 声明                                                      |
| Import                      | -    | 导入           | 是           | @Use 声明                                                         |
| Alias                       | -    | 别名           | 是           | 导入类型的本地名称                                                |
| Inline Import               | -    | 行内导入       | 否           | 交互输入或命令行代码中的 @Use                                     |
| Binary Prefix               | -    | -              | 是           | 存储单位约定: 内存容量用 GiB, MiB, TiB; 磁盘与网络带宽用 GB, Gbps |
| Tree-sitter                 | -    | -              | 是           | 增量解析器生成器, 直接使用, 不译                                  |
| External Scanner            | -    | 外部扫描器     | 是           | tree-sitter 语法中由手写代码识别的记号                            |
| Drift                       | -    | 漂移           | 是           | 同一语言规则的两份实现互相分叉                                    |
