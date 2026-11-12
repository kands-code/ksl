---
updated: 2026-11-27
---

# 关于 ksl

ksl 是一个个人编程语言项目, 语法采用 Wolfram Language 风格的前缀调用, 使用 Rust 实现.
语言规则与实现见 `docs/topics/`, 未实现方向与待确认事项见 `docs/roadmap.md`.

## 1. 定位

- 个人项目, 以作者自身需求为准.
- 通用语言, 定位接近 Python 一类命令式脚本语言, 不以函数式特性为目标.
- 运行模式以脚本为主, REPL 为辅.
- 静态类型, 类型标注由用户书写, 绑定标注可省略, 全部单态;
  不引入 trait 与子类型, Data 类型覆盖联合类型.

## 2. 语言形态

- 只有前缀调用与 `@` 特殊形式; 没有中缀表达式, 优先级与结合性.
- 绑定不可变, 状态更新生成新值; 并发只使用 Actor 模型.
- 迭代通过递归表达; 失败用 `Result` 与 `Option` 表示, 不设异常机制.
- 不支持元编程与 quote.
- 内置能力由内置函数与特殊形式提供, 不设独立标准库与模块搜索路径.

## 3. 文档索引

### 3.1 项目

| 文档                           | 内容                         |
| ------------------------------ | ---------------------------- |
| `docs/tutorial.md`             | 教程                         |
| `docs/roadmap.md`              | 远期方向与待确认事项         |
| `docs/glossary.md`             | 术语表                       |
| `docs/中文科技文档协作规范.md` | 中文科技文档与注释的协作规范 |

### 3.2 语言

| 文档                                 | 内容                          |
| ------------------------------------ | ----------------------------- |
| `docs/topics/language/syntax.md`     | 词法与语法, 字面值形态        |
| `docs/topics/language/types.md`      | 类型体系与类型检查规则        |
| `docs/topics/language/evaluation.md` | 求值模型, 绑定与作用域        |
| `docs/topics/language/functions.md`  | 函数定义, 递归规则与闭包捕获  |
| `docs/topics/language/data.md`       | Data 类型, 字段访问与模式匹配 |
| `docs/topics/language/errors.md`     | 错误目录, 错误作用域与退出    |
| `docs/topics/language/modules.md`    | 模块组织, 导入导出与加载      |
| `docs/topics/language/actors.md`     | Actor 模型与原语规则          |

### 3.3 内置能力

| 文档                                            | 内容                                           |
| ----------------------------------------------- | ---------------------------------------------- |
| `docs/topics/library/builtins.md`               | 内置能力总览, 命名约定与保留名                 |
| `docs/topics/library/builtins-arithmetic.md`    | 算术, 绝对值与取整                             |
| `docs/topics/library/builtins-math.md`          | 幂, 根, 三角与双曲函数                         |
| `docs/topics/library/builtins-bitwise.md`       | 位运算与移位                                   |
| `docs/topics/library/builtins-convert.md`       | 数值转换                                       |
| `docs/topics/library/builtins-compare.md`       | 数值比较与布尔                                 |
| `docs/topics/library/builtins-string.md`        | 字符串, 字符, 编解码与解析                     |
| `docs/topics/library/builtins-regex.md`         | 正则表达式                                     |
| `docs/topics/library/builtins-list.md`          | 列表与配对                                     |
| `docs/topics/library/builtins-dict-set.md`      | 字典与集合                                     |
| `docs/topics/library/builtins-option-result.md` | Option 与 Result                               |
| `docs/topics/library/builtins-io.md`            | 输出, 标准输入, 终端按键与原始模式, 文件与目录 |
| `docs/topics/library/builtins-time.md`          | 日期时间                                       |
| `docs/topics/library/builtins-network.md`       | TCP 原语                                       |
| `docs/topics/library/special-forms.md`          | 核心与 Actor 特殊形式                          |

### 3.4 实现

| 文档                                         | 内容                           |
| -------------------------------------------- | ------------------------------ |
| `docs/topics/implementation/architecture.md` | 流水线, 类型检查与宿主生命周期 |
| `docs/topics/implementation/ir.md`           | CPS IR 与线性 IR               |
| `docs/topics/implementation/scheduler.md`    | 调度器, 线程池与网络 I/O       |
| `docs/topics/implementation/runtime.md`      | 运行时值表示与全局环境         |
| `docs/topics/implementation/cli.md`          | 命令行, 交互模式与语言服务器   |
| `docs/topics/implementation/engineering.md`  | 工程约定与依赖                 |
| `docs/topics/implementation/plugins.md`      | 原生插件接口与编写方式         |

### 3.5 示例与工具

| 文档                                         | 内容                     |
| -------------------------------------------- | ------------------------ |
| `examples/README.md`                         | 示例列表与运行方式       |
| `bench/README.md`                            | 基准用例与测量结果       |
| `crates/ksl-runtime/tests/scripts/README.md` | 脚本测试用例写法         |
| `crates/ksl-treesitter/README.md`            | tree-sitter 语法与编辑器 |
| `plugins/raylib/README.md`                   | raylib 插件构建与示例    |
