---
updated: 2026-11-21
---

# ksl 容器

ksl 的列表, 配对, 字典, 集合, Option 与 Result 操作.
以下操作全部为特殊形式, 其类型含类型变量,
无法在语言内书写为函数类型.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
Data 类型的模式匹配见 `docs/topics/language/data.md` 第 3 节.

## 1. 列表

列表不可变; 所有变更返回新列表, 原列表不变.

- `@ListGet[list : List[T], index : u64] : T`
  - 取下标元素; 下标越界报错, 无负索引.
- `@ListUpdate[list : List[T], index : u64, value : T] : List[T]`
  - 返回替换下标元素后的新列表; 下标越界报错.
- `@ListPrepend[list : List[T], value : T] : List[T]`
  - 返回在开头插入一个元素的新列表.
- `@ListAppend[list : List[T], value : T] : List[T]`
  - 返回在末尾追加一个元素的新列表.
- `@ListConcatenate[left : List[T], right : List[T]] : List[T]`
  - 按顺序连接两个列表, 返回新列表.
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

配对访问使用特殊形式, 因为 `@Match` 只处理 Data.

## 3. 字典

`Dict[K, V]` 是不可变有序映射; 遍历按键排序, 结果稳定.
键限 i64, u8, u64, char, bool 与 string, 不支持 f64, Data, List, Pair 等.

- `@Dict[entries... : Pair[K, V]] : Dict[K, V]`
  - 构造字典; 重复键在后写覆盖先写.
- `@DictLookup[dict : Dict[K, V], key : K] : Option[V]`
  - 按键查找; 缺失时返回 None.
- `@DictGet[dict : Dict[K, V], key : K, default : V] : V`
  - 按键查找; 缺键时求值 default, 命中时不求值.
- `@DictPut[dict : Dict[K, V], key : K, value : V] : Dict[K, V]`
  - 返回写入键值后的新字典; 已有键被覆盖.
- `@DictUpdate[dict : Dict[K, V], key : K, f : Fn[[V] : V], default : V] : Dict[K, V]`
  - 命中时以旧值调用 f; 缺键时使用 default, 不调用 f.
- `@DictRemove[dict : Dict[K, V], key : K] : Dict[K, V]`
  - 返回删除键后的新字典; 键不存在时返回原字典.
- `@DictHas[dict : Dict[K, V], key : K] : bool`
  - 键存在时返回 true.
- `@DictLength[dict : Dict[K, V]] : u64`
  - 返回键值对个数.
- `@IsDictEmpty[dict : Dict[K, V]] : bool`
  - 字典为空时返回 true.
- `@DictEntries[dict : Dict[K, V]] : List[Pair[K, V]]`
  - 返回按键排序的键值对列表.
- `@DictKeys[dict : Dict[K, V]] : List[K]`
  - 返回按键排序的键列表.
- `@DictValues[dict : Dict[K, V]] : List[V]`
  - 返回按键排序的值列表.
- `@DictMerge[left : Dict[K, V], right : Dict[K, V]] : Dict[K, V]`
  - 合并两个字典; right 覆盖 left 的同名键.
- `@DictFilter[dict : Dict[K, V], f : Fn[[K, V] : bool]] : Dict[K, V]`
  - 保留谓词为真的键值对, 保持按键排序.
- `@DictFromEntries[entries : List[Pair[K, V]]] : Dict[K, V]`
  - 由键值对列表构造字典; 重复键在后写覆盖先写.

Dict 不可打印; 需要展示时用 `@DictEntries` 转为列表, 再按需格式化.

## 4. 集合

`Set[T]` 是不可变有序集合; 遍历按元素排序, 结果稳定.
元素限 i64, u8, u64, char, bool 与 string, 不支持 f64, Data, List, Pair 等.

- `@Set[items... : T] : Set[T]`
  - 构造集合; 重复元素只保留一个.
- `@SetInsert[set : Set[T], item : T] : Set[T]`
  - 返回插入元素后的新集合; 已存在时返回原集合.
- `@SetRemove[set : Set[T], item : T] : Set[T]`
  - 返回删除元素后的新集合; 元素不存在时返回原集合.
- `@SetHas[set : Set[T], item : T] : bool`
  - 元素存在时返回 true.
- `@SetLength[set : Set[T]] : u64`
  - 返回元素个数.
- `@IsSetEmpty[set : Set[T]] : bool`
  - 集合为空时返回 true.
- `@SetItems[set : Set[T]] : List[T]`
  - 返回按元素排序的列表.
- `@SetUnion[left : Set[T], right : Set[T]] : Set[T]`
  - 返回两个集合的并集.
- `@SetIntersect[left : Set[T], right : Set[T]] : Set[T]`
  - 返回两个集合的交集.
- `@SetDifference[left : Set[T], right : Set[T]] : Set[T]`
  - 返回存在于 left 但不存在于 right 的元素.
- `@SetFilter[set : Set[T], f : Fn[[T] : bool]] : Set[T]`
  - 保留谓词为真的元素, 保持元素排序.
- `@SetFromList[items : List[T]] : Set[T]`
  - 由列表构造集合; 重复元素只保留一个.

空字面量需要期望类型或标注.
Set 不可打印; 需要展示时用 `@SetItems` 转为列表, 再按需格式化.

## 5. Option 与 Result

`Option` 与 `Result` 的消费以 `@Match` 为主,
见 `docs/topics/language/data.md` 第 3 节.
其余便捷函数负责判定, 映射, 串联与默认取值.

- `@IsSome[option : Option[T]] : bool`
  - 值为 Some 时返回 true.
- `@IsNone[option : Option[T]] : bool`
  - 值为 None 时返回 true.
- `@IsOk[result : Result[T, E]] : bool`
  - 值为 Ok 时返回 true.
- `@IsErr[result : Result[T, E]] : bool`
  - 值为 Err 时返回 true.
- `@OptionMap[option : Option[T], f : Fn[[T] : U]] : Option[U]`
  - 有值时以值调用 f, 返回 Some[结果]; 无值时返回 None.
- `@ResultMap[result : Result[T, E], f : Fn[[T] : U]] : Result[U, E]`
  - 成功时以值调用 f, 返回 Ok[结果]; 失败时原样返回 Err.
- `@OptionAndThen[option : Option[T], f : Fn[[T] : Option[U]]] : Option[U]`
  - 有值时以值调用 f; 无值时返回 None.
- `@ResultAndThen[result : Result[T, E], f : Fn[[T] : Result[U, E]]] : Result[U, E]`
  - 成功时以值调用 f; 失败时原样返回 Err.
- `@OptionGet[option : Option[T], default : T] : T`
  - 有值时返回值, 无值时求值 default.
- `@ResultGet[result : Result[T, E], default : T] : T`
  - 成功时返回成功值, 失败时求值 default; 丢弃错误内容.
- `@ResultOrElse[result : Result[T, E], f : Fn[[E] : Result[T, F]]] : Result[T, F]`
  - 成功时原样返回该 Result, 失败时以错误内容调用 f 并以其返回值为结果.

映射, 串联与 `@ResultOrElse` 接收 `@Fn`, 而 `@Fn` 体内不能使用 Actor 原语;
分支中需要 `@Spawn` 与 `@Join` 等 Actor 原语时仍用 `@Match`.
`@ResultOrElse` 的替代函数只在失败路径上调用.
不提供 `MapErr` 与 `Filter`.
