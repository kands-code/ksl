---
updated: 2026-11-24
---

# ksl 算术与取整

整数与浮点算术, 绝对值与取整函数.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
数值类型见 `docs/topics/language/types.md` 第 2.1 节.

## 1. 算术

运算函数全部单态化, 按类型独立命名, 不做隐式类型转换.
整数运算上溢或下溢报运行时错误, 无符号类型同样.
除数为零报运行时错误.

f32 与 f64 遵循 IEEE 754: 溢出得无穷, 除零得无穷或 NaN, 不报运行时错误.

- `AddI64[left : i64, right : i64] : i64`
  - 返回两数之和; 溢出报运行时错误.
- `SubI64[left : i64, right : i64] : i64`
  - 返回两数之差; 溢出报运行时错误.
- `NegI64[value : i64] : i64`
  - 返回相反数; i64 最小值取负报运行时错误.
- `MulI64[left : i64, right : i64] : i64`
  - 返回两数之积; 溢出报运行时错误.
- `DivideI64[left : i64, right : i64] : i64`
  - 向下取整除, 商向负无穷取整; 除数为零或商超出范围报错.
- `QuotientI64[left : i64, right : i64] : i64`
  - 向零截断除; 除数为零或商超出范围报错.
- `RemainderI64[left : i64, right : i64] : i64`
  - 返回带号余数, 符号随被除数; 除数为零报错.
- `ModuloI64[left : i64, right : i64] : i64`
  - 返回取模结果, 符号随除数; 除数为零报错.
- `AddI32[left : i32, right : i32] : i32`
  - 返回两数之和; 溢出报运行时错误.
- `SubI32[left : i32, right : i32] : i32`
  - 返回两数之差; 溢出报运行时错误.
- `NegI32[value : i32] : i32`
  - 返回相反数; i32 最小值取负报运行时错误.
- `MulI32[left : i32, right : i32] : i32`
  - 返回两数之积; 溢出报运行时错误.
- `DivideI32[left : i32, right : i32] : i32`
  - 向下取整除, 商向负无穷取整; 除数为零或商超出范围报错.
- `QuotientI32[left : i32, right : i32] : i32`
  - 向零截断除; 除数为零或商超出范围报错.
- `RemainderI32[left : i32, right : i32] : i32`
  - 返回带号余数, 符号随被除数; 除数为零报错.
- `ModuloI32[left : i32, right : i32] : i32`
  - 返回取模结果, 符号随除数; 除数为零报错.
- `AddU8[left : u8, right : u8] : u8`
  - 返回两数之和; 溢出报运行时错误.
- `SubU8[left : u8, right : u8] : u8`
  - 返回两数之差; 下溢报运行时错误.
- `MulU8[left : u8, right : u8] : u8`
  - 返回两数之积; 溢出报运行时错误.
- `DivideU8[left : u8, right : u8] : u8`
  - 返回整除结果; 除数为零报错.
- `ModuloU8[left : u8, right : u8] : u8`
  - 返回取模结果; 除数为零报错.
- `AddU32[left : u32, right : u32] : u32`
  - 返回两数之和; 溢出报运行时错误.
- `SubU32[left : u32, right : u32] : u32`
  - 返回两数之差; 下溢报运行时错误.
- `MulU32[left : u32, right : u32] : u32`
  - 返回两数之积; 溢出报运行时错误.
- `DivideU32[left : u32, right : u32] : u32`
  - 向下取整除; 除数为零报运行时错误.
- `ModuloU32[left : u32, right : u32] : u32`
  - 返回取模结果; 除数为零报运行时错误.
- `AddU64[left : u64, right : u64] : u64`
  - 返回两数之和; 溢出报运行时错误.
- `SubU64[left : u64, right : u64] : u64`
  - 返回两数之差; 下溢报运行时错误.
- `MulU64[left : u64, right : u64] : u64`
  - 返回两数之积; 溢出报运行时错误.
- `DivideU64[left : u64, right : u64] : u64`
  - 返回整除结果; 除数为零报错.
- `ModuloU64[left : u64, right : u64] : u64`
  - 返回取模结果; 除数为零报错.
- `AddF64[left : f64, right : f64] : f64`
  - 返回两数之和.
- `SubF64[left : f64, right : f64] : f64`
  - 返回两数之差.
- `NegF64[value : f64] : f64`
  - 返回相反数.
- `MulF64[left : f64, right : f64] : f64`
  - 返回两数之积.
- `DivideF64[left : f64, right : f64] : f64`
  - 返回两数之商.
- `AddF32[left : f32, right : f32] : f32`
  - 返回两数之和.
- `SubF32[left : f32, right : f32] : f32`
  - 返回两数之差.
- `NegF32[value : f32] : f32`
  - 返回相反数.
- `MulF32[left : f32, right : f32] : f32`
  - 返回两数之积.
- `DivideF32[left : f32, right : f32] : f32`
  - 返回两数之商.

## 2. 绝对值与取整

- `AbsI64[value : i64] : i64`
  - 返回绝对值; i64 最小值取绝对值报错.
- `AbsI32[value : i32] : i32`
  - 返回绝对值; i32 最小值取绝对值报错.
- `AbsF64[value : f64] : f64`
  - 返回绝对值.
- `AbsF32[value : f32] : f32`
  - 返回绝对值.
- `TruncateF64[value : f64] : i64`
  - 向零截断为 i64; 结果超出 i64 范围报错.
- `RoundF64[value : f64] : i64`
  - 银行家舍入, 平局取偶; 结果超出 i64 范围报错.
- `CeilingF64[value : f64] : i64`
  - 向上取整为 i64; 结果超出 i64 范围报错.
- `FloorF64[value : f64] : i64`
  - 向下取整为 i64; 结果超出 i64 范围报错.
- `TruncateF32[value : f32] : i64`
  - 向零截断为 i64; 结果超出 i64 范围报错.
- `RoundF32[value : f32] : i64`
  - 银行家舍入, 平局取偶; 结果超出 i64 范围报错.
- `CeilingF32[value : f32] : i64`
  - 向上取整为 i64; 结果超出 i64 范围报错.
- `FloorF32[value : f32] : i64`
  - 向下取整为 i64; 结果超出 i64 范围报错.

结果不是有限值时, 取整函数同样报运行时错误, 不做截断.
