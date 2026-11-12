---
updated: 2026-11-25
---

# ksl 字典与集合

`Dict[K, V]` 与 `Set[T]` 的操作.
两者都是不可变有序容器, 遍历按元素或键排序, 结果稳定.
以下操作全部为特殊形式.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.

键与元素限 i64, i32, u8, u32, u64, char, bool 与 string,
不支持 f32, f64, Data, List, Pair 等.

## 1. 字典

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

## 2. 集合

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
