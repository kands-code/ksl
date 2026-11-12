//! builtin 模块测试.

use std::collections::HashSet;

use super::registry::TABLE;
use super::*;

#[test]
fn builtin_ids_are_unique_and_round_trip() {
    // 注册表是检查器与运行时的唯一来源; 编号与符号必须一一对应.
    let mut seen = HashSet::new();
    for (index, entry) in TABLE.iter().enumerate() {
        assert!(seen.insert(entry.symbol), "重复内置名: {}", entry.name);
        let expected = u16::try_from(index).expect("内置函数数量超出 u16");
        assert_eq!(super::id(entry.symbol).map(BuiltinId::index), Some(expected));
        assert_eq!(super::symbol(BuiltinId::from_index(expected)), entry.symbol);
        assert_eq!(super::name(BuiltinId::from_index(expected)), entry.name);
        assert!(super::signature(entry.symbol).is_some());
    }
}
