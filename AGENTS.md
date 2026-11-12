# AGENTS.md

## 项目概述

ksl 是一个个人娱乐性质的编程语言项目, 通用语言定位, 语法风格参照 Wolfram Language, 仅保留前缀调用与 `@` 特殊形式. 设计结论记录在 `docs/design.md`, 待确认事项以 `待确认:` 标记.

## 硬性约束

1. 代码: Rust workspace, 默认最新 Nightly, MSRV 1.99.1, edition 2024.
2. 质量优先级: 可维护性 > 可读性 > 性能. 非优化阶段不做性能取舍.
3. 检查: clippy 严格模式, `clippy.toml` 放宽对测试代码的约束. rustfmt 与 clippy 的问题优先自动修复.
4. 测试: 最小原则, 仅作为排错手段, 不以数量作为指标.
   功能性测试优先使用脚本形式: 在 `crates/ksl-core/tests/scripts/` 下新增 `.ksl` 用例, 用例以行注释给出期望, 指令取 `# @value <显示文本>`, `# @error <错误码>`, `# @exit <退出码>`, `# @check`, `# @run` 五种, 缺省按 `@run`.
   仅当被断言的数据在 ksl 源码层面不可见时才写 Rust 测试, 典型如源码区间 `Span`, 类型表内容, IR 文本形态, 符号驻留表与弱引用状态.
5. 注释: 全中文, 遵循 `docs/中文科技文档协作规范.md`, 写 why 不写 what.
6. Git: 提交默认允许, 采用小步提交, 阶段完成后再压缩为版块提交; 压缩前须向用户说明并取得确认. 提交信息遵循 Conventional Commits 风格. 其余历史改写操作, 如 rebase, reset 等, 未经确认不执行.
7. 子代理: 不使用, 所有工作在当前会话直接完成.
8. 文档与协作文本遵循 `docs/中文科技文档协作规范.md`, 术语以 `GLOSSARY.md` 为准.

## 目录结构

- `docs/中文科技文档协作规范.md`: 协作规范, 修改文档前必读.
- `docs/design.md`: 语言设计结论与待确认事项.
- `docs/GLOSSARY.md`: 术语表.
- `crates/ksl-core`: 语言核心库, 词法, 语法, 类型检查, CPS IR 与线性 IR 抽象机, Actor 调度器与 I/O.
- `crates/ksl-core/tests/scripts`: 脚本形式的功能测试用例, 一个 `.ksl` 文件一个用例, 指令说明见 `crates/ksl-core/tests/scripts/README.md`.
- `crates/ksl-core/tests/scripts.rs`: 脚本用例的驱动程序.
- `crates/ksl-cli`: 命令行二进制, 产物名为 `ksl`.
- `crates/ksl-macros`: 过程宏, 生成错误目录的编号.
- `scripts/check-docs.sh`: Markdown 与 TOML 格式检查及注释标点检查, 加 `--fix` 就地格式化.
- `.githooks/pre-commit`: 提交前调用格式检查; `scripts/install-hooks.sh` 启用.
- `examples`: 可直接用 `ksl run` 运行的示例程序.
- `bench`: 用 hyperfine 测量的基准用例, 运行方式见 `bench/README.md`.

## 工具

- `rg`: `$HOME/.cache/opencode/bin/rg`, 该路径不在 `PATH` 中.
- Cargo registry: `$HOME/.local/sdk/cargo/registry`; 查阅依赖源码时直接在此目录查找.
