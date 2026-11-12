# ksl

ksl 是一个个人编程语言项目, 语法采用 Wolfram Language 风格的前缀调用, 使用 Rust 实现.

## 快速开始

```sh
cargo build -p ksl-cli
cargo run -p ksl-cli -- run examples/01_language_tour.ksl
cargo run -p ksl-cli -- repl
cargo test --workspace
```

## crate 组成

- `ksl-base`: 符号驻留, 源码区间, 类型表示与格式化规则.
- `ksl-catalog`: 内置函数元数据, 特殊形式, 操作, 类型与保留名.
- `ksl-frontend`: 词法, 语法, AST, 类型检查, 重写与诊断.
- `ksl-runtime`: 值表示, CPS IR 与线性 IR 抽象机, Actor 运行时, I/O 与模块加载.
- `ksl-plugin`: 插件 ABI, 值辅助与导出宏.
- `ksl-cli`: 命令行工具, 提供脚本执行, 交互模式与各阶段调试输出.
- `ksl-macros`: 过程宏, 生成错误目录编号.
- `ksl-treesitter`: tree-sitter 语法与编辑器查询.

## 开发

- 格式化 Rust: `cargo fmt --all`
- 检查 Rust: `cargo clippy --workspace --all-targets`
- 检查 Markdown, TOML 与注释标点: `scripts/check-docs.sh`
- 就地格式化文档: `scripts/check-docs.sh --fix`
- 检查示例与基准: `scripts/check-programs.sh`
- 安装提交与推送钩子: `scripts/install-hooks.sh`

## 文档

文档入口见 `docs/about.md`, 包括教程, 设计, 路线图, 术语表与协作规范.
