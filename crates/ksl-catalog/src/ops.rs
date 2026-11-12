//! 操作元数据.
//!
//! 操作名, 种类与参数个数集中在本表.
//! 语法分析, 保留名与检查器的参数检查都由本表派生.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListOpKind {
    Get,
    Update,
    Prepend,
    Append,
    Concat,
    Head,
    Tail,
    Take,
    Drop,
    Reverse,
    Repeat,
    Flatten,
    Chunk,
    TakeWhile,
    DropWhile,
    Map,
    MapIndexed,
    Filter,
    Fold,
    PrefixFold,
    Any,
    All,
    Length,
    IsEmpty,
    Riffle,
    Window,
    Zip,
    ZipWith,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairOpKind {
    Fst,
    Snd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryOp {
    Register,
    WhereIs,
    Unregister,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictOpKind {
    Lookup,
    Get,
    Put,
    Update,
    Remove,
    Has,
    Length,
    IsEmpty,
    Entries,
    Keys,
    Values,
    Merge,
    Filter,
    FromEntries,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetOpKind {
    Insert,
    Remove,
    Has,
    Length,
    IsEmpty,
    Items,
    Union,
    Intersect,
    Difference,
    Filter,
    FromList,
}

/// Option 与 Result 的便捷操作种类.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionOpKind {
    /// 判定是否为 Some, 结果为 bool.
    IsSome,
    /// 判定是否为 None, 结果为 bool.
    IsNone,
    /// 判定是否为 Ok, 结果为 bool.
    IsOk,
    /// 判定是否为 Err, 结果为 bool.
    IsErr,
    /// Some 时应用函数并重新包装, None 时原样返回.
    OptionMap,
    /// Ok 时应用函数并重新包装, Err 时原样返回.
    ResultMap,
    /// Some 时应用函数, None 时原样返回.
    OptionAndThen,
    /// Ok 时应用函数, Err 时原样返回.
    ResultAndThen,
    /// Some 取值, None 取默认值.
    OptionGet,
    /// Ok 取值, Err 取默认值.
    ResultGet,
    /// Ok 时原样返回, Err 时以错误内容调用函数取替代值.
    ResultOrElse,
}

/// 操作元数据: 名字, 种类与参数个数.
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

pub const PAIR_OPS: &[OpSpec<PairOpKind>] = &[
    OpSpec::new("Fst", PairOpKind::Fst, 1),
    OpSpec::new("Snd", PairOpKind::Snd, 1),
];

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
    OpSpec::new("ResultOrElse", OptionOpKind::ResultOrElse, 2),
];

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

/// 按名字查列表操作种类.
#[must_use]
pub fn list_op_kind(name: &str) -> Option<ListOpKind> {
    kind_of(LIST_OPS, name)
}

/// 按名字查配对操作种类.
#[must_use]
pub fn pair_op_kind(name: &str) -> Option<PairOpKind> {
    kind_of(PAIR_OPS, name)
}

/// 按名字查字典操作种类.
#[must_use]
pub fn dict_op_kind(name: &str) -> Option<DictOpKind> {
    kind_of(DICT_OPS, name)
}

/// 按名字查集合操作种类.
#[must_use]
pub fn set_op_kind(name: &str) -> Option<SetOpKind> {
    kind_of(SET_OPS, name)
}

/// 按名字查 Option 与 Result 操作种类.
#[must_use]
pub fn option_op_kind(name: &str) -> Option<OptionOpKind> {
    kind_of(OPTION_OPS, name)
}

/// 按名字查命名注册操作种类.
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

macro_rules! op_accessors {
    ($ty:ty, $table:ident) => {
        impl $ty {
            /// 操作参数个数.
            #[must_use]
            pub fn arity(self) -> usize {
                arity_of($table, &self)
            }

            /// 操作名.
            #[must_use]
            pub fn name(self) -> &'static str {
                name_of($table, &self)
            }
        }
    };
}

op_accessors!(ListOpKind, LIST_OPS);
op_accessors!(PairOpKind, PAIR_OPS);
op_accessors!(DictOpKind, DICT_OPS);
op_accessors!(SetOpKind, SET_OPS);
op_accessors!(OptionOpKind, OPTION_OPS);
op_accessors!(RegistryOp, REGISTRY_OPS);

impl ListOpKind {
    /// 是否需要回调用户函数; 这类操作由抽象机的续延循环驱动.
    #[must_use]
    pub fn is_higher_order(self) -> bool {
        match self {
            Self::TakeWhile
            | Self::DropWhile
            | Self::Map
            | Self::MapIndexed
            | Self::Filter
            | Self::Fold
            | Self::PrefixFold
            | Self::Any
            | Self::All
            | Self::ZipWith => true,
            Self::Get
            | Self::Update
            | Self::Prepend
            | Self::Append
            | Self::Concat
            | Self::Head
            | Self::Tail
            | Self::Take
            | Self::Drop
            | Self::Reverse
            | Self::Repeat
            | Self::Flatten
            | Self::Chunk
            | Self::Length
            | Self::IsEmpty
            | Self::Riffle
            | Self::Window
            | Self::Zip => false,
        }
    }
}

impl DictOpKind {
    /// 是否直接产出结果, 不回调用户函数.
    #[must_use]
    pub fn is_synchronous(self) -> bool {
        self != Self::Filter
    }
}

impl SetOpKind {
    /// 是否直接产出结果, 不回调用户函数.
    #[must_use]
    pub fn is_synchronous(self) -> bool {
        self != Self::Filter
    }
}

impl OptionOpKind {
    /// 是否直接产出结果, 不回调用户函数.
    #[must_use]
    pub fn is_synchronous(self) -> bool {
        matches!(
            self,
            Self::IsSome
                | Self::IsNone
                | Self::IsOk
                | Self::IsErr
                | Self::OptionGet
                | Self::ResultGet
        )
    }
}
