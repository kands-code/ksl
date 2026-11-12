---
updated: 2026-11-24
---

# ksl 数学函数

幂, 根, 三角与双曲函数.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.

数学函数提供 f32 与 f64 两个版本, 名称以 `F32` 或 `F64` 结尾.
定义域外的结果遵循 IEEE 754, 生成 NaN 或无穷, 不报运行时错误.

## 1. 幂与根

- `PowerF64[base : f64, exponent : f64] : f64`
  - 返回幂, 支持小数指数.
- `PowerF32[base : f32, exponent : f32] : f32`
  - 返回幂, 支持小数指数.
- `SqrtF64[value : f64] : f64`
  - 返回平方根; 负数得 NaN.
- `SqrtF32[value : f32] : f32`
  - 返回平方根; 负数得 NaN.

## 2. 三角与反三角

- `SinF64[value : f64] : f64`
  - 返回正弦值.
- `SinF32[value : f32] : f32`
  - 返回正弦值.
- `CosF64[value : f64] : f64`
  - 返回余弦值.
- `CosF32[value : f32] : f32`
  - 返回余弦值.
- `TanF64[value : f64] : f64`
  - 返回正切值.
- `TanF32[value : f32] : f32`
  - 返回正切值.
- `ArcSinF64[value : f64] : f64`
  - 返回反正弦值.
- `ArcSinF32[value : f32] : f32`
  - 返回反正弦值.
- `ArcCosF64[value : f64] : f64`
  - 返回反余弦值.
- `ArcCosF32[value : f32] : f32`
  - 返回反余弦值.
- `ArcTanF64[value : f64] : f64`
  - 返回反正切值.
- `ArcTanF32[value : f32] : f32`
  - 返回反正切值.
- `ArcTan2F64[x : f64, y : f64] : f64`
  - 返回二参数反正切; 参数顺序为 x, y, 等价于常规 `atan2(y, x)`.
- `ArcTan2F32[x : f32, y : f32] : f32`
  - 返回二参数反正切; 参数顺序为 x, y, 等价于常规 `atan2(y, x)`.

## 3. 双曲与反双曲

- `SinhF64[value : f64] : f64`
  - 返回双曲正弦值.
- `SinhF32[value : f32] : f32`
  - 返回双曲正弦值.
- `CoshF64[value : f64] : f64`
  - 返回双曲余弦值.
- `CoshF32[value : f32] : f32`
  - 返回双曲余弦值.
- `TanhF64[value : f64] : f64`
  - 返回双曲正切值.
- `TanhF32[value : f32] : f32`
  - 返回双曲正切值.
- `ArcSinhF64[value : f64] : f64`
  - 返回反双曲正弦值.
- `ArcSinhF32[value : f32] : f32`
  - 返回反双曲正弦值.
- `ArcCoshF64[value : f64] : f64`
  - 返回反双曲余弦值.
- `ArcCoshF32[value : f32] : f32`
  - 返回反双曲余弦值.
- `ArcTanhF64[value : f64] : f64`
  - 返回反双曲正切值.
- `ArcTanhF32[value : f32] : f32`
  - 返回反双曲正切值.
