---
updated: 2026-11-21
---

# 关于 ksl

ksl 是一个个人编程语言项目, 语法采用 Wolfram Language 风格的前缀调用, 使用 Rust 实现.
当前设计细节见 `docs/topics/` 下的各篇, 远期方向与待确认事项见 `docs/roadmap.md`.

## 1. 定位

- 个人娱乐项目, 以作者自身需求为准.
- 通用语言, 定位接近 Python 一类命令式脚本语言, 不以函数式特性为目标.
- 运行模式以脚本为主, REPL 为辅.
- 采用静态类型, 用户书写类型标注, 全部单态, 不允许多态, 不引入 trait.
  Data 类型覆盖联合类型.

## 2. 语言形态

- 语法只保留前缀调用与 `@` 特殊形式, 不提供中缀表达式, 没有优先级与结合性.
- 所有绑定都不可变, 状态更新通过生成新值完成, 不提供共享可变状态原语.
- 并发只使用 Actor 模型: 绿色线程, 消息通信, 唯一所有者.
- 迭代通过递归表达, 不提供循环原语.
- 不支持元编程与 quote, 不设异常机制, 失败用 `Result` 与 `Option` 表示.
- 内置能力由内置函数与特殊形式提供, 暂不引入独立标准库与模块搜索路径.

## 3. 文档索引

| 文档                                         | 内容                                           |
| -------------------------------------------- | ---------------------------------------------- |
| `docs/tutorial.md`                           | 教程                                           |
| `docs/roadmap.md`                            | 远期方向与待确认事项                           |
| `docs/glossary.md`                           | 术语表                                         |
| `docs/中文科技文档协作规范.md`               | 中文科技文档与注释的协作规范                   |
| `docs/topics/language/syntax.md`             | 词法与语法, 字面值形态                         |
| `docs/topics/language/types.md`              | 类型体系与类型检查规则                         |
| `docs/topics/language/evaluation.md`         | 求值模型, 绑定与作用域                         |
| `docs/topics/language/functions.md`          | 函数定义, 递归规则与闭包捕获                   |
| `docs/topics/language/data.md`               | Data 类型与模式匹配                            |
| `docs/topics/language/errors.md`             | 错误目录, 错误作用域与主动中断                 |
| `docs/topics/language/modules.md`            | 模块组织, 导入导出与加载语义                   |
| `docs/topics/language/actors.md`             | Actor 模型与原语规则                           |
| `docs/topics/library/builtins.md`            | 内置能力总览, 命名约定与保留名                 |
| `docs/topics/library/builtins-numeric.md`    | 数值, 比较, 位运算与转换                       |
| `docs/topics/library/builtins-string.md`     | 字符串, 字符, 编解码与解析                     |
| `docs/topics/library/builtins-regex.md`      | 正则表达式                                     |
| `docs/topics/library/builtins-collection.md` | 列表, 字典, 集合, Option 与 Result             |
| `docs/topics/library/builtins-io.md`         | 输出, 标准输入, 终端按键与原始模式, 文件与目录 |
| `docs/topics/library/builtins-time.md`       | 日期时间                                       |
| `docs/topics/library/builtins-network.md`    | TCP 原语                                       |
| `docs/topics/library/special-forms.md`       | 核心与 Actor 特殊形式                          |
| `docs/topics/implementation/architecture.md` | 解释器架构, 调度器与 I/O 执行                  |
| `docs/topics/implementation/cli.md`          | 命令行, 交互模式与语言服务器                   |
| `docs/topics/implementation/engineering.md`  | 工程约定与依赖                                 |
| `docs/topics/implementation/plugins.md`      | 插件 ABI 与编写方式                            |
