//! 操作元数据.
//!
//! 操作名, 种类与参数个数集中在本表.
//! 语法分析, 保留名与检查器的参数检查都由本表派生.

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

/// 由一份操作清单生成枚举, 元数据表与名字查询.
macro_rules! define_ops {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => ($text:literal, $arity:literal)
            ),* $(,)?
        }
        $table:ident;
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )*
        }

        impl $name {
            /// 操作参数个数.
            #[must_use]
            pub fn arity(self) -> usize {
                match self {
                    $(
                        Self::$variant => $arity,
                    )*
                }
            }

            /// 操作名.
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $text,
                    )*
                }
            }
        }

        $vis const $table: &[OpSpec<$name>] = &[
            $(
                OpSpec::new($text, $name::$variant, $arity),
            )*
        ];
    };
}

define_ops! {
    /// 列表操作种类.
    pub enum ListOpKind {
        Get => ("ListGet", 2),
        Lookup => ("ListLookup", 2),
        Update => ("ListUpdate", 3),
        Prepend => ("ListPrepend", 2),
        Append => ("ListAppend", 2),
        Head => ("ListHead", 1),
        Tail => ("ListTail", 1),
        Take => ("ListTake", 2),
        Drop => ("ListDrop", 2),
        Reverse => ("ListReverse", 1),
        Repeat => ("ListRepeat", 2),
        Flatten => ("ListFlatten", 1),
        Chunk => ("ListChunk", 2),
        TakeWhile => ("ListTakeWhile", 2),
        DropWhile => ("ListDropWhile", 2),
        Map => ("ListMap", 2),
        MapIndexed => ("ListMapIndexed", 2),
        Filter => ("ListFilter", 2),
        FindIndex => ("ListFindIndex", 2),
        Fold => ("ListFold", 3),
        PrefixFold => ("ListPrefixFold", 3),
        Any => ("ListAny", 2),
        All => ("ListAll", 2),
        Length => ("ListLength", 1),
        IsEmpty => ("IsListEmpty", 1),
        Riffle => ("ListRiffle", 3),
        Window => ("ListWindow", 2),
        Zip => ("ListZip", 2),
        ZipWith => ("ListZipWith", 3),
    }
    LIST_OPS;
}

define_ops! {
    /// 配对操作种类.
    pub enum PairOpKind {
        Fst => ("Fst", 1),
        Snd => ("Snd", 1),
    }
    PAIR_OPS;
}

define_ops! {
    /// 字典操作种类.
    pub enum DictOpKind {
        Lookup => ("DictLookup", 2),
        Get => ("DictGet", 3),
        Put => ("DictPut", 3),
        Update => ("DictUpdate", 4),
        Remove => ("DictRemove", 2),
        Has => ("DictHas", 2),
        Length => ("DictLength", 1),
        IsEmpty => ("IsDictEmpty", 1),
        Entries => ("DictEntries", 1),
        Keys => ("DictKeys", 1),
        Values => ("DictValues", 1),
        Merge => ("DictMerge", 2),
        Filter => ("DictFilter", 2),
        FromEntries => ("DictFromEntries", 1),
    }
    DICT_OPS;
}

define_ops! {
    /// 集合操作种类.
    pub enum SetOpKind {
        Insert => ("SetInsert", 2),
        Remove => ("SetRemove", 2),
        Has => ("SetHas", 2),
        Length => ("SetLength", 1),
        IsEmpty => ("IsSetEmpty", 1),
        Items => ("SetItems", 1),
        Union => ("SetUnion", 2),
        Intersect => ("SetIntersect", 2),
        Difference => ("SetDifference", 2),
        Filter => ("SetFilter", 2),
        FromList => ("SetFromList", 1),
    }
    SET_OPS;
}

define_ops! {
    /// Option 与 Result 的便捷操作种类.
    pub enum OptionOpKind {
        /// 判定是否为 Some, 结果为 bool.
        IsSome => ("IsSome", 1),
        /// 判定是否为 None, 结果为 bool.
        IsNone => ("IsNone", 1),
        /// 判定是否为 Ok, 结果为 bool.
        IsOk => ("IsOk", 1),
        /// 判定是否为 Err, 结果为 bool.
        IsErr => ("IsErr", 1),
        /// Some 时应用函数并重新包装, None 时原样返回.
        OptionMap => ("OptionMap", 2),
        /// Ok 时应用函数并重新包装, Err 时原样返回.
        ResultMap => ("ResultMap", 2),
        /// Some 时应用函数, None 时原样返回.
        OptionAndThen => ("OptionAndThen", 2),
        /// Ok 时应用函数, Err 时原样返回.
        ResultAndThen => ("ResultAndThen", 2),
        /// Some 取值, None 取默认值.
        OptionGet => ("OptionGet", 2),
        /// Ok 取值, Err 取默认值.
        ResultGet => ("ResultGet", 2),
        /// Ok 时原样返回, Err 时以错误内容调用函数取替代值.
        ResultOrElse => ("ResultOrElse", 2),
    }
    OPTION_OPS;
}

fn kind_of<K: Copy>(table: &[OpSpec<K>], name: &str) -> Option<K> {
    table
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.kind)
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

/// 全部操作名的并集; 保留名与 tree-sitter 一致性测试使用.
pub fn operation_names() -> impl Iterator<Item = &'static str> {
    LIST_OPS
        .iter()
        .map(|spec| spec.name)
        .chain(PAIR_OPS.iter().map(|spec| spec.name))
        .chain(DICT_OPS.iter().map(|spec| spec.name))
        .chain(SET_OPS.iter().map(|spec| spec.name))
        .chain(OPTION_OPS.iter().map(|spec| spec.name))
}

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
            | Self::FindIndex
            | Self::Fold
            | Self::PrefixFold
            | Self::Any
            | Self::All
            | Self::ZipWith => true,
            Self::Get
            | Self::Lookup
            | Self::Update
            | Self::Prepend
            | Self::Append
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
    /// Dict 操作是否直接产出结果, 不回调用户函数.
    #[must_use]
    pub fn is_synchronous(self) -> bool {
        self != Self::Filter
    }
}

impl SetOpKind {
    /// Set 操作是否直接产出结果, 不回调用户函数.
    #[must_use]
    pub fn is_synchronous(self) -> bool {
        self != Self::Filter
    }
}

impl OptionOpKind {
    /// Option 与 Result 操作是否直接产出结果, 不回调用户函数.
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
