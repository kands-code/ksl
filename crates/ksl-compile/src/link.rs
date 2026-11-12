//! 模块链接: 建立导入/导出接口并完成检查.
//!
//! 值保持当前模块的本地名字, 由运行时环境隔离.
//! 类型名由检查器按绑定解析为真实身份, AST 保留源码中的本地名;
//! `lib/a/A` 与导入方的本地别名指向同一个类型身份.

use std::collections::HashMap;

use indexmap::IndexMap;
use ksl_base::name::Name;
use ksl_base::plugin::PluginSource;
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};
use ksl_frontend::ast::{DataVariant, ExprKind, Special};
use ksl_frontend::check::{self, DataInfo, DataTable, TypeBindings, TypeEnv, TypeTarget};

use crate::graph::ModuleId;

mod exports;
mod imports;
mod linker;
mod main;
mod plugin;

pub(crate) use linker::link_and_check_cached;
pub(crate) use main::{normalize_main, take_main};

/// 值的来源: 定义模块与导出名.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueRef {
    pub module: ModuleId,
    pub name: Name,
}

/// 类型的真实身份; `full` 是唯一的完全限定名.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub full: Name,
    pub module: ModuleId,
    pub name: Name,
    /// 可见名的引入位置.
    pub span: Span,
    /// Link 类型的所属插件; 普通类型为空.
    pub(crate) plugin: Option<PluginSource>,
}

impl TypeRef {
    /// 该类型引用对应的真实类型类别.
    #[must_use]
    pub(crate) fn target(&self) -> TypeTarget {
        if self.plugin.is_some() {
            TypeTarget::Opaque(self.full.clone())
        } else {
            TypeTarget::Data(self.full.clone())
        }
    }
}

/// 模块内可见的插件类型: 所属插件与描述符中的类型名.
#[derive(Debug, Clone)]
pub struct PluginType {
    pub source: PluginSource,
    pub name: Name,
}

/// 模块接口: 导入方可见的值, 类型与导出值类型.
#[derive(Debug, Clone, Default)]
pub struct ModuleInterface {
    pub values: IndexMap<Name, ValueRef>,
    pub types: IndexMap<Name, TypeRef>,
    /// `@Link` 声明的值; 与普通导出的值分开记录.
    pub link_values: IndexMap<Name, ValueRef>,
    /// `@Link` 声明的类型; 与普通导出的类型分开记录.
    pub link_types: IndexMap<Name, TypeRef>,
    /// 导出 Data 类型的声明; 按真实身份索引.
    pub type_decls: HashMap<Name, DataInfo>,
    /// 导出值的类型; 含 Link 值.
    pub value_types: ValueTypes,
}

impl ModuleInterface {
    /// 按可见名查找值, 包含普通导出与 Link 导出.
    #[must_use]
    pub fn value(&self, name: &Name) -> Option<&ValueRef> {
        self.values.get(name).or_else(|| self.link_values.get(name))
    }

    /// 按可见名查找类型, 包含普通导出与 Link 导出.
    #[must_use]
    pub fn type_ref(&self, name: &Name) -> Option<&TypeRef> {
        self.types.get(name).or_else(|| self.link_types.get(name))
    }
}

/// 已检查模块: 可执行程序, 入口与执行元数据.
#[derive(Debug)]
pub struct CheckedModule {
    pub id: ModuleId,
    pub path: std::path::PathBuf,
    /// 已检查程序; 入口声明保存在 `main`.
    pub program: ksl_typed::CheckedProgram,
    /// `@Link` 声明; 执行期据此加载动态库.
    pub links: Vec<ksl_frontend::ast::LinkDecl>,
    /// 插件名到链接阶段解析出的来源.
    pub link_sources: HashMap<String, PluginSource>,
    /// 可见插件类型的所属来源与描述符类型名.
    pub plugin_types: HashMap<Name, PluginType>,
    /// 本地导入名到真实值来源.
    pub imports: HashMap<Name, ValueRef>,
    /// 本地导入名到导入值的类型.
    pub imported_value_types: HashMap<Name, Type>,
    /// 本地导入名到导入类型的真实名.
    pub imported_bindings: TypeBindings,
    /// 导入 Data 类型的声明; 按真实身份索引, 供交互会话恢复.
    pub imported_data: DataTable,
    /// 导出接口与导出值类型.
    pub interface: ModuleInterface,
}

