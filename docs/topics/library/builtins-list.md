---
updated: 2026-11-25
---

# ksl 列表与配对

`List[T]` 与 `Pair[A, B]` 的操作.
以下操作全部为特殊形式.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
Data 类型的模式匹配见 `docs/topics/language/data.md` 第 4 节.

## 1. 列表

列表不可变; 所有变更返回新列表, 原列表不变.

- `@ListGet[list : List[T], index : u64] : T`
  - 取下标元素; 下标越界报错, 无负索引.
- `@ListLookup[list : List[T], index : u64] : Option[T]`
  - 取下标元素; 下标越界返回 None.
- `@ListUpdate[list : List[T], index : u64, value : T] : List[T]`
  - 返回替换下标元素后的新列表; 下标越界报错.
- `@ListPrepend[list : List[T], value : T] : List[T]`
  - 返回在开头插入一个元素的新列表.
- `@ListAppend[list : List[T], value : T] : List[T]`
  - 返回在末尾追加一个元素的新列表.
- `@ListHead[list : List[T]] : T`
  - 取首元素; 空列表报错.
- `@ListTail[list : List[T]] : List[T]`
  - 去掉首元素; 空列表返回空列表.
- `@ListTake[list : List[T], count : u64] : List[T]`
  - 取前 count 个元素; 不足 count 时返回整个列表.
- `@ListDrop[list : List[T], count : u64] : List[T]`
  - 丢弃前 count 个元素; 不足 count 时返回空列表.
- `@ListReverse[list : List[T]] : List[T]`
  - 返回元素顺序相反的列表.
- `@ListRepeat[value : T, count : u64] : List[T]`
  - 返回包含 count 个 value 的列表; count 为 0 时为空列表.
  - count 不设语言级上限, 内存规模由调用方负责.
- `@ListFlatten[lists : List[List[T]]] : List[T]`
  - 摊平一层嵌套; 内层列表按原顺序连接.
- `@ListChunk[list : List[T], size : u64] : List[List[T]]`
  - 按 size 个元素分组; 末块可以不足, size 为 0 报错.
- `@ListTakeWhile[list : List[T], f : Fn[[T] : bool]] : List[T]`
  - 取满足条件的最长前缀; 遇到第一个不满足条件的元素即停止.
- `@ListDropWhile[list : List[T], f : Fn[[T] : bool]] : List[T]`
  - 丢弃满足条件的最长前缀, 返回剩余列表.
- `@ListMap[list : List[T], f : Fn[[T] : U]] : List[U]`
  - 对每个元素调用 f, 返回结果列表; 空列表返回空列表.
- `@ListMapIndexed[list : List[T], f : Fn[[u64, T] : U]] : List[U]`
  - 对每个元素调用 f, 同时传入从 0 开始的下标.
- `@ListFilter[list : List[T], f : Fn[[T] : bool]] : List[T]`
  - 保留满足条件的元素, 保持原顺序.
- `@ListFindIndex[list : List[T], f : Fn[[T] : bool]] : Option[u64]`
  - 返回首个满足条件的元素下标; 未命中返回 None.
- `@ListFold[list : List[T], f : Fn[[T, U] : U], init : U] : U`
  - 从左到右累积; init 作为第一次调用的累积值.
- `@ListPrefixFold[list : List[T], f : Fn[[T, U] : U], init : U] : List[U]`
  - 返回含初始值的前缀累积列表; 结果长度比输入多一.
- `@ListAny[list : List[T], f : Fn[[T] : bool]] : bool`
  - 存在满足条件的元素时返回 true; 空列表返回 false.
- `@ListAll[list : List[T], f : Fn[[T] : bool]] : bool`
  - 全部元素满足条件时返回 true; 空列表返回 true.
- `@ListLength[list : List[T]] : u64`
  - 返回元素个数.
- `@IsListEmpty[list : List[T]] : bool`
  - 列表为空时返回 true.
- `@ListRiffle[list : List[T], separator : T, interval : u64] : List[T]`
  - 按 interval 插入分隔元素; interval 为 1 时相邻元素之间都插入,
    大于列表长度时返回原列表, 为 0 报错.
- `@ListWindow[list : List[T], size : u64] : List[List[T]]`
  - 生成固定大小的滑动窗口; 窗口大于列表长度时返回空列表,
    size 为 0 报错.
- `@ListZip[left : List[A], right : List[B]] : List[Pair[A, B]]`
  - 按位置配对; 长度不一致时截短到较短列表.
- `@ListZipWith[left : List[A], right : List[B], f : Fn[[A, B] : C]] : List[C]`
  - 按位置调用 f 合并两个列表; 长度不一致时截短到较短列表.

## 2. 配对

- `@Fst[pair : Pair[A, B]] : A`
  - 返回配对第一个分量.
- `@Snd[pair : Pair[A, B]] : B`
  - 返回配对第二个分量.

配对访问使用特殊形式; `@Match` 只处理 Data.
