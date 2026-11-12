# ksl

ksl 是一个个人编程语言项目, 语法采用 Wolfram Language 风格的前缀调用, 使用 Rust 实现.
语言规则与实现见 `docs/topics/`, 文档入口见 `docs/about.md`.

## 快速开始

```sh
cargo build -p ksl-cli
cargo run -p ksl-cli -- run examples/01_language_tour.ksl
cargo run -p ksl-cli -- repl
```

`cargo test` 覆盖语言实现; `cargo test --workspace` 会构建插件.
raylib 插件需要 `plugins/raylib/vendor/` 下的 raylib 发布包.

## crate 组成

- `ksl-base`: 名字, 源码区间, 类型表示, 失败类别与格式化规则.
- `ksl-catalog`: 内置函数元数据, 内置 Data 类型表, 特殊形式, 操作, 类型与保留名.
- `ksl-macros`: 过程宏, 生成错误目录编号与插件导出.
- `ksl-diagnostics`: 错误目录, 错误编号与诊断渲染.
- `ksl-plan`: 执行计划数据, 线性指令与执行视图.
- `ksl-typed`: 带类型的 AST 接口与 `CheckedProgram`.
- `ksl-frontend`: 词法, 语法, AST 与类型检查.
- `ksl-compile`: 模块加载, 链接, 检查缓存与模块接口.
- `ksl-ir`: CPS 树 IR, 线性 IR, 降级与线性化.
- `ksl-value`: 值表示, 宿主资源接口, 闭包与原生插件 ABI.
- `ksl-io`: 文件, 子进程, 网络, 终端, 时间与正则.
- `ksl-runtime`: 执行计划上的抽象机, Actor 运行时与内置函数.
- `ksl-host`: 宿主状态, 模块执行, 插件加载与交互会话.
- `ksl-cli`: 命令行工具, 提供脚本执行, 交互模式与各阶段调试输出.
- `ksl-treesitter`: tree-sitter 语法与编辑器查询.

插件:

- `plugins/raylib`: raylib 包装插件与示例, 构建方式见 `plugins/raylib/README.md`.
- `plugins/demo`: 原生插件测试插件, 由 `ksl-runtime` 测试驱动, 说明见 `plugins/demo/README.md`.

## 示例

`examples` 中的程序可直接用 `ksl run` 执行, 列表与运行方式见 `examples/README.md`.

## 开发

- 格式化 Rust: `cargo fmt --all`
- 检查 Rust: `cargo clippy --workspace --all-targets`
- 检查 Markdown, TOML 与注释标点: `scripts/check-docs.sh`
- 就地格式化文档: `scripts/check-docs.sh --fix`
- 检查示例与基准: `scripts/check-programs.sh`
- 安装提交与推送钩子: `scripts/install-hooks.sh`
