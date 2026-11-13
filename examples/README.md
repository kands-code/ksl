# 示例

示例按能力递进排列. 前四个分别展示语言基础, 模块与文件, Actor 与终端按键; 第五个是综合任务管理器, 第六个是模块化的 TCP 点对点聊天.

```bash
cargo run -p ksl-cli -- run examples/01_language_tour.ksl
cargo run -p ksl-cli -- run examples/02_text_report/main.ksl -- examples/02_text_report/data/notes.txt
cargo run -p ksl-cli -- run examples/03_actor_service.ksl
cargo run -p ksl-cli -- run examples/04_terminal.ksl
```

- `01_language_tour.ksl`: 值, 绑定, 函数, Data, 列表, Dict, Set 与格式化输出.
- `02_text_report/`: 本地模块统计文件的行数, 单词数与字符数, 演示 `@Use`/`@Export`, `Args` 与 `ReadFile`.
- `03_actor_service.ksl`: 命名注册, 请求回复, 监视与重启, 用 `@ResultGet` 取 Join 结果.
- `04_terminal.ksl`: 用 `ReadKey` 读取方向键与确认键; 非终端输入时只打印提示.
- `05_task_manager/`: 综合项目, 包含数据模型, 持久化, 一次性命令与 Actor 服务两个入口; 模型层用 `@OptionMap` 与 `@OptionAndThen` 组合可选解析. 运行方式见 `05_task_manager/README.md`.
- `06_p2p_chat/`: 模块化点对点聊天, `lib/chat.ksl` 导出聊天事件, 协调者与用法文本; 标准输入读 Actor 与套接字读 Actor 分工, 按行收发. 运行方式见 `06_p2p_chat/README.md`.

```bash
cd examples/06_p2p_chat

# 终端一
cargo run -p ksl-cli -- run main.ksl -- listen 9000

# 终端二
cargo run -p ksl-cli -- run main.ksl -- connect 9000
```

输入一行后按 Enter 发送, Ctrl-D 离开. 示例只使用回环地址, 明文传输, 不做加密.
