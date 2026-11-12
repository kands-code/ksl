//! 检查器与模块共用的类型上下文.
//!
//! 把 Data 表, 模块级值类型, 类型别名与类型表收在一处, 避免会话, 模块上下文
//! 与链接结果各自维护同构字段. 检查期间的词法作用域仍由 `Env` 单独维护.

use std::collections::HashMap;

use super::{DataTable, Type, TypeTable};
use crate::rewrite::{TypeName, TypeNames};
use crate::symbol::Symbol;

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
}

impl TypeContext {
    pub(crate) fn new(data: DataTable, values: HashMap<Symbol, Type>) -> Self {
        Self {
            data,
            values,
            type_names: TypeNames::new(),
            types: TypeTable::new(),
        }
    }

    /// 记录类型别名: 可见名指向真实类型.
    pub(crate) fn add_type_name(&mut self, local: Symbol, alias: TypeName) {
        self.type_names.insert(local, alias);
    }

    pub(crate) fn add_types(&mut self, types: TypeTable) {
        self.types.extend(types);
    }
}

/// 模块检查上下文; 与检查器共用同一结构.
pub type ModuleContext = TypeContext;
