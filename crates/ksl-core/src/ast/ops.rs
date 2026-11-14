//! 特殊形式的操作元数据.
//!
//! 操作名, 种类与参数个数集中在本表; 语法分析, 保留名与检查器的参数检查
//! 都从这里派生, 避免多份名单在改名或新增操作时漂移.

use super::{DictOpKind, ListOpKind, OptionOpKind, PairOpKind, RegistryOp, SetOpKind};

/// 一项操作元数据.
#[derive(Debug, Clone, Copy)]
pub struct OpSpec<K> {
    pub name: &'static str,
    pub kind: K,
    pub arity: usize,
}

impl<K> OpSpec<K> {
    const fn new(name: &'static str, kind: K, arity: usize) -> Self {
        Self { name, kind, arity }
    }
}

/// 列表操作.
pub const LIST_OPS: &[OpSpec<ListOpKind>] = &[
    OpSpec::new("ListGet", ListOpKind::Get, 2),
    OpSpec::new("ListUpdate", ListOpKind::Update, 3),
    OpSpec::new("ListPrepend", ListOpKind::Prepend, 2),
    OpSpec::new("ListAppend", ListOpKind::Append, 2),
    OpSpec::new("ListConcatenate", ListOpKind::Concat, 2),
    OpSpec::new("ListHead", ListOpKind::Head, 1),
    OpSpec::new("ListTail", ListOpKind::Tail, 1),
    OpSpec::new("ListTake", ListOpKind::Take, 2),
    OpSpec::new("ListDrop", ListOpKind::Drop, 2),
    OpSpec::new("ListReverse", ListOpKind::Reverse, 1),
    OpSpec::new("ListRepeat", ListOpKind::Repeat, 2),
    OpSpec::new("ListFlatten", ListOpKind::Flatten, 1),
    OpSpec::new("ListChunk", ListOpKind::Chunk, 2),
    OpSpec::new("ListTakeWhile", ListOpKind::TakeWhile, 2),
    OpSpec::new("ListDropWhile", ListOpKind::DropWhile, 2),
    OpSpec::new("ListMap", ListOpKind::Map, 2),
    OpSpec::new("ListMapIndexed", ListOpKind::MapIndexed, 2),
    OpSpec::new("ListFilter", ListOpKind::Filter, 2),
    OpSpec::new("ListFold", ListOpKind::Fold, 3),
    OpSpec::new("ListPrefixFold", ListOpKind::PrefixFold, 3),
    OpSpec::new("ListAny", ListOpKind::Any, 2),
    OpSpec::new("ListAll", ListOpKind::All, 2),
    OpSpec::new("ListLength", ListOpKind::Length, 1),
    OpSpec::new("IsListEmpty", ListOpKind::IsEmpty, 1),
    OpSpec::new("ListRiffle", ListOpKind::Riffle, 3),
    OpSpec::new("ListWindow", ListOpKind::Window, 2),
    OpSpec::new("ListZip", ListOpKind::Zip, 2),
    OpSpec::new("ListZipWith", ListOpKind::ZipWith, 3),
];

/// 配对访问操作.
pub const PAIR_OPS: &[OpSpec<PairOpKind>] = &[
    OpSpec::new("Fst", PairOpKind::Fst, 1),
    OpSpec::new("Snd", PairOpKind::Snd, 1),
];

/// Dict 操作.
pub const DICT_OPS: &[OpSpec<DictOpKind>] = &[
    OpSpec::new("DictLookup", DictOpKind::Lookup, 2),
    OpSpec::new("DictGet", DictOpKind::Get, 3),
    OpSpec::new("DictPut", DictOpKind::Put, 3),
    OpSpec::new("DictUpdate", DictOpKind::Update, 4),
    OpSpec::new("DictRemove", DictOpKind::Remove, 2),
    OpSpec::new("DictHas", DictOpKind::Has, 2),
    OpSpec::new("DictLength", DictOpKind::Length, 1),
    OpSpec::new("IsDictEmpty", DictOpKind::IsEmpty, 1),
    OpSpec::new("DictEntries", DictOpKind::Entries, 1),
    OpSpec::new("DictKeys", DictOpKind::Keys, 1),
    OpSpec::new("DictValues", DictOpKind::Values, 1),
    OpSpec::new("DictMerge", DictOpKind::Merge, 2),
    OpSpec::new("DictFilter", DictOpKind::Filter, 2),
    OpSpec::new("DictFromEntries", DictOpKind::FromEntries, 1),
];

/// Set 操作.
pub const SET_OPS: &[OpSpec<SetOpKind>] = &[
    OpSpec::new("SetInsert", SetOpKind::Insert, 2),
    OpSpec::new("SetRemove", SetOpKind::Remove, 2),
    OpSpec::new("SetHas", SetOpKind::Has, 2),
    OpSpec::new("SetLength", SetOpKind::Length, 1),
    OpSpec::new("IsSetEmpty", SetOpKind::IsEmpty, 1),
    OpSpec::new("SetItems", SetOpKind::Items, 1),
    OpSpec::new("SetUnion", SetOpKind::Union, 2),
    OpSpec::new("SetIntersect", SetOpKind::Intersect, 2),
    OpSpec::new("SetDifference", SetOpKind::Difference, 2),
    OpSpec::new("SetFilter", SetOpKind::Filter, 2),
    OpSpec::new("SetFromList", SetOpKind::FromList, 1),
];

