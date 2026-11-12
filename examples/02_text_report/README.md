# 文本报表

读取一个文本文件, 通过本地模块统计行数, 单词数与字符数.

## 运行

从仓库根目录执行:

```bash
cargo run -p ksl-cli -- run examples/02_text_report/main.ksl -- examples/02_text_report/data/notes.txt
```

输出:

```text
examples/02_text_report/data/notes.txt: 4 lines, 5 words, 32 chars
```

不带路径时打印用法. 文件不存在或没有读取权限时只打印简短错误, 不中断示例.
