# 示例

示例按能力递进排列. 前四个分别展示语言基础, 模块与文件, Actor 与终端按键; 最后一个是综合任务管理器.

```bash
cargo run -p ksl-cli -- run examples/01_language_tour.ksl
cargo run -p ksl-cli -- run examples/02_text_report/main.ksl -- examples/02_text_report/data/notes.txt
cargo run -p ksl-cli -- run examples/03_actor_service.ksl
cargo run -p ksl-cli -- run examples/04_terminal.ksl
```

- `01_language_tour.ksl`: 值, 绑定, 函数, Data, 列表, Dict, Set 与格式化输出.
- `02_text_report/`: 本地模块统计文件的行数, 单词数与字符数, 演示 `@Use`/`@Export`, `Args` 与 `ReadFile`.
- `03_actor_service.ksl`: 命名注册, 请求回复, 监视与重启.
- `04_terminal.ksl`: 用 `ReadKey` 读取方向键与确认键; 非终端输入时只打印提示.
- `05_task_manager/`: 综合项目, 包含数据模型, 持久化, 一次性命令与 Actor 服务两个入口. 运行方式见 `05_task_manager/README.md`.
