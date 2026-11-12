---
updated: 2026-11-26
---

# ksl 网络

ksl 的 TCP 原语.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
网络 I/O 的实现见 `docs/topics/implementation/scheduler.md` 第 7 节.

## 1. 句柄与范围

TCP 以不透明句柄 `TcpListener` 与 `Socket` 表示, 二者都不可打印.
`Socket` 是已连接的字节流.

只支持明文 TCP, IPv4 与 IPv6 字面量地址; 监听器只接受显式 IP 字面量.
UDP, TLS, 主机名解析, `@Select` 与透明跨进程 Pid 不在范围内, 见 `docs/roadmap.md`.
网络操作在完成前保持目标句柄存活.

内置只提供字节流与按行读写的原语.
长度前缀分帧与聊天协议以 `.ksl` 模块或示例实现,
不引入独立标准库与模块搜索路径.

## 2. 监听与连接

- `TcpListen[address : string, port : u64] : Result[TcpListener, SocketError]`
  - 监听指定地址与端口; port 为 0 时由宿主分配临时端口,
    实际端口由 `TcpLocalPort` 给出.
- `TcpAccept[listener : TcpListener, timeout : u64] : Result[Socket, SocketError]`
  - 接受一个连接; timeout 为超时毫秒.
- `TcpConnect[address : string, port : u64, timeout : u64] : Result[Socket, SocketError]`
  - 连接指定地址与端口; timeout 为超时毫秒.
- `TcpCloseListener[listener : TcpListener] : nil`
  - 关闭监听器.
- `TcpLocalPort[listener : TcpListener] : u64`
  - 返回监听器实际端口.

接受与连接的流默认开启 TCP_NODELAY.
监听器在非 Windows 平台开启地址复用; Windows 不开启.

## 3. 套接字信息

- `SocketPeer[socket : Socket] : string`
  - 返回对端地址.
- `SocketIsClosed[socket : Socket] : bool`
  - 句柄是否已关闭; 包括 `SocketClose` 与句柄释放触发的关闭.

## 4. 读

- `SocketRead[socket : Socket, max : u64, timeout : u64] : Result[Option[List[u8]], SocketError]`
  - 读最多 max 字节; None 表示对端正常关闭; max 必须大于 0.
  - 单次最多读取 64 KiB 新数据; 需要更多时循环调用.
- `SocketReadExact[socket : Socket, count : u64, timeout : u64] : Result[List[u8], SocketError]`
  - 读满 count 字节; 不足返回 `UnexpectedEof`, count 可以为 0.
- `SocketReadAll[socket : Socket, max : u64, timeout : u64] : Result[List[u8], SocketError]`
  - 读到 EOF; 超过 max 返回 `TooLarge`, max 必须大于 0.
  - 实际分配由 max 与对端发送量共同决定, 调用方负责选择合理的 max.
- `SocketReadLine[socket : Socket, max : u64, timeout : u64] : Result[Option[string], SocketError]`
  - 行长度上限由 max 控制; 持续发送无换行数据时按块扫描, 长行会放大扫描开销.
  - 读 UTF-8 行, 去掉 `\r\n`; 对端关闭返回 None, 超长返回 `LineTooLong`,
    非法 UTF-8 返回 `InvalidUtf8`, max 必须大于 0.

同一套接字上的读操作共享内部读缓冲.
超时后未完成的行, 精确读或全量读保留在缓冲中, 下一次继续.

## 5. 写

- `SocketWrite[socket : Socket, bytes : List[u8], timeout : u64] : Result[u64, SocketError]`
  - 写入字节; 可能只写入一部分并返回实际字节数.
- `SocketWriteAll[socket : Socket, bytes : List[u8], timeout : u64] : Result[nil, SocketError]`
  - 写完整段数据; 失败时套接字可能已部分写入, 调用方应关闭连接.
- `SocketWriteLine[socket : Socket, text : string, timeout : u64] : Result[nil, SocketError]`
  - 写 UTF-8 文本加换行.

同一套接字上的写操作按提交顺序串行.

## 6. 关闭

- `SocketShutdownWrite[socket : Socket, timeout : u64] : Result[nil, SocketError]`
  - 关闭写侧; 对端读到 EOF, 读侧保持打开.
    用于发完请求后等待回复的协议形态.
  - 排在前序写操作之后; timeout 是包含排队时间的总时限.
- `SocketClose[socket : Socket] : nil`
  - 关闭套接字.

## 7. 超时

超时参数为 u64 毫秒, 0 表示立即尝试一次; 需要排队或等待时返回 `TimedOut`.
默认超时是从提交开始的总时限, 包含排队时间.
`SocketReadAll`, `SocketReadExact` 与 `SocketReadLine` 例外, 按每次等待计算:
排队不计时, 每次有进展后重新计时, 单次操作的总耗时不受固定上限约束.
`TcpConnect` 的 timeout 必须大于 0, 0 返回 `InvalidArgument`.
timeout 超出平台计时范围时报运行时错误 `TimeoutOutOfRange`.

以下操作不提供超时参数:
`TcpListen`, `TcpCloseListener`, `TcpLocalPort`, `SocketPeer` 与 `SocketClose`.

## 8. 套接字错误

可失败的套接字操作返回 `Result[T, SocketError]`.
`SocketError` 与 `FileError`, `InputError` 分开: 前者覆盖网络连接, 地址, 超时, 行协议与关闭,
后两者用于文件与标准输入.
变体为 `TimedOut`, `InvalidArgument[string]`, `ConnectionRefused`,
`ConnectionReset`, `ConnectionClosed`, `Closed`, `AddrInUse`, `AddrNotAvailable`,
`PermissionDenied`, `InvalidUtf8`, `LineTooLong`, `TooLarge`, `UnexpectedEof` 与 `Other[string]`.
`Closed` 表示本地句柄已关闭, `ConnectionClosed` 表示对端在写操作中关闭.

## 9. 与 Actor 的关系

每个 Actor 同时只能挂起一个 I/O 操作, 因此同一 Actor 不能同时等待套接字读取与信箱接收.
惯用法是读 Actor 与写 Actor 分离, 由协调者 Actor 接收事件;
套接字句柄可以在 Actor 之间传递, 读写可同时挂起.

`@Main` 不能使用 `@Continue`, 而嵌套 `@Def` 不继承 Actor 上下文.
需要循环处理连接或消息时, 使用 `@Handler` 并在体内 `@Continue`, 由 `@Main` 启动后 `@Join`.
