---
updated: 2026-11-19
---

# ksl 数值与比较

ksl 的数值运算, 取整, 数学函数, 位运算, 类型转换与比较函数.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.
数值类型见 `docs/topics/language/types.md` 第 2.1 节.

## 1. 算术

运算函数全部单态化, 按类型独立命名, 不允许任何隐式类型转换.
整数运算上溢或下溢一律报运行时错误, 无符号类型不例外.
除数为零报运行时错误.
f64 运算遵循 IEEE 754: 溢出得无穷, 除零得无穷或 NaN, 不报运行时错误.

- `AddI64[left : i64, right : i64] : i64`
  - 返回两数之和; 溢出报运行时错误.
- `SubI64[left : i64, right : i64] : i64`
  - 返回两数之差; 溢出报运行时错误.
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
- `MulF64[left : f64, right : f64] : f64`
  - 返回两数之积.
- `DivideF64[left : f64, right : f64] : f64`
  - 返回两数之商.

## 2. 绝对值与取整

- `AbsI64[value : i64] : i64`
  - 返回绝对值; i64 最小值取绝对值报错.
- `AbsF64[value : f64] : f64`
  - 返回绝对值.
- `Truncate[value : f64] : i64`
  - 向零截断为 i64; 结果超出 i64 范围报错.
- `Round[value : f64] : i64`
  - 银行家舍入, 平局取偶; 结果超出 i64 范围报错.
- `Ceiling[value : f64] : i64`
  - 向上取整为 i64; 结果超出 i64 范围报错.
- `Floor[value : f64] : i64`
  - 向下取整为 i64; 结果超出 i64 范围报错.

四个取整函数在结果不是有限值时同样报运行时错误, 不做截断.

## 3. 数学函数

数学函数只有 f64 版本, 因此不加类型后缀.
定义域外的结果遵循 IEEE 754, 生成 NaN 或无穷而不报运行时错误.

- `Power[base : f64, exponent : f64] : f64`
  - 返回幂, 支持小数指数.
- `Sqrt[value : f64] : f64`
  - 返回平方根; 负数得 NaN.
- `Sin[value : f64] : f64`
  - 返回正弦值.
- `Cos[value : f64] : f64`
  - 返回余弦值.
- `Tan[value : f64] : f64`
  - 返回正切值.
- `ArcSin[value : f64] : f64`
  - 返回反正弦值.
- `ArcCos[value : f64] : f64`
  - 返回反余弦值.
- `ArcTan[value : f64] : f64`
  - 返回反正切值.
- `ArcTan2[x : f64, y : f64] : f64`
  - 返回二参数反正切; 参数顺序为 x, y, 等价于常规 `atan2(y, x)`.
- `Sinh[value : f64] : f64`
  - 返回双曲正弦值.
- `Cosh[value : f64] : f64`
  - 返回双曲余弦值.
- `Tanh[value : f64] : f64`
  - 返回双曲正切值.
- `ArcSinh[value : f64] : f64`
  - 返回反双曲正弦值.
- `ArcCosh[value : f64] : f64`
  - 返回反双曲余弦值.
- `ArcTanh[value : f64] : f64`
  - 返回反双曲正切值.

## 4. 位运算与移位

移位为全函数且满足结合律: 位数达到或超过字宽时结果为 0.
移出的位直接丢弃, 不属于溢出报错范围.

- `ShiftLeftI64[value : i64, bits : u8] : i64`
  - 逻辑左移.
- `ShiftRightI64[value : i64, bits : u8] : i64`
  - 逻辑右移, 高位补 0.
- `BitOrI64[left : i64, right : i64] : i64`
  - 按位或.
- `BitAndI64[left : i64, right : i64] : i64`
  - 按位与.
- `BitXorI64[left : i64, right : i64] : i64`
  - 按位异或.
- `BitReverseI64[value : i64] : i64`
  - 按位反转.
- `ShiftLeftU8[value : u8, bits : u8] : u8`
  - 逻辑左移.
- `ShiftRightU8[value : u8, bits : u8] : u8`
  - 逻辑右移.
- `BitOrU8[left : u8, right : u8] : u8`
  - 按位或.
- `BitAndU8[left : u8, right : u8] : u8`
  - 按位与.
- `BitXorU8[left : u8, right : u8] : u8`
  - 按位异或.
- `BitReverseU8[value : u8] : u8`
  - 按位反转.
- `ShiftLeftU64[value : u64, bits : u8] : u64`
  - 逻辑左移.
- `ShiftRightU64[value : u64, bits : u8] : u64`
  - 逻辑右移.
- `BitOrU64[left : u64, right : u64] : u64`
  - 按位或.
- `BitAndU64[left : u64, right : u64] : u64`
  - 按位与.
- `BitXorU64[left : u64, right : u64] : u64`
  - 按位异或.
- `BitReverseU64[value : u64] : u64`
  - 按位反转.

## 5. 数值转换

转换不会发生静默截断, 越界时报运行时错误.
u64 到 f64 的转换允许精度损失.

- `I64ToF64[value : i64] : f64`
  - i64 转 f64; 绝对值超过 2^53 时允许精度损失.
- `U64ToF64[value : u64] : f64`
  - u64 转 f64, 允许精度损失.
- `U8ToI64[value : u8] : i64`
  - u8 转 i64, 精确.
- `U8ToU64[value : u8] : u64`
  - u8 转 u64, 精确.
- `U64ToI64[value : u64] : i64`
  - u64 转 i64; 超出 i64 范围报错.
- `U64ToU8[value : u64] : u8`
  - u64 转 u8; 超出 u8 范围报错.
- `I64ToU8[value : i64] : u8`
  - i64 转 u8; 超出 u8 范围报错.
- `I64ToU64[value : i64] : u64`
  - i64 转 u64; 负数报错.

## 6. 数值比较

相等性按类型提供. 列表与 Data 不提供内置相等性, 由用户自行定义.
f64 的 NaN 相等比较为假.

- `EqualU8[left : u8, right : u8] : bool`
  - 相等.
- `EqualU64[left : u64, right : u64] : bool`
  - 相等.
- `EqualI64[left : i64, right : i64] : bool`
  - 相等.
- `EqualF64[left : f64, right : f64] : bool`
  - 相等.
- `LessU8[left : u8, right : u8] : bool`
  - 小于.
- `LessOrEqualU8[left : u8, right : u8] : bool`
  - 小于等于.
- `GreaterU8[left : u8, right : u8] : bool`
  - 大于.
- `GreaterOrEqualU8[left : u8, right : u8] : bool`
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
- `LessF64[left : f64, right : f64] : bool`
  - 小于.
- `LessOrEqualF64[left : f64, right : f64] : bool`
  - 小于等于.
- `GreaterF64[left : f64, right : f64] : bool`
  - 大于.
- `GreaterOrEqualF64[left : f64, right : f64] : bool`
  - 大于等于.

## 7. 布尔

布尔函数的参数先求值, 无短路语义.

- `Not[value : bool] : bool`
  - 逻辑非.
- `And[left : bool, right : bool] : bool`
  - 逻辑与.
- `Or[left : bool, right : bool] : bool`
  - 逻辑或.
- `EqualBool[left : bool, right : bool] : bool`
  - 相等.
