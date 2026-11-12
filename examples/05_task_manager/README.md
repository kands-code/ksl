# 任务管理器

综合示例: 数据模型, 文件持久化, 命令解析与 Actor 服务.

## 目录

```text
examples/05_task_manager/
├── data/tasks.txt   种子数据
├── lib/model.ksl    数据模型与纯函数
├── lib/store.ksl    文本文件读写
├── main.ksl         一次性命令入口
├── service.ksl      Actor 服务入口
└── tmp/             运行生成的数据目录, 被 Git 忽略
```

两个入口都读写 `examples/05_task_manager/tmp/tasks.txt`, 路径相对模块文件所在目录,
与当前工作目录无关. 首次运行可以导入种子数据:

```bash
mkdir -p examples/05_task_manager/tmp
cp examples/05_task_manager/data/tasks.txt examples/05_task_manager/tmp/tasks.txt
```

不导入时从空列表开始.

## 一次性命令

```bash
cargo run -p ksl-cli -- run examples/05_task_manager/main.ksl -- list
cargo run -p ksl-cli -- run examples/05_task_manager/main.ksl -- stats
cargo run -p ksl-cli -- run examples/05_task_manager/main.ksl -- add 2 write release notes
cargo run -p ksl-cli -- run examples/05_task_manager/main.ksl -- done 1
cargo run -p ksl-cli -- run examples/05_task_manager/main.ksl -- remove 2
```

`add` 的第一个参数是优先级, 其余参数拼成标题.

## Actor 服务

```bash
cargo run -p ksl-cli -- run examples/05_task_manager/service.ksl
```

服务从标准输入逐行读取命令, 支持 `list`, `stats`, `add`, `done`, `remove` 与 `quit`.
也可以用管道驱动:

```bash
printf 'list\nadd 2 write release notes\nlist\nquit\n' \
  | cargo run -p ksl-cli -- run examples/05_task_manager/service.ksl
```

服务端 Handler 持有任务列表, 客户端通过消息发送请求并在自己的信箱收取回复.
每次变更后立即写回文件, 服务退出不会丢失已提交的修改.

## 存储格式

每行一条任务:

```text
id|status|priority|title
```

`status` 为 `pending` 或 `done`. 标题不允许包含 `|`. 解析失败时报告 `FileError/Other`.
