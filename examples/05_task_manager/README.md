# 任务管理器

综合示例, 包含数据模型, 文件持久化, 命令解析与 Actor 服务.

## 目录

```text
examples/05_task_manager/
├── data/tasks.txt   种子数据
├── lib/model.ksl    数据模型与纯函数
├── lib/store.ksl    文本文件读写
├── main.ksl         一次性命令入口
└── service.ksl      Actor 服务入口
```

两个入口都从当前工作目录读写 `tasks.txt`. 运行前进入示例目录并复制种子数据:

```bash
cd examples/05_task_manager
cp data/tasks.txt tasks.txt
```

## 一次性命令

```bash
cargo run -p ksl-cli -- run main.ksl list
cargo run -p ksl-cli -- run main.ksl stats
cargo run -p ksl-cli -- run main.ksl add 2 write release notes
cargo run -p ksl-cli -- run main.ksl done 1
cargo run -p ksl-cli -- run main.ksl remove 2
```

`add` 的第一个参数是优先级, 其余参数拼成标题. 文件不存在时按空列表处理, 首次 `add` 会创建文件.

## Actor 服务

```bash
cargo run -p ksl-cli -- run service.ksl
```

服务启动后从标准输入逐行读取命令, 支持 `list`, `stats`, `add`, `done`, `remove` 与 `quit`. 也可以用管道驱动:

```bash
printf 'list\nadd 2 write release notes\nlist\nquit\n' | cargo run -p ksl-cli -- run service.ksl
```

服务端 Handler 持有任务列表, 客户端通过消息发送请求并在自己的信箱收取回复. 每次变更后立即写回 `tasks.txt`, 因此服务退出不会丢失已提交的修改.

## 存储格式

每行一条任务:

```text
id|status|priority|title
```

`status` 为 `pending` 或 `done`. 标题不允许包含 `|`. 解析失败时报告 `IOError/Other`.
