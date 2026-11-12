//! 模块链接: 建立导入/导出接口, 重写类型名并完成检查.
//!
//! 值保持当前模块的本地名字, 由运行时环境隔离. 类型名重写为完全限定名,
//! 因此 `lib/a/A` 与导入方的本地别名最终指向同一个类型身份.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, resolve};
use ksl_frontend::ast::{DataVariant, Expr, ExprKind, Special};
use ksl_frontend::check::{self, DataTable, HandlerType};
use ksl_frontend::error::{ErrorKind, KslError};
use ksl_frontend::rewrite::{self, TypeNames};

use super::ModuleId;
use crate::ffi::PluginSource;

mod exports;
mod imports;
mod linker;
mod main;
mod plugin;
mod rewriting;

pub(crate) use linker::{link_and_check, link_and_check_cached};
pub(crate) use main::{normalize_main, take_main};

/// 值的来源: 定义模块与导出名.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueRef {
    pub module: ModuleId,
    pub name: Symbol,
}

/// 类型的真实身份; `full` 是唯一的完全限定名.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    pub full: Symbol,
    pub module: ModuleId,
    pub name: Symbol,
    /// 可见名的引入位置.
    pub span: Span,
    /// Link 类型的所属插件; 普通类型为空.
    pub(crate) plugin: Option<PluginSource>,
}

/// 模块内可见的插件类型: 所属插件与描述符中的类型名.
#[derive(Debug, Clone)]
pub(crate) struct PluginType {
    pub(crate) source: PluginSource,
    pub(crate) name: Symbol,
}

#[derive(Debug, Clone, Default)]
pub struct Exports {
    pub values: IndexMap<Symbol, ValueRef>,
    pub types: IndexMap<Symbol, TypeRef>,
    /// `@Link` 声明的值; 与普通导出的值分开记录.
    pub link_values: IndexMap<Symbol, ValueRef>,
    /// `@Link` 声明的类型; 与普通导出的类型分开记录.
    pub link_types: IndexMap<Symbol, TypeRef>,
}

impl Exports {
    /// 按可见名查找值, 包含普通导出与 Link 导出.
    #[must_use]
    pub fn value(&self, name: Symbol) -> Option<&ValueRef> {
        self.values.get(&name).or_else(|| self.link_values.get(&name))
    }

    /// 按可见名查找类型, 包含普通导出与 Link 导出.
    #[must_use]
    pub fn type_ref(&self, name: Symbol) -> Option<&TypeRef> {
        self.types.get(&name).or_else(|| self.link_types.get(&name))
    }
}

/// 模块入口声明: 根 Actor 的语法节点与内部蓝图类型.
#[derive(Debug, Clone)]
pub struct MainEntry {
    /// `@Main` 声明节点; 仅根模块求值并 spawn.
    pub expr: Expr,
    /// 入口的 Handler 蓝图类型; 内部降级与执行使用.
    pub handler: HandlerType,
}

#[derive(Debug)]
pub struct LinkedModule {
    pub id: ModuleId,
    pub path: std::path::PathBuf,
    pub program: ksl_frontend::ast::Program,
    /// 模块入口声明; 依赖模块只检查不执行.
    pub main: Option<MainEntry>,
    /// `@Link` 声明; 执行期据此加载动态库.
    pub links: Vec<ksl_frontend::ast::LinkDecl>,
    /// 插件名到链接阶段解析出的来源.
    pub(crate) link_sources: HashMap<String, PluginSource>,
    /// 可见插件类型的所属来源与描述符类型名.
    pub(crate) plugin_types: HashMap<Symbol, PluginType>,
    /// 本地导入名到真实值来源.
    pub imports: HashMap<Symbol, ValueRef>,
    /// 本地导入名到导入值的类型.
    pub imported_value_types: HashMap<Symbol, ksl_frontend::check::Type>,
    /// 本地导入名到导入类型的真实名; 本地名只是别名.
    pub imported_type_names: TypeNames,
    pub exports: Exports,
    /// 导出名到值类型.
    pub value_types: HashMap<Symbol, ksl_frontend::check::Type>,
    /// 本模块源码区间到类型.
    pub types: ksl_frontend::check::TypeTable,
    /// 本模块可见的不透明类型完全名; 含导入的 Link 类型.
    pub opaque_types: HashSet<Symbol>,
}

#[derive(Debug)]
pub struct LinkedGraph {
    pub root: ModuleId,
    pub order: Vec<ModuleId>,
    pub modules: HashMap<ModuleId, LinkedModule>,
    pub(crate) data: DataTable,
    /// 不含根模块的 Data 表; 根模块不进入会话缓存, 其 Data 声明也不进入.
    pub(crate) cache_data: DataTable,
}

impl LinkedGraph {
    #[must_use]
    pub fn get(&self, id: &ModuleId) -> Option<&LinkedModule> {
        self.modules.get(id)
    }

