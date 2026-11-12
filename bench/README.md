---
updated: 2026-11-13
---

# 基准

用 [hyperfine](https://github.com/sharkdp/hyperfine) 测量实际程序的耗时.
每个用例是一个 `.ksl` 程序, 覆盖一条热路径; 用例末尾打印结果, 便于确认程序
确实跑到了预期的量.

## 运行

```bash
cargo build --release -p ksl-cli
bench/run.sh
```

`run.sh` 逐个用例调用 hyperfine, 默认 3 次预热, 10 次测量. 加 `--json` 则
额外把原始结果导出到 `bench/results/`, 便于前后对比.

长 REPL 会话单独测量:

```bash
bench/repl.sh 3000
```

该脚本生成 3000 条顶层绑定输入并一次喂给 REPL, 覆盖会话克隆, 类型表合并与
全局环境增长. 加 `--json` 导出到 `bench/results/repl.json`.

## 用例

| 文件             | 覆盖                                              | 结果      |
| ---------------- | ------------------------------------------------- | --------- |
| `call.ksl`       | 函数调用; 尾递归与非尾递归的续延帧                | 110000    |
| `list_build.ksl` | 反复 `@ListAppend`; 列表元素的存储宽度            | 50000     |
| `list_scan.ksl`  | `@ListMap` 与 `@ListFold` 的高阶遍历              | 400026666 |
| `string.ksl`     | `StringToChars`, `CharsToString` 与 `EqualString` | 305000    |
| `actor.ksl`      | 信箱队列与调度器                                  | 20000     |
| `timer.ksl`      | `@SetTimer` 延迟投递的共享定时堆                  | 0         |
| `timeout.ksl`    | 并发 `@Receive` 超时与批量唤醒                    | 2000      |

## 注意

- hyperfine 测量的是整个进程, 含启动与退出; 用例耗时在百毫秒量级, 启动占比可忽略.
- 用例规模是固定的, 不随机器变化; 前后对比必须在同一台机器上进行.
- 语料只覆盖几条主路径, 不是完整性能画像; 新增用例时保持一个用例一条路径.

## 参考测量

下列数字取自 Apple M1 Pro, macOS 27.2, release 构建; 换机器后不可直接对比.
记录于此供察觉量级变化, 不作为指标.

| 用例             | 墙钟  | 峰值常驻内存 |
| ---------------- | ----- | ------------ |
| `call.ksl`       | 69 ms | 5.3 MiB      |
| `list_build.ksl` | 36 ms | 5.0 MiB      |
| `list_scan.ksl`  | 39 ms | 5.5 MiB      |
| `string.ksl`     | 15 ms | 8.1 MiB      |
| `actor.ksl`      | 31 ms | 4.7 MiB      |
| `timer.ksl`      | 38 ms | 5.2 MiB      |
| `timeout.ksl`    | 21 ms | 8.9 MiB      |

REPL 长会话 (3000 次输入, `bench/repl.sh 3000`): 约 146 ms, 8.9 MiB.
