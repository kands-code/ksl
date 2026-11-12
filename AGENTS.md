# AGENTS.md

ksl 是一个个人编程语言项目, 语法采用 Wolfram Language 风格的前缀调用, 使用 Rust 实现.
项目定位与文档索引见 `docs/about.md`, 当前设计见 `docs/topics/`, 未实现方向与待确认事项见 `docs/roadmap.md`.

## 硬性约束

1. 代码: Rust workspace, 默认最新 Nightly, 最低支持 Rust 版本 1.100.0, edition 2024.
2. 质量优先级: 可维护性 > 可读性 > 性能. 非优化阶段不做性能取舍.
3. 检查: `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `scripts/check-docs.sh`.
   rustfmt 与 clippy 的问题优先自动修复. 工程约定见 `docs/topics/implementation/engineering.md`.
4. 测试遵循最小原则, 仅作为排错手段, 不以数量作为指标.
   功能性测试优先写成 `crates/ksl-runtime/tests/scripts/` 下的 `.ksl` 用例;
   仅当被断言的数据在 ksl 源码层面不可见时才写 Rust 测试.
   错误路径由 Rust 测试断言 `ErrorKind`, 不比较错误编号.
   用例写法见 `crates/ksl-runtime/tests/scripts/README.md`.
5. 注释与文档全中文, 遵循 `docs/中文科技文档协作规范.md`, 术语以 `docs/glossary.md` 为准.
6. Git 采用小步提交, 提交信息遵循 Conventional Commits; 提交默认允许,
   阶段完成后再压缩为版块提交, 压缩前须说明并取得确认.
   其余历史改写操作, 如 rebase, reset 等, 未经确认不执行.
7. 不使用子代理.

## 目录

crate 组成见 `README.md`, 文档结构见 `docs/about.md`. 常用路径:

- `crates/ksl-runtime/tests/scripts`: 脚本形式的功能测试用例.
- `examples`: 可直接用 `ksl run` 运行的示例程序.
- `bench`: hyperfine 基准用例, 说明见 `bench/README.md`.
- `scripts`: 格式检查与示例检查, 提交与推送钩子由 `scripts/install-hooks.sh` 启用.

## 工具

- `rg`: `$HOME/.cache/opencode/bin/rg`, 该路径不在 `PATH` 中.
- Cargo registry: `$HOME/.local/sdk/cargo/registry`; 查阅依赖源码时直接在此目录查找.
