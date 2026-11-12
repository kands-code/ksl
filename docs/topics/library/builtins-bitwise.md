---
updated: 2026-11-25
---

# ksl 位运算与移位

整数的按位运算与移位.
类型记号见 `docs/topics/library/builtins.md` 第 3 节.

移位为全函数且满足结合律: 位数达到或超过字宽时结果为 0.
移出的位直接丢弃, 不报运行时错误.

## 1. i64

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

## 2. i32

- `ShiftLeftI32[value : i32, bits : u8] : i32`
  - 逻辑左移.
- `ShiftRightI32[value : i32, bits : u8] : i32`
  - 逻辑右移, 高位补 0.
- `BitOrI32[left : i32, right : i32] : i32`
  - 按位或.
- `BitAndI32[left : i32, right : i32] : i32`
  - 按位与.
- `BitXorI32[left : i32, right : i32] : i32`
  - 按位异或.
- `BitReverseI32[value : i32] : i32`
  - 按位反转.

## 3. u8

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

## 4. u32

- `ShiftLeftU32[value : u32, bits : u8] : u32`
  - 逻辑左移.
- `ShiftRightU32[value : u32, bits : u8] : u32`
  - 逻辑右移.
- `BitOrU32[left : u32, right : u32] : u32`
  - 按位或.
- `BitAndU32[left : u32, right : u32] : u32`
  - 按位与.
- `BitXorU32[left : u32, right : u32] : u32`
  - 按位异或.
- `BitReverseU32[value : u32] : u32`
  - 按位反转.

## 5. u64

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
