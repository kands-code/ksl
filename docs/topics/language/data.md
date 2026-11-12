---
updated: 2026-11-27
---

# ksl Data 类型与模式匹配

用户定义的联合类型, 构造器与模式匹配的语义.
类型体系见 `docs/topics/language/types.md`, 求值模型见 `docs/topics/language/evaluation.md`.

## 1. 定义

Data 类型用 `@Data` 声明, 例如 `@Data[Shape, {Point[], Circle[f64], Rect[f64, f64]}]`.

- Data 类型至少要有一个变体; `@Data[D, {}]` 无法实例化, 报错.
- 字段类型必须显式书写; 变体不得与类型重名.
- `@Data` 仅限顶层声明.
- 类型声明允许前向引用与相互引用. 类型检查阶段先收集全部声明, 再逐项检查,
  因此声明顺序与语句求值顺序无关.
- Data 值自底向上构造, 不可能形成环.

## 2. 构造与使用

构造器以类型限定名书写, 例如 `Shape/Circle[10.2]`.
单一变体示例: `@Data[Person, {Init[string, u8]}]`, 使用 `Person/Init["Kevin", 24u8]`.

## 3. 字段访问

单变体 Data 可以用 `@DataGet[值, 下标]` 按位置读取字段:

```ksl
@Data[Person, {Init[string, u8]}];

@Let[p, Person/Init["Kevin", 24u8]];
Print[f"{@DataGet[p, 0u8]} {@DataGet[p, 1u8]}\n"];
```

- 下标必须是 u8 字面量, 例如 `0u8`; 变量或表达式不接受.
- 只有单变体 Data 支持字段访问; 多变体在检查阶段报错, 应使用 `@Match`.
- 被读取值的类型必须在当前环境中可见, 见 `docs/topics/language/modules.md` 第 3 节.
- 下标越界在检查阶段报错; 结果类型是字段的声明类型.
- 每个变体的字段数上限为 256, 与 u8 下标范围一致.
- 嵌套单变体可以连续嵌套调用, 例如 `@DataGet[@DataGet[outer, 0u8], 1u8]`.
- 运行时不做变体检查; 单变体由类型保证.

## 4. 模式匹配

`@Match[值, {(模式, 结果), ...}]` 执行模式匹配. 每个分支是圆括号包裹的模式与结果对.

- 模式形态为 `[类型/]构造器[字段绑定, ...]`.
  写明类型时直接写 `(Job/Stop[], 结果)`;
  省略类型时以 `/` 开头, 由被匹配值的类型推断, 例如 `(/Stop[], 结果)`.
  省略类型时, 被匹配值的类型必须在当前环境中可见, 见
  `docs/topics/language/modules.md` 第 3 节.
- 字段绑定为变量名或 `_`; 字段类型由被匹配值的类型决定.
- 匹配必须穷尽, 漏写变体在类型检查阶段报错. 不允许嵌套解构, 不引入守卫模式.
- 通配模式 `_` 匹配任意变体且不绑定字段, 作为最后的分支.
  它满足穷尽性, 但其后不可再有分支.

按变体分派并绑定字段:

```ksl
@Data[Shape, {Point[], Circle[f64], Rect[f64, f64]}];

@Def[describe, [s : Shape] : string,
  @Match[s, {
    (/Point[], "原点"),
    (/Circle[r], f"半径 {r}"),
    (/Rect[w, h], f"{w} 乘 {h}")
  }]];

@Let[text, describe[Shape/Circle[2.5]]];
Print[f"{text}\n"];
```

同名构造器出现在不同类型时, 构造处必须写全限定名; 模式按被匹配值的类型可省略限定.

通配模式作为最后分支:

```ksl
@Data[Either2, {Left[i64], Right[i64]}];

@Def[describe, [p : Either2] : string,
  @Match[p, {
    (/Left[n], f"左边 {n}"),
    (/Right[n], f"右边 {n}"),
    (_, "未知变体")
  }]];

@Let[text, describe[Either2/Left[3]]];
Print[f"{text}\n"];
```

## 5. 变更

Data 不提供原地修改. 变更一律由 `@Match` 解构后重建:
复用字段绑定原名, 新字段直接构造.

```ksl
@Data[Box, {Item[i64]}];

@Let[original, Box/Item[1]];

@Let[updated, @Match[original, {
  (/Item[n], Box/Item[AddI64[n, 10]])
}]];

@Def[unwrap, [b : Box] : i64, @DataGet[b, 0u8]];

@Let[before, unwrap[original]];
@Let[after, unwrap[updated]];
Print[f"原值 {before}, 新值 {after}\n"];
```

## 6. 输出

Data 类型不可打印; 需要输出时由用户实现格式化函数.
`%type` 与非打印结果的展示使用会话别名; 函数签名与不透明类型按真实类型名显示.
同一真实类型有多个可见别名时, 取声明或导入顺序中的第一个.
