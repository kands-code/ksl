# 点对点聊天

纯文本 TCP 聊天示例, 演示模块导出, 跨模块 Handler, 套接字读写与 Actor 分工.

## 目录

```text
examples/06_p2p_chat/
├── lib/chat.ksl    聊天事件, 握手, 读写 Actor 与协调者
├── main.ksl        参数解析, 连接建立与握手
└── README.md
```

## 运行

从仓库根目录打开两个终端:

```bash
# 终端一
cargo run -p ksl-cli -- run examples/06_p2p_chat/main.ksl -- listen 9000 alice

# 终端二
cargo run -p ksl-cli -- run examples/06_p2p_chat/main.ksl -- connect 9000 bob
```

昵称随命令行给出. 输入一行后按 Enter 发送, Ctrl-D 离开.
示例只使用 `127.0.0.1`, 明文传输, 不做加密.

## 显示

- 本端输入以昵称作提示符, 例如 `alice> `; 输入内容回显在提示符之后.
- 对端消息带发送方昵称, 例如 `[bob] hello`.

## 结构

- `lib/chat.ksl` 声明 `ChatEvent`, 导出 `usage`, `handshake`, `coordinator` 与 `^ChatEvent`.
- `main.ksl` 解析命令, 端口与昵称, 建立连接后完成握手, 再启动协调者.
- 握手: 双方各发一行昵称, 收到对端昵称后才开始聊天.
- 协调者持有套接字, 第一次进入时启动标准输入读 Actor 与套接字读 Actor.
- 标准输入读 Actor 使用 `ReadLine`; 对端消息到达时插入提示符上方, 当前输入行保留并重绘.
- 套接字读 Actor 用 `SocketReadLine` 按行读取; 空闲超时只表示暂时没有消息, 继续等待.
- 协调者在信箱中接收 `ChatEvent`, 负责 `SocketWriteLine` 与 `Print`.
- `@Main` 不能使用 `@Continue`, 消息循环放在 Handler 中.
