# 点对点聊天

纯文本 TCP 聊天示例, 演示模块导出, 跨模块 Handler, 套接字读写与 Actor 分工.

## 目录

```text
examples/06_p2p_chat/
├── lib/chat.ksl    聊天事件, 读写 Actor 与协调者
├── main.ksl        参数解析与连接建立
└── README.md
```

## 运行

进入示例目录, 两个终端分别运行:

```bash
cd examples/06_p2p_chat

# 终端一
cargo run -p ksl-cli -- run main.ksl -- listen 9000

# 终端二
cargo run -p ksl-cli -- run main.ksl -- connect 9000
```

输入一行后按 Enter 发送, Ctrl-D 离开. 本示例只使用 `127.0.0.1`, 明文传输, 不做加密.

## 结构

- `lib/chat.ksl` 声明 `ChatEvent`, 导出 `usage`, `coordinator` 与 `^ChatEvent`.
- `coordinator` 持有套接字, 第一次进入时启动标准输入读 Actor 与套接字读 Actor.
- 标准输入读 Actor 使用 `ReadLinePlain`, 按行读取, 不使用内置行编辑器; 避免对端消息到达时行重绘产生多余缩进. 终端自身的行编辑仍会在对端消息插入当前输入行时产生视觉混排, 但输入内容不会被改动, 按 Enter 提交后继续.
- 套接字读 Actor 用 `SocketReadLine` 按行读取; 空闲超时只表示暂时没有消息, 继续等待.
- 协调者在信箱中接收 `ChatEvent`, 负责 `SocketWriteLine` 与 `Print`.

`@Main` 只负责建立监听或连接, spawn 协调者并 `@Join` 等待; 因为 `@Main` 不能使用 `@Continue`, 消息循环放在 Handler 中.
