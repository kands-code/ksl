---
updated: 2026-11-21
---

# 基准

用 [hyperfine](https://github.com/sharkdp/hyperfine) 测量实际程序的耗时.
每个用例是一个 `.ksl` 程序, 覆盖一条热路径; 文件头用 `# @result <值>` 给出期望输出,
`run.sh` 在测量前先运行一次并校验, 确认程序确实跑到了预期的量.

## 运行

```bash
cargo build --release -p ksl-cli
bench/run.sh
```

`run.sh` 按固定顺序逐个用例调用 hyperfine, 默认 3 次预热, 10 次测量.
用例顺序由脚本中的 `cases` 数组决定, 即报告顺序; 新增用例需要登记数组并写好 `# @result`.
加 `--json` 则额外把原始结果导出到 `bench/results/`, 便于保存原始数据.

## 用例

| 文件             | 覆盖                                                  | 结果      |
| ---------------- | ----------------------------------------------------- | --------- |
| `call.ksl`       | 函数调用; 尾递归与非尾递归的续延帧                    | 110000    |
| `list_build.ksl` | 反复 `@ListAppend`; 元素存储与持久化列表              | 50000     |
| `list_scan.ksl`  | `@ListMap`, `@ListFold` 与 `@ListFilter`              | 400026666 |
| `dict.ksl`       | `@DictPut` 与 `@DictGet` 的持久化映射                 | 200010000 |
| `match.ksl`      | Data 构造与 `@Match` 变体匹配                         | 600020000 |
| `string.ksl`     | `StringJoin`, `StringToChars`, `CharsToString` 与比较 | 1220000   |
| `actor.ksl`      | 信箱队列与调度器                                      | 20000     |
| `timer.ksl`      | `@SetTimer` 延迟投递的共享定时堆                      | 0         |
| `timeout.ksl`    | 并发 `@Receive` 超时与批量唤醒                        | 2000      |

## 测量约定

- hyperfine 测量的是整个进程, 含启动与退出; 用例耗时在十几到几十毫秒量级, 启动占比可忽略.
- 用例规模是固定的, 不随机器变化; 前后对比必须在同一台机器上进行.
- 语料只覆盖几条主路径, 不是完整性能画像; 新增用例时保持一个用例一条路径.

## 当前测量

下列数字取自 Apple M1 Pro, macOS 27.2, release 构建; 构建采用 fat LTO 与单一 codegen unit. 换机器后需重新测量.
数字只表示量级, 不作为跨机器指标.

| 用例             | 实际耗时 | 峰值常驻内存 |
| ---------------- | -------- | ------------ |
| `call.ksl`       | 23 ms    | 5.8 MiB      |
| `list_build.ksl` | 14 ms    | 5.3 MiB      |
| `list_scan.ksl`  | 14 ms    | 5.5 MiB      |
| `dict.ksl`       | 17 ms    | 6.4 MiB      |
| `match.ksl`      | 22 ms    | 7.3 MiB      |
| `string.ksl`     | 19 ms    | 15.6 MiB     |
| `actor.ksl`      | 16 ms    | 5.7 MiB      |
| `timer.ksl`      | 27 ms    | 6.0 MiB      |
| `timeout.ksl`    | 19 ms    | 9.3 MiB      |