    /// 主模块导出的类型环境; 供 REPL 进入时恢复可见名字.
    ///
    /// # Panics
    ///
    /// 根模块不在图中时 panic; 加载器保证根模块已链接.
    #[must_use]
    pub fn main_context(&self) -> check::ModuleContext {
        let module = self.modules.get(&self.root).expect("根模块已链接");
        let mut context =
            check::ModuleContext::new(self.data.clone(), module.value_types.clone());
        for (name, target) in module.exports.types.iter().chain(&module.exports.link_types) {
            context.add_type_name(
                *name,
                rewrite::TypeName {
                    full: target.full,
                    span: target.span,
                },
            );
        }
        for (name, target) in &module.exports.link_types {
            context.add_link_type(*name, target.full);
        }
        for full in &module.opaque_types {
            context.add_opaque_type(*full);
        }
        context.add_types(module.types.clone());
        context
    }

    /// 主模块导入的类型环境; 供行内 `@Use` 并入会话.
    ///
    /// # Panics
    ///
    /// 根模块不在图中时 panic; 加载器保证根模块已链接.
    #[must_use]
    pub fn import_context(&self) -> check::ModuleContext {
        let module = self.modules.get(&self.root).expect("根模块已链接");
        let mut context =
            check::ModuleContext::new(self.data.clone(), module.imported_value_types.clone());
        for (name, alias) in &module.imported_type_names {
            context.add_type_name(*name, *alias);
            if module.opaque_types.contains(&alias.full) {
                context.add_opaque_type(alias.full);
            }
        }
        context.add_types(module.types.clone());
        context
    }
}

/// 一个模块的导出值类型表.
type ModuleValueTypes = HashMap<Symbol, ksl_frontend::check::Type>;

/// 会话模块缓存中的导出值类型表.
type CachedValueTypes = imbl::HashMap<ModuleId, ModuleValueTypes>;

pub(super) fn link_name_conflict(name: Symbol, span: Span) -> KslError {
    KslError::with_subject(
        ErrorKind::LinkNameConflict {
            name: resolve(name).to_string(),
        },
        span,
        resolve(name).to_string(),
    )
}

fn insert_value_import(
    imports: &mut HashMap<Symbol, ValueRef>,
    local: Symbol,
    target: ValueRef,
    span: Span,
) -> Result<(), KslError> {
    if imports.contains_key(&local) {
        return Err(KslError::new(
            ErrorKind::DuplicateImport {
                name: resolve(local).to_string(),
            },
            span,
        ));
    }
    imports.insert(local, target);
    Ok(())
}

fn insert_type_import(
    types: &mut HashMap<Symbol, TypeRef>,
    local: Symbol,
    target: TypeRef,
    span: Span,
) -> Result<(), KslError> {
    if types.contains_key(&local) {
        return Err(KslError::new(
            ErrorKind::DuplicateImport {
                name: resolve(local).to_string(),
            },
            span,
        ));
    }
    // 可见名的引入位置取导入项的位置, 而非导出方的声明位置.
    types.insert(local, TypeRef { span, ..target });
    Ok(())
}

fn check_local_name(name: Symbol, span: Span) -> Result<(), KslError> {
    if check::is_reserved(name) {
        return Err(KslError::with_subject(
            ErrorKind::ReservedName,
            span,
            resolve(name).to_string(),
        ));
    }
    Ok(())
}

fn local_type_names(
    program: &ksl_frontend::ast::Program,
) -> Result<HashMap<Symbol, Span>, KslError> {
    let mut names = HashMap::new();
    for stmt in program.exprs() {
        let ExprKind::Special(Special::Data { name, variants }) = &stmt.node else {
            continue;
        };
        if names.insert(name.node, name.span).is_some() {
            return Err(KslError::with_subject(
                ErrorKind::DuplicateData,
                name.span,
                resolve(name.node).to_string(),
            ));
        }
        for DataVariant { name: variant, .. } in variants {
            if variant.node == name.node {
                return Err(KslError::new(ErrorKind::VariantNameConflict, variant.span));
            }
        }
    }
    Ok(names)
}

fn local_value_names(program: &ksl_frontend::ast::Program) -> HashMap<Symbol, Span> {
    let mut names = HashMap::new();
    for stmt in program.exprs() {
        match &stmt.node {
            ExprKind::Special(Special::Let { target, .. }) => {
                if let Some(name) = &target.name {
                    names.insert(name.node, name.span);
                }
            },
            ExprKind::Special(Special::Def(def)) => {
                names.insert(def.name.node, def.name.span);
            },
            ExprKind::Special(Special::Fix(defs)) => {
                for def in defs {
                    names.insert(def.name.node, def.name.span);
                }
            },
            _ => {},
        }
    }
    names
}