/// 已检查的模块图; 按依赖顺序保存可执行模块.
#[derive(Debug)]
pub struct CheckedGraph {
    pub root: ModuleId,
    pub order: Vec<ModuleId>,
    pub modules: HashMap<ModuleId, CheckedModule>,
}

impl CheckedGraph {
    #[must_use]
    pub fn get(&self, id: &ModuleId) -> Option<&CheckedModule> {
        self.modules.get(id)
    }

    /// 主模块导出的类型环境; 供 REPL 进入时恢复可见名字.
    ///
    /// # Panics
    ///
    /// 根模块不在图中时 panic; 加载器先链接根模块.
    #[must_use]
    pub fn main_context(&self) -> check::ModuleContext {
        let module = self.modules.get(&self.root).expect("根模块已链接");
        let mut data = DataTable::default();
        for (identity, info) in &module.interface.type_decls {
            data.insert(identity.clone(), info.clone());
        }
        let mut type_env = TypeEnv::with_data(data);
        for (name, target) in module
            .interface
            .types
            .iter()
            .chain(&module.interface.link_types)
        {
            type_env.bind(name.clone(), target.target(), target.span);
        }
        check::ModuleContext::new(type_env, module.interface.value_types.clone())
    }

    /// 主模块导入的类型环境; 供行内 `@Use` 并入会话.
    ///
    /// # Panics
    ///
    /// 根模块不在图中时 panic; 加载器先链接根模块.
    #[must_use]
    pub fn import_context(&self) -> check::ModuleContext {
        let module = self.modules.get(&self.root).expect("根模块已链接");
        let mut data = DataTable::default();
        for (_, binding) in module.imported_bindings.iter() {
            let TypeTarget::Data(identity) = &binding.target else {
                continue;
            };
            let info = module.imported_data.get(identity).expect("导入类型应有声明");
            data.insert(identity.clone(), info.clone());
        }
        let mut type_env = TypeEnv::with_data(data);
        let _ = type_env.bindings_mut().merge(&module.imported_bindings);
        check::ModuleContext::new(type_env, module.imported_value_types.clone())
    }
}

/// 一个模块的值类型表.
pub type ValueTypes = HashMap<Name, Type>;

fn insert_value_import(
    imports: &mut HashMap<Name, ValueRef>,
    local: Name,
    target: ValueRef,
    span: Span,
) -> Result<(), KslError> {
    if imports.contains_key(&local) {
        return Err(KslError::duplicate_import(&local, span));
    }
    imports.insert(local, target);
    Ok(())
}

fn insert_type_import(
    types: &mut HashMap<Name, TypeRef>,
    local: Name,
    target: TypeRef,
    span: Span,
) -> Result<(), KslError> {
    if types.contains_key(&local) {
        return Err(KslError::duplicate_import(&local, span));
    }
    // 可见名的引入位置取导入项位置.
    types.insert(local, TypeRef { span, ..target });
    Ok(())
}

fn check_local_name(name: &Name, span: Span) -> Result<(), KslError> {
    if check::is_reserved(name) {
        return Err(KslError::reserved_name(name, span));
    }
    Ok(())
}

fn local_type_names(
    program: &ksl_frontend::ast::Program,
) -> Result<HashMap<Name, Span>, KslError> {
    let mut names = HashMap::new();
    for stmt in program.exprs() {
        let ExprKind::Special(Special::Data { name, variants }) = &stmt.node else {
            continue;
        };
        if names.insert(name.node.clone(), name.span).is_some() {
            return Err(KslError::with_name(
                ErrorKind::DuplicateData,
                name.span,
                &name.node,
            ));
        }
        for DataVariant { name: variant, .. } in variants {
            if variant.node.clone() == name.node.clone() {
                return Err(KslError::new(ErrorKind::VariantNameConflict, variant.span));
            }
        }
    }
    Ok(names)
}

fn local_value_names(program: &ksl_frontend::ast::Program) -> HashMap<Name, Span> {
    let mut names = HashMap::new();
    for stmt in program.exprs() {
        match &stmt.node {
            ExprKind::Special(Special::Let { target, .. }) => {
                if let Some(name) = &target.name {
                    names.insert(name.node.clone(), name.span);
                }
            },
            ExprKind::Special(Special::Def(def)) => {
                names.insert(def.name.node.clone(), def.name.span);
            },
            ExprKind::Special(Special::Fix(defs)) => {
                for def in defs {
                    names.insert(def.name.node.clone(), def.name.span);
                }
            },
            _ => {},
        }
    }
    names
}
