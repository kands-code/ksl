---
updated: 2026-11-21
---

# ksl 正则

ksl 的正则表达式编译, 匹配, 改写与拆分.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.

## 1. 编译

正则由 `RegexCompile` 编译一次后反复使用.
模式串支持内联标志 `(?i)` `(?m)` `(?s)` `(?x)` `(?U)` `(?R)` `(?u)`.
匹配是线性时间的, 不含回溯, 因此匹配阶段不会失败.
引擎不支持环视与反向引用, 模式里出现这两者时报 `UnsupportedFeature`.

- `RegexCompile[pattern : string] : Result[Regex, RegexError]`
  - 编译模式串; 模式非法时返回 `RegexError`.
- `EmptyRegexMatch[] : RegexMatch`
  - 返回空匹配; 匹配文本为空, 只有整体匹配组.
    当前没有匹配而又必须给出 `RegexMatch` 值时使用.

## 2. 匹配

- `RegexIsMatch[regex : Regex, text : string] : bool`
  - 文本中是否有匹配.
- `RegexFind[regex : Regex, text : string] : Option[RegexMatch]`
  - 第一处匹配; 没有匹配时返回 None.
- `RegexFindAll[regex : Regex, text : string] : List[RegexMatch]`
  - 全部匹配, 按出现顺序排列.
- `RegexCaptureNames[regex : Regex] : List[string]`
  - 命名捕获组的名称, 按组下标升序.

## 3. 匹配结果

匹配结果是不透明类型 `RegexMatch`, 由取值函数读取.
偏移按 Unicode 标量计数, 与 `StringLength` 的计数方式一致.

- `RegexMatchText[match : RegexMatch] : string`
  - 整体匹配文本.
- `RegexMatchStart[match : RegexMatch] : u64`
  - 起始偏移.
- `RegexMatchEnd[match : RegexMatch] : u64`
  - 结束偏移.
- `RegexMatchGroup[match : RegexMatch, index : u64] : Option[string]`
  - 按下标取捕获组文本; 下标 0 是整体匹配, 1 起是捕获组.
    下标越界或组未参与匹配时返回 None.
- `RegexMatchNamedGroup[match : RegexMatch, name : string] : Option[string]`
  - 按名称取捕获组文本; 名称不存在或组未参与匹配时返回 None.

捕获组未参与匹配与匹配到空串不同: 前者取值为 None, 后者取值为空串.

## 4. 改写与拆分

替换串中的 `$1` 与 `${name}` 引用捕获组, `$$` 表示字面量 `$`.
引用不存在的组时替换为空串.

- `RegexReplace[regex : Regex, text : string, replacement : string] : string`
  - 改写第一处匹配, 其余文本原样保留.
- `RegexReplaceAll[regex : Regex, text : string, replacement : string] : string`
  - 改写全部非重叠匹配.
- `RegexSplit[regex : Regex, text : string] : List[string]`
  - 按匹配切分; 保留空片段.

## 5. 错误

正则编译失败用内置 Data 类型 `RegexError`, 变体为:
`Invalid[offset]`, `UnclosedGroup[offset]`, `UnopenedGroup[offset]`,
`UnclosedClass[offset]`, `InvalidClass[offset]`, `InvalidEscape[offset]`,
`InvalidNumber[offset]`, `InvalidRepeat[offset]`, `InvalidName[offset]`,
`DuplicateName[offset]`, `InvalidFlag[offset]`, `InvalidUnicode[offset]`,
`UnsupportedFeature[offset]`, `TooComplex[offset]`, `TooLarge[]`.

除 `TooLarge` 外都带出错位置, 即模式串中的字符偏移.
`TooLarge` 表示编译体积超出上限.
