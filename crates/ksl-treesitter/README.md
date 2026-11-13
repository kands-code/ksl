---
updated: 2026-11-12
---

# ksl-treesitter

ksl 的 tree-sitter 语法, 供编辑器做高亮与结构化查询. 语义以 `ksl-core` 为准,
两侧记号流由漂移测试校验.

## 目录

```
crates/ksl-treesitter/
├── grammar.js            # 语法描述
├── src/scanner.c         # 外部扫描器
├── src/parser.c          # 自动生成, 勿手工修改
├── queries/              # Helix 查询
├── scripts/              # 生成与安装脚本
├── test/                 # tree-sitter 用例与漂移语料
├── tests/                # 漂移与名单一致性测试
├── .helix/               # Helix 工作区配置与 MT 语法文件
└── tree-sitter.json
```

## 开发

解析器已入库. 改动 `grammar.js` 或 `src/scanner.c` 后重新生成 (需 tree-sitter CLI):

```bash
cd crates/ksl-treesitter
tree-sitter generate
```

测试:

```bash
cargo test -p ksl-treesitter
```

`test/drift` 是本地漂移语料, 新增语法时同步更新. `test/corpus` 需用
`tree-sitter test` 运行.

## 脚本

在 `crates/ksl-treesitter` 下运行:

```bash
scripts/gen-builtins.sh   # 生成 Helix 内置函数名单
scripts/gen-mtsx.sh       # 生成 MT 语法名单
scripts/install-helix.sh  # 编译并安装 Helix 运行时文件
```

## 编辑器

Helix: `.helix/languages.toml` 登记语言; 运行 `scripts/install-helix.sh` 安装
运行时文件, 然后从 crate 目录启动:

```bash
cd crates/ksl-treesitter && hx test/drift/tour.ksl
```

MT: 在 MT 管理器中打开 `.helix/ksl.mtsx` 安装.

卸载 Helix:

```bash
rm "$HOME/.config/helix/runtime/grammars/ksl.so"
rm -r "$HOME/.config/helix/runtime/queries/ksl"
```
