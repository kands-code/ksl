//! builtin 模块测试.

use std::collections::HashSet;

use ksl_catalog::builtin::{self, signature, symbol};

use super::*;

#[test]
fn builtin_ids_are_unique_and_round_trip() {
    // 注册表是检查器与运行时的唯一来源; 编号与名字必须一一对应.
    let mut seen = HashSet::new();
    for (index, entry) in builtin::specs().enumerate() {
        let id = BuiltinId::ALL[index];
        let symbol = symbol(id);
        assert_eq!(id.index(), index);
        assert_eq!(symbol.as_str(), entry.name);
        assert!(seen.insert(symbol.clone()), "重复内置名: {}", entry.name);
        assert_eq!(ksl_catalog::builtin::id(&symbol), Some(id));
        assert_eq!(
            ksl_catalog::builtin::is_pure(id),
            entry.kind == builtin::BuiltinKind::Pure
        );
        assert_eq!(ksl_catalog::builtin::name(id), entry.name);
        assert!(signature(&symbol).is_some());
    }
}
