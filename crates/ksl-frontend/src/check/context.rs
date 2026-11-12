//! 检查器与模块共用的类型上下文.
//!
//! 把 Data 表, 类型名绑定与模块级值类型收在一处,
//! 供会话, 模块上下文与链接结果共用. 检查期间的词法作用域仍由 `Env` 单独维护.

use std::collections::{HashMap, HashSet};

use ksl_base::display::TypeNames;
use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::reserved;

use super::DataTable;

/// 可见类型名指向的真实类型身份.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeTarget {
    /// 用户 Data 或内置 Data.
    Data(Name),
    /// 文件, 套接字与插件句柄等不透明类型.
    Opaque(Name),
}

impl TypeTarget {
    /// 真实类型名; Data 与 Opaque 的名字空间不同, 但统一以该名字作为类型身份.
    #[must_use]
    pub fn full(&self) -> Name {
        match self {
            Self::Data(name) | Self::Opaque(name) => name.clone(),
        }
    }

    #[must_use]
    pub fn to_type(self) -> Type {
        match self {
            Self::Data(name) => Type::data(name),
            Self::Opaque(name) => Type::opaque(name),
        }
    }
}

/// 一个可见类型名的绑定.
#[derive(Debug, Clone)]
pub struct TypeBinding {
    pub target: TypeTarget,
    /// 可见名的引入位置; 本地声明为声明处, 导入为导入项.
    pub span: Span,
}

impl TypeBinding {
    #[must_use]
    pub fn new(target: TypeTarget, span: Span) -> Self {
        Self { target, span }
    }
}

/// 可见类型名到真实类型的映射.
///
/// 持久化映射使会话克隆只做结构共享; 需要顺序时按绑定位置排序.
#[derive(Debug, Clone, Default)]
pub struct TypeBindings {
    names: imbl::HashMap<Name, TypeBinding>,
}

impl TypeBindings {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn get(&self, name: &Name) -> Option<&TypeBinding> {
        self.names.get(name)
    }

    #[must_use]
    pub fn contains_key(&self, name: &Name) -> bool {
        self.names.contains_key(name)
    }

    /// 插入绑定; 返回被替换的绑定.
    pub fn insert(&mut self, name: Name, binding: TypeBinding) -> Option<TypeBinding> {
        self.names.insert(name, binding)
    }

    /// 是否存在指向该真实身份的可见名.
    #[must_use]
    pub fn contains_target(&self, identity: &Name) -> bool {
        self.names.values().any(|binding| match &binding.target {
            TypeTarget::Data(name) | TypeTarget::Opaque(name) => name == identity,
        })
    }

    /// 按引入位置排序的绑定; 同位置按名字排序.
    fn ordered(&self) -> Vec<(&Name, &TypeBinding)> {
        let mut entries: Vec<_> = self.names.iter().collect();
        entries.sort_by(|left, right| {
            (left.1.span.start, left.1.span.end, left.0.as_str()).cmp(&(
                right.1.span.start,
                right.1.span.end,
                right.0.as_str(),
            ))
        });
        entries
    }

    /// 绑定表迭代; 顺序不保证, 稳定显示使用 [`Self::type_names`].
    pub fn iter(&self) -> impl Iterator<Item = (&Name, &TypeBinding)> {
        self.names.iter()
    }

    /// 构造真实名到可见名的显示表.
    #[must_use]
    pub fn type_names(&self) -> TypeNames {
        TypeNames::new(
            self.ordered()
                .into_iter()
                .map(|(local, binding)| (binding.target.full(), local.clone())),
        )
    }

    /// 合并另一张绑定表; 同名但指向不同真实类型时返回冲突的可见名与传入绑定.
    pub fn merge(&mut self, other: &Self) -> Option<(Name, TypeBinding)> {
        if let Some(conflict) = other.ordered().into_iter().find_map(|(name, incoming)| {
            self.get(name)
                .filter(|existing| existing.target != incoming.target)
                .map(|_| (name.clone(), incoming.clone()))
        }) {
            return Some(conflict);
        }
        for (name, incoming) in other.iter() {
            self.insert(name.clone(), incoming.clone());
        }
        None
    }
}

/// 检查期的类型环境: Data 表与可见类型名绑定.
///
/// 默认环境含全部内置 Data 与内置不透明类型.
#[derive(Debug, Clone)]
pub struct TypeEnv {
    data: DataTable,
    bindings: TypeBindings,
}

impl Default for TypeEnv {
    fn default() -> Self {
        Self::with_data(DataTable::default())
    }
}

impl TypeEnv {
    /// 以已有的 Data 表构造环境, 并登记内置类型名.
    #[must_use]
    pub fn with_data(data: DataTable) -> Self {
        let mut bindings = TypeBindings::new();
        for name in reserved::BUILTIN_DATA_TYPE_NAMES.iter() {
            let name = Name::new(name);
            bindings.insert(
                name.clone(),
                TypeBinding::new(TypeTarget::Data(name), Span::default()),
            );
        }
        for name in reserved::BUILTIN_OPAQUE_TYPE_NAMES {
            let name = Name::new(name);
            bindings.insert(
                name.clone(),
                TypeBinding::new(TypeTarget::Opaque(name), Span::default()),
            );
        }
        Self { data, bindings }
    }

    #[must_use]
    pub fn data(&self) -> &DataTable {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut DataTable {
        &mut self.data
    }

    #[must_use]
    pub fn bindings(&self) -> &TypeBindings {
        &self.bindings
    }

    pub fn bindings_mut(&mut self) -> &mut TypeBindings {
        &mut self.bindings
    }

    /// 当前类型环境中真实名到可见名的显示表.
    #[must_use]
    pub fn type_names(&self) -> TypeNames {
        self.bindings.type_names()
    }

    #[must_use]
    pub fn resolve(&self, name: &Name) -> Option<TypeTarget> {
        self.bindings.get(name).map(|binding| binding.target.clone())
    }

    /// 真实身份是否在当前环境有可见名.
    #[must_use]
    pub fn is_visible(&self, identity: &Name) -> bool {
        self.bindings.contains_target(identity)
    }

    pub fn bind(&mut self, name: Name, target: TypeTarget, span: Span) {
        self.bindings.insert(name, TypeBinding::new(target, span));
    }

    /// 合并另一份类型环境; 绑定冲突时不修改自身.
    pub fn merge(&mut self, other: &Self) -> Option<(Name, TypeBinding)> {
        if let Some(conflict) = self.bindings.merge(&other.bindings) {
            return Some(conflict);
        }
        self.data.merge(&other.data);
        None
    }
}

/// 类型检查与模块接口共用的状态.
#[derive(Clone, Default)]
pub struct TypeContext {
    /// Data 表与可见类型名绑定.
    pub(crate) type_env: TypeEnv,
    /// 模块级可见值的类型快照.
    pub(crate) values: HashMap<Name, Type>,
    /// 本模块 `@Link` 声明的值名; 其他绑定不得使用该名.
    pub(crate) link_values: HashSet<Name>,
}

impl TypeContext {
    pub fn new(type_env: TypeEnv, values: HashMap<Name, Type>) -> Self {
        Self {
            type_env,
            values,
            link_values: HashSet::new(),
        }
    }

    /// 登记 Link 值名; 之后的绑定使用该名时报错.
    pub fn add_link_value(&mut self, local: Name) {
        self.link_values.insert(local);
    }
}

/// 模块检查上下文; 与检查器共用同一结构.
pub type ModuleContext = TypeContext;
