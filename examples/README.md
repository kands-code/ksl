# 示例

示例按能力递进排列, 从单文件程序到模块化项目.

```bash
cargo run -p ksl-cli -- run examples/01_language_tour.ksl
cargo run -p ksl-cli -- run examples/02_text_report/main.ksl -- examples/02_text_report/data/notes.txt
cargo run -p ksl-cli -- run examples/03_actor_service.ksl
cargo run -p ksl-cli -- run examples/04_terminal.ksl
```

- `01_language_tour.ksl`: 值, 绑定, 函数, Data, 列表, Dict, Set, 多路分支与格式化输出.
- `02_text_report/`: 统计文件的行数, 单词数与字符数, 演示 `@Use`/`@Export`, `Args` 与 `ReadFile`.
- `03_actor_service.ksl`: 请求回复, Join 等待与重启, 用 `@ResultGet` 取 Join 结果.
- `04_terminal.ksl`: 原始模式下用 `ReadKey` 统计按键次数并原地重绘; 非终端输入时只打印提示.
- `05_task_manager/`: 综合项目, 包含数据模型, 持久化, 一次性命令与 Actor 服务两个入口.
  运行方式见 `05_task_manager/README.md`.
- `06_p2p_chat/`: 模块化 TCP 点对点聊天, `lib/chat.ksl` 导出聊天事件, 握手与协调者;
  标准输入与套接字各由一个读 Actor 负责. 运行方式见 `06_p2p_chat/README.md`.

`02_text_report`, `05_task_manager` 与 `06_p2p_chat` 是模块化项目, 目录带 `.kslproj` 项目标记.
`run` 仍以被运行文件所在目录为模块根; `check` 与语言服务器在项目目录启动时以该目录为项目根.

点对点聊天需要两个终端:

```bash
cargo run -p ksl-cli -- run examples/06_p2p_chat/main.ksl -- listen 9000 alice
cargo run -p ksl-cli -- run examples/06_p2p_chat/main.ksl -- connect 9000 bob
```

昵称随命令行给出, 握手时双方各发一行昵称. 输入一行后按 Enter 发送, Ctrl-D 离开.
示例只使用回环地址, 明文传输, 不做加密.
