---
updated: 2026-11-25
---

# ksl 教程

依次介绍表达式, 绑定, 函数, Data, 集合, Option 与 Result, 模块, Actor 与输入输出.
每章的完整规则见 `docs/topics/` 下的对应文档.

先构建并运行:

```bash
cargo build -p ksl-cli
cargo run -p ksl-cli -- run examples/01_language_tour.ksl
```

## 1. 值与表达式

ksl 只有一种调用形式: 前缀调用加方括号. 不提供中缀运算符, 也没有优先级.

```ksl
@Let[sum, AddI64[1, 2]];
Print[f"1 + 2 = {sum}\n"];

@Let[product, MulF64[3.5, 2.0]];
Print[f"3.5 * 2 = {product}\n"];
```

- 所有计算都显式调用函数; 每种数值类型各有一组函数, 不做隐式转换.
- `f"..."` 是格式化字符串; 花括号里只能放变量名或字面值, 不能放表达式, 因此需要先绑定再插值.
- `Print` 原样写出到标准输出, 不追加换行, 返回 nil.
- 完整规则见 `docs/topics/language/syntax.md` 与 `docs/topics/library/builtins-arithmetic.md`.

## 2. 绑定与作用域

`@Let` 建立绑定. 绑定不可变, 同名的新绑定遮蔽旧绑定.

```ksl
@Let[x, 10];
@Let[x, AddI64[x, 5]];
Print[f"x = {x}\n"];
```

- 绑定的标注可省略, 类型由检查器推断.
- 非 nil 的语句必须显式绑定, 不想保留结果时写 `@Let[_, 值]`.
- 作用域块用 `@Block[语句, ...]`, 块内定义对外层不可见, 块的值是最后一条语句的值.
- 完整规则见 `docs/topics/language/evaluation.md`.

## 3. 函数

匿名函数写 `@Fn`, 具名函数写 `@Def`. 具名函数可以自递归.

```ksl
@Def[factorial, [n : i64] : i64,
  @If[LessOrEqualI64[n, 1], 1, MulI64[n, factorial[SubI64[n, 1]]]]];

@Let[answer, factorial[5]];
Print[f"5! = {answer}\n"];

@Let[adder, @Fn[[n : i64] : i64, AddI64[n, 1]]];
@Let[value, adder[41]];
Print[f"adder[41] = {value}\n"];
```

- 参数类型与返回类型必须标注.
- 互递归用 `@Fix` 把若干 `@Def` 组成一组, 组内名字互相可见.
- 闭包按值捕获自由变量, 捕获的是定义时刻的值.
- 完整规则见 `docs/topics/language/functions.md`.

## 4. Data 与模式匹配

`@Data` 定义联合类型, `@Match` 按变体分派.

```ksl
@Data[Shape, {Circle[f64], Rect[f64, f64]}];

@Def[area, [s : Shape] : f64,
  @Match[s, {
    (/Circle[r], MulF64[3.14159, MulF64[r, r]]),
    (/Rect[w, h], MulF64[w, h])
  }]];

@Let[result, area[Shape/Rect[3.0, 4.0]]];
Print[f"area = {result}\n"];
```

- 构造器要写类型限定名, 例如 `Shape/Rect`; 模式里可以省成 `/Rect`.
- 匹配必须穷尽, 漏写变体在类型检查阶段报错.
- Data 不支持原地修改; 变更通过 `@Match` 解构后重建.
- 完整规则见 `docs/topics/language/data.md`.

## 5. 集合

列表字面量用花括号, 字典用 `@Dict`, 集合用 `@Set`. 列表上的高阶操作都是特殊形式.

```ksl
@Let[xs, {1, 2, 3, 4, 5}];
@Let[squares, @ListMap[xs, @Fn[[x : i64] : i64, MulI64[x, x]]]];
Print[f"squares = {squares}\n"];

@Let[ages, @Dict[("alice", 30), ("bob", 25)]];
@Let[alice, @DictGet[ages, "alice", 0]];
Print[f"alice = {alice}\n"];
```

- 列表元素类型必须一致, 异构列表在类型检查时报错.
- 字典键限基础标量, 遍历按键排序, 结果稳定.
- 列表操作见 `docs/topics/library/builtins-list.md`, 字典与集合见 `docs/topics/library/builtins-dict-set.md`.
- Option 与 Result 的便捷操作见 `docs/topics/library/builtins-option-result.md`.

## 6. Option 与 Result

可能失败的操作返回 `Result`, 可能缺失的值返回 `Option`. 两者通常用 `@Match` 解构.

