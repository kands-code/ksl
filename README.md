# ksl

ksl 是一个以 Wolfram Language 风格前缀调用为语法的个人编程语言项目, 使用 Rust 实现.

## 快速开始

```sh
cargo build -p ksl-cli
cargo run -p ksl-cli -- run examples/01_language_tour.ksl
cargo run -p ksl-cli -- repl
cargo test --workspace
```

## crate

- `ksl-core`: 语言核心, 包括词法, 语法, 类型检查, 模块系统, CPS IR 与线性 IR 抽象机, Actor 运行时.
- `ksl-cli`: 命令行工具, 提供脚本执行, 交互模式与各阶段调试输出.
- `ksl-macros`: 过程宏, 生成错误目录编号.
- `ksl-treesitter`: tree-sitter 语法与编辑器查询.

## 开发

- 检查文档, TOML 与注释标点: `scripts/check-docs.sh`
- 就地格式化: `scripts/check-docs.sh --fix`
- 启用提交前检查: `scripts/install-hooks.sh`

## 文档

- 设计结论: `docs/design.md`
- 术语表: `docs/GLOSSARY.md`
- 示例: `examples/README.md`
- 基准: `bench/README.md`
- 脚本测试: `crates/ksl-core/tests/scripts/README.md`
