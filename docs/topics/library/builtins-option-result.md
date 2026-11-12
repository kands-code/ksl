---
updated: 2026-11-25
---

# ksl Option 与 Result

`Option[T]` 与 `Result[T, E]` 的便捷操作.
两者通常用 `@Match` 消费, 见 `docs/topics/language/data.md` 第 4 节.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.

## 1. 判定

- `@IsSome[option : Option[T]] : bool`
  - 值为 Some 时返回 true.
- `@IsNone[option : Option[T]] : bool`
  - 值为 None 时返回 true.
- `@IsOk[result : Result[T, E]] : bool`
  - 值为 Ok 时返回 true.
- `@IsErr[result : Result[T, E]] : bool`
  - 值为 Err 时返回 true.

## 2. 映射与串联

- `@OptionMap[option : Option[T], f : Fn[[T] : U]] : Option[U]`
  - 有值时以值调用 f, 返回 Some[结果]; 无值时返回 None.
- `@ResultMap[result : Result[T, E], f : Fn[[T] : U]] : Result[U, E]`
  - 成功时以值调用 f, 返回 Ok[结果]; 失败时原样返回 Err.
- `@OptionAndThen[option : Option[T], f : Fn[[T] : Option[U]]] : Option[U]`
  - 有值时以值调用 f; 无值时返回 None.
- `@ResultAndThen[result : Result[T, E], f : Fn[[T] : Result[U, E]]] : Result[U, E]`
  - 成功时以值调用 f; 失败时原样返回 Err.
- `@ResultOrElse[result : Result[T, E], f : Fn[[E] : Result[T, F]]] : Result[T, F]`
  - 成功时原样返回该 Result, 失败时以错误内容调用 f 并以其返回值为结果.

## 3. 默认取值

- `@OptionGet[option : Option[T], default : T] : T`
  - 有值时返回值, 无值时求值 default.
- `@ResultGet[result : Result[T, E], default : T] : T`
  - 成功时返回成功值, 失败时求值 default; 丢弃错误内容.

映射, 串联与 `@ResultOrElse` 接收 `@Fn`, 而 `@Fn` 体内不能使用 Actor 原语.
分支中需要 `@Spawn` 与 `@Join` 等 Actor 原语时仍用 `@Match`.
`@ResultOrElse` 的替代函数只在失败路径上调用.
不提供 `MapErr` 与 `Filter`.
