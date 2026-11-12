---
updated: 2026-11-24
---

# ksl 数值转换

整数与浮点之间的显式转换.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
字符与标量值的转换见 `docs/topics/library/builtins-string.md` 第 2 节.

整数与浮点之间的转换不做静默截断, 超出目标范围时报运行时错误.
转到 f32, 以及 i64 或 u64 转到 f64 时允许精度损失; 其余拓宽转换精确.
f64 转 f32 按 IEEE 754 舍入, 溢出得无穷.

## 1. 整数到浮点

- `I64ToF64[value : i64] : f64`
  - i64 转 f64; 绝对值超过 2^53 时允许精度损失.
- `I64ToF32[value : i64] : f32`
  - i64 转 f32; 允许精度损失.
- `I32ToF64[value : i32] : f64`
  - i32 转 f64, 精确.
- `I32ToF32[value : i32] : f32`
  - i32 转 f32; 允许精度损失.
- `U32ToF64[value : u32] : f64`
  - u32 转 f64, 精确.
- `U32ToF32[value : u32] : f32`
  - u32 转 f32, 允许精度损失.
- `U64ToF64[value : u64] : f64`
  - u64 转 f64, 允许精度损失.
- `U64ToF32[value : u64] : f32`
  - u64 转 f32, 允许精度损失.

## 2. 浮点之间

- `F32ToF64[value : f32] : f64`
  - f32 转 f64, 精确.
- `F64ToF32[value : f64] : f32`
  - f64 转 f32, 按 IEEE 754 舍入, 溢出得无穷.

## 3. 整数之间

- `U8ToI64[value : u8] : i64`
  - u8 转 i64, 精确.
- `U8ToI32[value : u8] : i32`
  - u8 转 i32, 精确.
- `U8ToU32[value : u8] : u32`
  - u8 转 u32, 精确.
- `U8ToU64[value : u8] : u64`
  - u8 转 u64, 精确.
- `I32ToI64[value : i32] : i64`
  - i32 转 i64, 精确.
- `I32ToU64[value : i32] : u64`
  - i32 转 u64; 负数报错.
- `U32ToI64[value : u32] : i64`
  - u32 转 i64, 精确.
- `U32ToU64[value : u32] : u64`
  - u32 转 u64, 精确.
- `I64ToI32[value : i64] : i32`
  - i64 转 i32; 超出 i32 范围报错.
- `I64ToU32[value : i64] : u32`
  - i64 转 u32; 负数或超出 u32 范围报错.
- `I64ToU64[value : i64] : u64`
  - i64 转 u64; 负数报错.
- `U64ToI64[value : u64] : i64`
  - u64 转 i64; 超出 i64 范围报错.
- `U64ToI32[value : u64] : i32`
  - u64 转 i32; 超出 i32 范围报错.
- `U64ToU32[value : u64] : u32`
  - u64 转 u32; 超出 u32 范围报错.
- `U64ToU8[value : u64] : u8`
  - u64 转 u8; 超出 u8 范围报错.
- `I32ToU8[value : i32] : u8`
  - i32 转 u8; 超出 u8 范围报错.
- `I32ToU32[value : i32] : u32`
  - i32 转 u32; 负数报错.
- `U32ToI32[value : u32] : i32`
  - u32 转 i32; 超出 i32 范围报错.
- `U32ToU8[value : u32] : u8`
  - u32 转 u8; 超出 u8 范围报错.
- `I64ToU8[value : i64] : u8`
  - i64 转 u8; 超出 u8 范围报错.
