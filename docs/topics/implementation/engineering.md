---
updated: 2026-11-21
---

# ksl 工程约束

ksl 项目的工程约定, 面向贡献者.
语言语义见 `docs/topics/language/` 下的各篇, 实现架构见 `docs/topics/implementation/architecture.md`.

## 1. 工程结构

- Rust workspace 实现, 项目名 ksl, crate 命名使用连字符;
  拆分为 `ksl-base`, `ksl-catalog`, `ksl-frontend`, `ksl-runtime`, `ksl-plugin`, `ksl-cli`,
  `ksl-macros` 与 `ksl-treesitter`.
- 默认工具链为最新 Nightly, 最低支持 Rust 版本 1.100.0, edition 2024.
- 许可证采用 AGPL-3.0-or-later.

## 2. 依赖

- 依赖使用最新稳定版本, 允许使用第三方库.
  采用: annotate-snippets 用于错误报告, imbl 用于持久化结构, arcstr 用于运行时字符串,
  crossbeam-channel 用于调度器收件箱, 释放队列唤醒与工作线程池,
  shared_child 用于子进程的并发等待与终止,
  mio 用于网络事件轮询, socket2 用于监听器创建, libloading 用于加载动态库,
  regex-automata 与 regex-syntax 用于正则,
  ksl-plugin 用于插件 ABI 与导出宏,
  unicode-ident 用于 XID 判定, parking_lot 用于同步, indexmap 用于稳定顺序,
  ksl-macros 用于错误编号, lsp-server 与 serde_json 用于 LSP,
  gen-lsp-types 用于协议类型并启用 url 特性, jiff 用于时间与日期,
  crossterm 用于终端按键与 REPL 行编辑,
  unicode-segmentation 与 unicode-width 用于字素切分与显示宽度,
  proc-macro2, quote 与 syn 用于过程宏, tree-sitter 与 tree-sitter-language 用于编辑器语法,
  cc 用于构建脚本;
  内部类型变量统一采用 ena;
  CLI 参数解析自行实现, 不依赖第三方库;
  CLI 默认使用 mimalloc v2 稳定分支作为全局分配器, 可用 `--no-default-features` 回退系统分配器;
  CLI 端到端测试直接使用标准库 `std::process::Command`, 公共辅助代码位于 `crates/ksl-cli/tests/common.rs`,
  测试临时目录位于 Cargo 的 `target/tmp`.
- 有序映射使用 indexmap:
  Data 收集阶段按声明顺序处理, 使首个错误选择可复现.
  需要结构共享的会话状态使用 imbl 持久化映射: Data 类型表, 类型表, 检查环境作用域与模块缓存外层;
  Data 类型表只按名字查找, 不保存声明顺序; 变体顺序由 `DataInfo` 内的 `IndexMap` 保持.
  其余只做查找或成员判断的表用 HashMap.
  编译器保持 fail-fast.

## 3. 质量约定

- clippy 严格检查, clippy.toml 放宽对测试代码的约束, 问题优先自动修复.
  例外限于以下情形, 新增例外须说明理由:
  - `too_many_lines`: 集中分派或线性流程, 拆分后反而割裂规则.
  - `unnecessary_wraps`: 内置函数构造器统一返回 `Result`, 由注册表分派.
  - `cast_precision_loss` 与 `cast_possible_truncation`: 数值转换的必要放宽, 范围已显式校验.
  - `float_cmp`: 相等比较按 IEEE 语义, NaN 不等于自身, 不能改用近似比较.
  - `needless_lifetimes`: 插件 ABI 的返回切片生命周期由调用方决定.
  - `deprecated`: LSP `DocumentSymbol.deprecated` 已被协议废弃, 但仍须填充.
  - `too_many_arguments`: 参数与 IR 字段, 声明字段或输入线程状态一一对应的分派与降级辅助函数.
- release 构建启用 fat LTO 与单一代码生成单元, 以构建时间换运行时性能.
- rustfmt 启用 Nightly 不稳定选项.
- 导入按标准库, 外部 crate, crate 内路径分组, 组间空一行;
  crate 内路径分组内 `use super::*` 居首.
- 子模块使用父文件 `foo.rs` 加子目录 `foo/`, 不使用 `mod.rs`.
- 手写 C 源文件与插件头文件由 `scripts/check-c.sh` 调用 clang-format 检查, 配置见 `.clang-format`;
  生成的 `crates/ksl-treesitter/src/parser.c` 与 `src/tree_sitter` 不参与格式化.
- 测试遵循最小原则, 仅作为排错手段, 不以数量作为指标.
  语言行为写在 `crates/ksl-runtime/tests/scripts/run` 下, 每个 `.ksl` 文件是一个执行用例;
  错误路径由 `crates/ksl-frontend/tests/errors.rs` 与 `crates/ksl-runtime/tests/runtime_errors.rs`
  断言 `ErrorKind`, 不比较错误编号.
  Rust 测试只覆盖 ksl 源码层面不可见的数据, 如类型表内容, IR 文本与内部状态.
  Rust 测试内嵌 ksl 片段时, 片段本身不超过 96 列则优先保持一行, 语句间用一个空格分隔;
  片段更长或嵌入后触发 rustfmt 行宽检查时改用多行块.
  多行使用原始字符串, 开引号后换行, 片段内容不缩进, 每个顶层语句独占一行,
  KSL 嵌套每级 2 个空格, 闭合引号独占一行.
  不按首行内容对齐; 断言 `Span` 或源码偏移的片段保持精确布局.
- 代码优先级: 可维护性 > 可读性 > 性能, 非优化阶段不做性能取舍.
- 注释全中文, 遵循 `docs/中文科技文档协作规范.md`;
  只写功能与契约, 不写项目变迁, 备选方案与讨论过程.
- Git 小步提交, 阶段完成后再压缩为版块提交, Conventional Commits 风格.
- 文档允许直接修改, Git 作为历史备份, 非必要不保留修改前内容.