```ksl
@Let[port, ParseI64["8080"]];
@Match[port, {
  (Result/Ok[n], Print[f"port = {n}\n"]),
  (Result/Err[_], Print["端口不是整数\n"])
}];
```

- `Option/Some` 与 `Option/None`, `Result/Ok` 与 `Result/Err` 是构造器, 也是模式.
- 没有异常机制, 内置错误不可捕获, 失败一律用值表达.
- `@OptionGet` 与 `@ResultGet` 提供带默认值的取法, 但 `ResultGet` 会丢弃错误负载.
- `@ResultOrElse` 以函数取替代值, 替代函数能读到错误内容.
- 完整规则见 `docs/topics/language/errors.md` 与 `docs/topics/library/builtins-option-result.md`.

## 7. 模块

文件即模块. `@Export` 声明导出项, `@Use` 导入. 模块路径相对当前模块文件所在目录.

目录结构:

```text
main.ksl
lib/math.ksl
```

`lib/math.ksl`:

```ksl
@Export[square];

@Def[square, [n : i64] : i64, MulI64[n, n]];
```

`main.ksl`:

```ksl
@Use[{square}, "lib/math"];

@Let[value, square[9]];
Print[f"9 的平方 = {value}\n"];
```

运行 `cargo run -p ksl-cli -- run main.ksl` 输出 `9 的平方 = 81`.

- 模块外部只能看到 `@Export` 列出的名字.
- 导出值的类型必须可见, 未导出的 Data 类型不能出现在导出签名里.
- 依赖图必须无环.
- 完整规则见 `docs/topics/language/modules.md`, 可运行的例子见 `examples/02_text_report/`.

## 8. Actor

Actor 是 ksl 的并发单位. `@Handler` 定义蓝图, `@Spawn` 启动, `@Send` 与 `@Receive` 传消息, `@Join` 等结果.

```ksl
@Let[worker, @Handler[[self : Pid[string, i64], client : Pid[string, nil]] : i64,
  @Block[
    @Let[_, @Send[client, 0u8, "任务完成"]],
    42
  ]
]];

@Main[[self : Pid[string, nil]] : nil,
  @Block[
    @Let[pid, @Spawn[worker, self]],
    @Let[message, @Receive[0u8, Option/Some[1000u64]]],
    @Match[message, {
      (Option/Some[text], Print[f"收到 {text}\n"]),
      (Option/None[], Print["超时\n"])
    }],
    @Let[result, @Join[pid]],
    @Match[result, {
      (Result/Ok[value], Print[f"worker 结束: {value}\n"]),
      (Result/Err[_reason], Print["worker 失败\n"])
    }]
  ]
];
```

- Actor 原语只允许出现在 `@Handler` 体与 `@Main` 体内.
- 信箱索引必须是 u8 字面量, 写 `0u8` 这样的形式.
- `Pid[T0, ..., Tn-1, R]` 的最后一个参数是 actor 的返回值类型, 前 n 个是各信箱的消息类型.
- 完整规则见 `docs/topics/language/actors.md`, 可运行的例子见 `examples/03_actor_service.ksl`.

## 9. 输入输出

示例读取当前工作目录下的 `data.txt`; 文件不存在时进入失败分支, 打印 `读取失败`.

```ksl
@Let[args, Args[]];
@Let[count, U64ToI64[@ListLength[args]]];
Print[f"参数个数 = {count}\n"];

@Let[content, ReadFile["data.txt"]];
@Match[content, {
  (Result/Ok[text], Print[f"读到 {text}\n"]),
  (Result/Err[_message], Print["读取失败\n"])
}];
```

- 可恢复的失败用 `FileError`, `InputError` 等域错误表示; 不可恢复的错误直接终止程序.
- 文件流用 `OpenRead` 与 `OpenWrite` 打开, 用 `ReadLineFrom` 与 `WriteString` 读写, 用完需要 `Close`.
- 终端按键用 `ReadKey`, 标准输入用 `ReadLine`; 连续按键交互先用 `EnterRawMode` 进入原始模式, 结束后用 `ExitRawMode` 退出.
- 完整清单见 `docs/topics/library/builtins-io.md`.

## 10. 下一步

- 交互模式: `cargo run -p ksl-cli -- repl`.
- 语言规则: `docs/topics/language/` 下的各篇.
- 内置能力清单: `docs/topics/library/` 下的各篇.
- 完整示例: `examples/` 目录, 从 `01_language_tour.ksl` 开始按能力递进.
