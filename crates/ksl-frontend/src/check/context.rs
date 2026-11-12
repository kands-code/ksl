//! 检查器与模块共用的类型上下文.
//!
//! 把 Data 表, 模块级值类型, 类型别名与类型表收在一处,
//! 供会话, 模块上下文与链接结果共用. 检查期间的词法作用域仍由 `Env` 单独维护.

use std::collections::{HashMap, HashSet};

use ksl_base::symbol::Symbol;

use super::{DataTable, Type, TypeTable};
use crate::rewrite::{TypeName, TypeNames};

/// 类型检查与模块接口共用的状态.
#[derive(Clone, Default)]
pub struct TypeContext {
    pub(crate) data: DataTable,
    /// 模块级可见值的类型快照.
    pub(crate) values: HashMap<Symbol, Type>,
    /// 可见类型名到真实类型的别名.
    pub(crate) type_names: TypeNames,
    /// 最近一次成功检查生成的类型表.
    pub(crate) types: TypeTable,
    /// 本模块 `@Link` 声明的值名; 其他绑定不得使用该名.
    pub(crate) link_values: HashSet<Symbol>,
    /// 本模块 `@Link` 声明的类型名; `@Data` 声明不得使用该名.
    pub(crate) link_types: HashSet<Symbol>,
    /// 可解析的不透明类型完全限定名.
    pub(crate) opaque_types: HashSet<Symbol>,
}

impl TypeContext {
    pub fn new(data: DataTable, values: HashMap<Symbol, Type>) -> Self {
        Self {
            data,
            values,
            type_names: TypeNames::new(),
            types: TypeTable::new(),
            link_values: HashSet::new(),
            link_types: HashSet::new(),
            opaque_types: HashSet::new(),
        }
    }

    /// 记录类型别名: 可见名指向真实类型.
    pub fn add_type_name(&mut self, local: Symbol, alias: TypeName) {
        self.type_names.insert(local, alias);
    }

    pub fn add_types(&mut self, types: TypeTable) {
        self.types.extend(types);
    }

    /// 登记不透明类型的完全名, 供类型表达式解析.
    pub fn add_opaque_type(&mut self, full: Symbol) {
        self.opaque_types.insert(full);
    }

    /// 登记 Link 类型; 本地名用于冲突检查, 完全名用于类型解析.
    pub fn add_link_type(&mut self, local: Symbol, full: Symbol) {
        self.link_types.insert(local);
        self.opaque_types.insert(full);
    }

    /// 登记 Link 值名; 之后的绑定使用该名时报错.
    pub fn add_link_value(&mut self, local: Symbol) {
        self.link_values.insert(local);
    }
}

/// 模块检查上下文; 与检查器共用同一结构.
pub type ModuleContext = TypeContext;
