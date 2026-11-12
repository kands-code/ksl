---
updated: 2026-11-24
---

# ksl 数值比较与布尔

标量相等性与大小比较, 以及布尔运算.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
字符串与字符的相等性见 `docs/topics/library/builtins-string.md` 第 1 节与第 2 节.

## 1. 数值比较

相等性按类型提供. 列表与 Data 不提供内置相等性, 由用户自行定义.
f32 与 f64 的 NaN 相等比较为假.

- `EqualU8[left : u8, right : u8] : bool`
  - 相等.
- `EqualU32[left : u32, right : u32] : bool`
  - 相等.
- `EqualU64[left : u64, right : u64] : bool`
  - 相等.
- `EqualI64[left : i64, right : i64] : bool`
  - 相等.
- `EqualI32[left : i32, right : i32] : bool`
  - 相等.
- `EqualF64[left : f64, right : f64] : bool`
  - 相等.
- `EqualF32[left : f32, right : f32] : bool`
  - 相等.
- `LessU8[left : u8, right : u8] : bool`
  - 小于.
- `LessOrEqualU8[left : u8, right : u8] : bool`
  - 小于等于.
- `GreaterU8[left : u8, right : u8] : bool`
  - 大于.
- `GreaterOrEqualU8[left : u8, right : u8] : bool`
  - 大于等于.
- `LessU32[left : u32, right : u32] : bool`
  - 小于.
- `LessOrEqualU32[left : u32, right : u32] : bool`
  - 小于等于.
- `GreaterU32[left : u32, right : u32] : bool`
  - 大于.
- `GreaterOrEqualU32[left : u32, right : u32] : bool`
  - 大于等于.
- `LessU64[left : u64, right : u64] : bool`
  - 小于.
- `LessOrEqualU64[left : u64, right : u64] : bool`
  - 小于等于.
- `GreaterU64[left : u64, right : u64] : bool`
  - 大于.
- `GreaterOrEqualU64[left : u64, right : u64] : bool`
  - 大于等于.
- `LessI64[left : i64, right : i64] : bool`
  - 小于.
- `LessOrEqualI64[left : i64, right : i64] : bool`
  - 小于等于.
- `GreaterI64[left : i64, right : i64] : bool`
  - 大于.
- `GreaterOrEqualI64[left : i64, right : i64] : bool`
  - 大于等于.
- `LessI32[left : i32, right : i32] : bool`
  - 小于.
- `LessOrEqualI32[left : i32, right : i32] : bool`
  - 小于等于.
- `GreaterI32[left : i32, right : i32] : bool`
  - 大于.
- `GreaterOrEqualI32[left : i32, right : i32] : bool`
  - 大于等于.
- `LessF64[left : f64, right : f64] : bool`
  - 小于.
- `LessOrEqualF64[left : f64, right : f64] : bool`
  - 小于等于.
- `GreaterF64[left : f64, right : f64] : bool`
  - 大于.
- `GreaterOrEqualF64[left : f64, right : f64] : bool`
  - 大于等于.
- `LessF32[left : f32, right : f32] : bool`
  - 小于.
- `LessOrEqualF32[left : f32, right : f32] : bool`
  - 小于等于.
- `GreaterF32[left : f32, right : f32] : bool`
  - 大于.
- `GreaterOrEqualF32[left : f32, right : f32] : bool`
  - 大于等于.

## 2. 布尔

布尔函数的参数先求值, 无短路语义.

- `Not[value : bool] : bool`
  - 逻辑非.
- `And[left : bool, right : bool] : bool`
  - 逻辑与.
- `Or[left : bool, right : bool] : bool`
  - 逻辑或.
- `EqualBool[left : bool, right : bool] : bool`
  - 相等.
