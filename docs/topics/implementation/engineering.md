---
updated: 2026-11-27
---

# ksl 工程约束

ksl 的工程约定, 面向贡献者.
语言语义见 `docs/topics/language/` 下的各篇, 实现架构见 `docs/topics/implementation/architecture.md`
与同目录的 `ir.md`, `scheduler.md`, `runtime.md`.

## 1. 工程结构

- Rust workspace, 项目名 ksl, crate 命名使用连字符.
  crate 分为 `ksl-base`, `ksl-catalog`, `ksl-macros`, `ksl-diagnostics`,
  `ksl-plan`, `ksl-typed`, `ksl-frontend`, `ksl-compile`, `ksl-ir`,
  `ksl-value`, `ksl-io`, `ksl-runtime`, `ksl-host`, `ksl-cli` 与 `ksl-treesitter`.
- 插件位于 `plugins/`: `plugins/demo` 是原生插件测试插件, `plugins/raylib` 是 raylib 包装.
  `default-members` 只包含语言实现; `--workspace` 会检查插件.
  raylib 发布包放在被忽略的 `plugins/raylib/vendor/` 下, 由构建脚本按平台选择.
- 工具链为最新 Nightly, 最低支持 Rust 1.100.0, edition 2024.
- 许可证为 AGPL-3.0-or-later.

## 2. 依赖与数据表示

依赖使用最新稳定版本. 主要依赖按用途分组:

- 诊断: annotate-snippets.
- 数据结构: imbl, arcstr, indexmap.
- 并发与 I/O: crossbeam-channel, parking_lot, mio, socket2, shared_child.
- 文本处理: unicode-ident, unicode-segmentation, unicode-width, regex-automata, regex-syntax.
- 类型检查: ena.
- 时间与终端: jiff, termina.
- 语言服务器: lsp-server, serde_json, gen-lsp-types, 启用 url 特性.
- 过程宏与编辑器语法: proc-macro2, quote, syn, tree-sitter, tree-sitter-language.
- 动态库与构建: libloading, cc.
- 工作区内部: ksl-macros 提供错误编号与原生插件导出宏.

CLI 参数解析自行实现. 进程统一使用系统分配器:
原生插件与宿主各持一份运行时副本, 跨副本释放要求同一分配器.
CLI 端到端测试使用标准库 `std::process::Command`,
公共辅助代码位于 `crates/ksl-cli/tests/common.rs`, 测试临时目录位于 `target/tmp`.

映射与结构共享:

- 有序映射使用 indexmap. Data 收集阶段按声明顺序处理, 首个错误可复现.
- 需要结构共享的会话状态使用 imbl 持久化映射: Data 类型表, 类型名绑定表, 检查环境作用域与模块缓存外层.
  Data 类型表只按名字查找; 变体顺序由 `DataInfo` 内的 `IndexMap` 保持.
- 其余只做查找或成员判断的表用 HashMap.
- 顺序只在可观察输出, 首个错误, 兼容别名选择与执行顺序处需要保证;
  仅用于查找的表不承诺迭代顺序. 编译器在首个错误处终止.

## 3. 代码质量

- clippy 严格检查, 问题优先自动修复. `clippy.toml` 放宽测试代码的 unwrap 与 expect.
- release 构建启用 fat LTO 与单一代码生成单元.
- rustfmt 启用 Nightly 不稳定选项, 配置见 `rustfmt.toml`.
- 导入按标准库, 外部 crate, crate 内路径分组, 组间空一行;
  crate 内路径分组内 `use super::*` 居首.
- 子模块使用父文件 `foo.rs` 加子目录 `foo/`, 不使用 `mod.rs`.
- 手写 C 源文件与插件头文件由 `scripts/check-c.sh` 调用 clang-format 检查,
  配置见 `.clang-format`. 生成的 `crates/ksl-treesitter/src/parser.c` 与 `src/tree_sitter`
  不参与格式化.

以下 clippy 放宽须在代码处说明理由:

| 规则                       | 放宽原因                                      |
| -------------------------- | --------------------------------------------- |
| `too_many_lines`           | 集中分派或线性流程, 拆分后割裂规则            |
| `unnecessary_wraps`        | 内置函数构造器统一返回 `Result`, 由注册表分派 |
| `cast_precision_loss`      | 数值转换, 范围已显式校验                      |
| `cast_possible_truncation` | 数值转换, 范围已显式校验                      |
| `float_cmp`                | 相等比较按 IEEE 语义                          |
| `too_many_arguments`       | 插件导出参数与 raylib 声明字段一一对应        |
| `wildcard_imports`         | FFI 绑定名与 C 声明一一对应                   |

## 4. 测试

测试遵循最小原则, 仅作为排错手段, 不以数量作为指标.

- 语言行为写在 `crates/ksl-runtime/tests/scripts/run` 下, 每个 `.ksl` 文件是一个执行用例.
  同能力的场景可以合并在一个文件内, 由同一个 `@Main` 顺序驱动.
- 错误路径断言 `ErrorKind`, 不比较错误编号.
  前端错误位于 `crates/ksl-frontend/tests/errors.rs`, 运行时错误位于
  `crates/ksl-runtime/tests/runtime_errors.rs`; 脚本无法触发的分支由模块单元测试覆盖.
- Rust 测试只覆盖 ksl 源码层面不可见的数据, 如 typed AST 内容, IR 文本与内部状态.
- 模块加载, 链接与编译缓存测试位于 `crates/ksl-compile/tests`;
  运行时只保留执行与运行缓存测试.
- Rust 测试内嵌 ksl 片段时, 不超过 96 列则优先保持一行, 语句间用一个空格分隔.
  片段更长或触发 rustfmt 行宽检查时改用多行块:
  原始字符串开引号后换行, 片段内容不缩进, 每个顶层语句独占一行,
  KSL 嵌套每级 2 个空格, 闭合引号独占一行.
  不按首行内容对齐; 断言 `Span` 或源码偏移的片段保持精确布局.

## 5. 文档与提交

- 代码优先级: 可维护性 > 可读性 > 性能. 非优化阶段不做性能取舍.
- 注释与文档全中文, 遵循 `docs/中文科技文档协作规范.md`.
  注释只写功能与契约, 不写项目变迁, 备选方案与讨论过程.
- Git 小步提交, Conventional Commits 风格.
  阶段完成后可压缩为版块提交, 压缩前先确认.
- 文档直接修改, Git 作为历史备份, 不保留修改前内容.