/// Option 与 Result 操作.
pub const OPTION_OPS: &[OpSpec<OptionOpKind>] = &[
    OpSpec::new("IsSome", OptionOpKind::IsSome, 1),
    OpSpec::new("IsNone", OptionOpKind::IsNone, 1),
    OpSpec::new("IsOk", OptionOpKind::IsOk, 1),
    OpSpec::new("IsErr", OptionOpKind::IsErr, 1),
    OpSpec::new("OptionMap", OptionOpKind::OptionMap, 2),
    OpSpec::new("ResultMap", OptionOpKind::ResultMap, 2),
    OpSpec::new("OptionAndThen", OptionOpKind::OptionAndThen, 2),
    OpSpec::new("ResultAndThen", OptionOpKind::ResultAndThen, 2),
    OpSpec::new("OptionGet", OptionOpKind::OptionGet, 2),
    OpSpec::new("ResultGet", OptionOpKind::ResultGet, 2),
];

/// 命名注册表操作.
pub const REGISTRY_OPS: &[OpSpec<RegistryOp>] = &[
    OpSpec::new("Register", RegistryOp::Register, 2),
    OpSpec::new("WhereIs", RegistryOp::WhereIs, 1),
    OpSpec::new("Unregister", RegistryOp::Unregister, 1),
];

fn kind_of<K: Copy>(table: &[OpSpec<K>], name: &str) -> Option<K> {
    table
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.kind)
}

fn spec_of<K: PartialEq>(table: &'static [OpSpec<K>], kind: &K) -> &'static OpSpec<K> {
    table
        .iter()
        .find(|spec| &spec.kind == kind)
        .unwrap_or_else(|| panic!("操作元数据缺失"))
}

fn arity_of<K: PartialEq>(table: &'static [OpSpec<K>], kind: &K) -> usize {
    spec_of(table, kind).arity
}

fn name_of<K: PartialEq>(table: &'static [OpSpec<K>], kind: &K) -> &'static str {
    spec_of(table, kind).name
}

/// 按名字查列表操作.
#[must_use]
pub fn list_op_kind(name: &str) -> Option<ListOpKind> {
    kind_of(LIST_OPS, name)
}

/// 按名字查配对操作.
#[must_use]
pub fn pair_op_kind(name: &str) -> Option<PairOpKind> {
    kind_of(PAIR_OPS, name)
}

/// 按名字查 Dict 操作.
#[must_use]
pub fn dict_op_kind(name: &str) -> Option<DictOpKind> {
    kind_of(DICT_OPS, name)
}

/// 按名字查 Set 操作.
#[must_use]
pub fn set_op_kind(name: &str) -> Option<SetOpKind> {
    kind_of(SET_OPS, name)
}

/// 按名字查 Option 与 Result 操作.
#[must_use]
pub fn option_op_kind(name: &str) -> Option<OptionOpKind> {
    kind_of(OPTION_OPS, name)
}

/// 按名字查命名注册表操作.
#[must_use]
pub fn registry_op_kind(name: &str) -> Option<RegistryOp> {
    kind_of(REGISTRY_OPS, name)
}

/// 全部操作名的并集; 保留名与 tree-sitter 一致性测试使用.
pub fn operation_names() -> impl Iterator<Item = &'static str> {
    LIST_OPS
        .iter()
        .map(|spec| spec.name)
        .chain(PAIR_OPS.iter().map(|spec| spec.name))
        .chain(DICT_OPS.iter().map(|spec| spec.name))
        .chain(SET_OPS.iter().map(|spec| spec.name))
        .chain(OPTION_OPS.iter().map(|spec| spec.name))
        .chain(REGISTRY_OPS.iter().map(|spec| spec.name))
}

impl ListOpKind {
    #[must_use]
    pub fn arity(self) -> usize {
        arity_of(LIST_OPS, &self)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        name_of(LIST_OPS, &self)
    }
}

impl PairOpKind {
    #[must_use]
    pub fn arity(self) -> usize {
        arity_of(PAIR_OPS, &self)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        name_of(PAIR_OPS, &self)
    }
}

impl DictOpKind {
    #[must_use]
    pub fn arity(self) -> usize {
        arity_of(DICT_OPS, &self)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        name_of(DICT_OPS, &self)
    }
}

impl SetOpKind {
    #[must_use]
    pub fn arity(self) -> usize {
        arity_of(SET_OPS, &self)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        name_of(SET_OPS, &self)
    }
}

impl OptionOpKind {
    #[must_use]
    pub fn arity(self) -> usize {
        arity_of(OPTION_OPS, &self)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        name_of(OPTION_OPS, &self)
    }
}

impl RegistryOp {
    #[must_use]
    pub fn arity(self) -> usize {
        arity_of(REGISTRY_OPS, &self)
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        name_of(REGISTRY_OPS, &self)
    }
}
