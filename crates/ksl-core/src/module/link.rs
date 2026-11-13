//! 模块链接: 建立导入/导出接口, 重写类型名并完成检查.
//!
//! 值保持当前模块的本地名字, 由运行时环境隔离; 类型名重写为完全限定名,
//! 因此 `lib/a/A` 与导入方的本地别名最终指向同一个类型身份.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use super::{ModuleGraph, ModuleId};
use crate::ast::{DataVariant, ExportItem, ExprKind, Special, UseItem};
use crate::check::{self, DataTable, Session};
use crate::error::{ErrorKind, KslError};
use crate::rewrite::{self, TypeNames};
use crate::source::Span;
use crate::symbol::{Symbol, intern, resolve};

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
}

#[derive(Debug, Clone, Default)]
pub struct Exports {
    pub values: IndexMap<Symbol, ValueRef>,
    pub types: IndexMap<Symbol, TypeRef>,
}

#[derive(Debug)]
pub struct LinkedModule {
    pub id: ModuleId,
    pub program: crate::ast::Program,
    /// 本地导入名到真实值来源.
    pub imports: HashMap<Symbol, ValueRef>,
    /// 本地导入名到导入值的类型.
    pub imported_value_types: HashMap<Symbol, crate::check::Type>,
    /// 本地导入名到导入类型的真实名; 本地名只是别名.
    pub imported_type_names: TypeNames,
    pub exports: Exports,
    /// 导出名到值类型.
    pub value_types: HashMap<Symbol, crate::check::Type>,
    /// 本模块源码区间到类型.
    pub types: crate::check::TypeTable,
}

#[derive(Debug)]
pub struct LinkedGraph {
    pub root: ModuleId,
    pub order: Vec<ModuleId>,
    pub modules: HashMap<ModuleId, LinkedModule>,
    pub(crate) data: DataTable,
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
        for (name, target) in &module.exports.types {
            context.add_type_name(
                *name,
                rewrite::TypeName {
                    full: target.full,
                    span: target.span,
                },
            );
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
        }
        context.add_types(module.types.clone());
        context
    }
}

struct Linker {
    interfaces: imbl::HashMap<ModuleId, Exports>,
    value_types: imbl::HashMap<ModuleId, HashMap<Symbol, crate::check::Type>>,
    data: DataTable,
}

/// 链接并检查模块图; 返回按依赖顺序排列的已检查模块.
pub(crate) fn link_and_check(graph: ModuleGraph) -> Result<LinkedGraph, KslError> {
    let cache = super::ModuleCache::new();
    link_and_check_cached(graph, &cache)
}

/// 链接并检查模块图, 复用会话模块缓存中的接口与类型.
pub(crate) fn link_and_check_cached(
    graph: ModuleGraph,
    cache: &super::ModuleCache,
) -> Result<LinkedGraph, KslError> {
    let (root, order, mut units) = graph.into_modules();
    let mut linker = Linker {
        interfaces: cache.interfaces.clone(),
        value_types: cache.value_types.clone(),
        data: cache.data.clone(),
    };
    let mut linked_modules = HashMap::new();
    for id in &order {
        let unit = units.remove(id).ok_or_else(|| {
            KslError::without_span(ErrorKind::ExportUnknown {
                name: id.to_string(),
            })
        })?;
        let module = linker.link_module(&unit)?;
        linked_modules.insert(id.clone(), module);
    }
    Ok(LinkedGraph {
        root,
        order,
        modules: linked_modules,
        data: linker.data,
    })
}

impl Linker {
    // 模块链接步骤对应 `docs/design.md` 第 14 节.
    #[allow(clippy::too_many_lines)]
    fn link_module(&mut self, unit: &super::ModuleUnit) -> Result<LinkedModule, KslError> {
        let type_names = local_type_names(&unit.program)?;
        let mut type_refs = HashMap::new();
        for (name, span) in &type_names {
            check_local_name(*name, *span)?;
            let full = intern(&unit.id.full_name(&resolve(*name)));
            type_refs.insert(
                *name,
                TypeRef {
                    full,
                    module: unit.id.clone(),
                    name: *name,
                    span: *span,
                },
            );
        }

        let mut imports = HashMap::new();
        let mut imported_value_types = HashMap::new();
        let mut imported_type_names = TypeNames::new();
        for import in &unit.uses {
            let interface = self.interfaces.get(&import.target).ok_or_else(|| {
                KslError::with_subject(
                    ErrorKind::ImportNotExported {
                        module: import.target.to_string(),
                        name: import.target.to_string(),
                    },
                    import.span,
                    import.target.to_string(),
                )
            })?;
            for item in &import.items {
                match item {
                    UseItem::Value(name) => {
                        let local = name.node;
                        check_local_name(local, name.span)?;
                        let target =
                            interface.values.get(&local).cloned().ok_or_else(|| {
                                KslError::with_subject(
                                    ErrorKind::ImportNotExported {
                                        module: import.target.to_string(),
                                        name: resolve(local).to_string(),
                                    },
                                    name.span,
                                    resolve(local).to_string(),
                                )
                            })?;
                        insert_value_import(&mut imports, local, target, name.span)?;
                        if let Some(ty) = self
                            .value_types
                            .get(&import.target)
                            .and_then(|types| types.get(&local))
                        {
                            imported_value_types.insert(local, ty.clone());
                        }
                    },
                    UseItem::Rename { from, to } => {
                        check_local_name(to.node, to.span)?;
                        let target =
                            interface.values.get(&from.node).cloned().ok_or_else(|| {
                                KslError::with_subject(
                                    ErrorKind::ImportNotExported {
                                        module: import.target.to_string(),
                                        name: resolve(from.node).to_string(),
                                    },
                                    from.span,
                                    resolve(from.node).to_string(),
                                )
                            })?;
                        insert_value_import(&mut imports, to.node, target, to.span)?;
                        if let Some(ty) = self
                            .value_types
                            .get(&import.target)
                            .and_then(|types| types.get(&from.node))
                        {
                            imported_value_types.insert(to.node, ty.clone());
                        }
                    },
                    UseItem::Type(name) => {
                        let local = name.node;
                        check_local_name(local, name.span)?;
                        let target = interface.types.get(&local).cloned().ok_or_else(|| {
                            KslError::with_subject(
                                ErrorKind::ImportNotExported {
                                    module: import.target.to_string(),
                                    name: resolve(local).to_string(),
                                },
                                name.span,
                                resolve(local).to_string(),
                            )
                        })?;
                        insert_type_import(&mut type_refs, local, target.clone(), name.span)?;
                        imported_type_names.insert(
                            local,
                            rewrite::TypeName {
                                full: target.full,
                                span: name.span,
                            },
                        );
                    },
                    UseItem::TypeRename { from, to } => {
                        check_local_name(to.node, to.span)?;
                        let target =
                            interface.types.get(&from.node).cloned().ok_or_else(|| {
                                KslError::with_subject(
                                    ErrorKind::ImportNotExported {
                                        module: import.target.to_string(),
                                        name: resolve(from.node).to_string(),
                                    },
                                    from.span,
                                    resolve(from.node).to_string(),
                                )
                            })?;
                        insert_type_import(&mut type_refs, to.node, target.clone(), to.span)?;
                        imported_type_names.insert(
                            to.node,
                            rewrite::TypeName {
                                full: target.full,
                                span: to.span,
                            },
                        );
                    },
                }
            }
        }

        // 本地值定义遮蔽同名导入值.
        let mut value_refs = imports.clone();
        for (name, _span) in local_value_names(&unit.program) {
            value_refs.insert(
                name,
                ValueRef {
                    module: unit.id.clone(),
                    name,
                },
            );
        }

        let mut exports = Exports::default();
        for item in &unit.exports {
            match item {
                ExportItem::Value(name) => {
                    let Some(target) = value_refs.get(&name.node).cloned() else {
                        return Err(KslError::with_subject(
                            ErrorKind::ExportUnknown {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    };
                    if exports.values.insert(name.node, target).is_some() {
                        return Err(KslError::with_subject(
                            ErrorKind::DuplicateExport {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    }
                },
                ExportItem::Type(name) => {
                    let Some(target) = type_refs.get(&name.node).cloned() else {
                        return Err(KslError::with_subject(
                            ErrorKind::ExportUnknown {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    };
                    if exports.types.insert(name.node, target).is_some() {
                        return Err(KslError::with_subject(
                            ErrorKind::DuplicateExport {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        ));
                    }
                },
            }
        }

        let type_map: TypeNames = type_refs
            .iter()
            .map(|(local, target)| {
                (
                    *local,
                    rewrite::TypeName {
                        full: target.full,
                        span: target.span,
                    },
                )
            })
            .collect();
        let mut program = unit.program.clone();
        normalize_main(&mut program, &unit.id)?;
        rewrite_program(&mut program, &type_map);

        check::collect_data(&program, &mut self.data)?;
        let mut session = Session::for_module(self.data.clone(), imported_value_types.clone());
        session.check_loaded(&program)?;

        let mut value_types = HashMap::new();
        for (name, target) in &exports.values {
            let ty = if target.module == unit.id {
                session.value_type(target.name)
            } else {
                imported_value_types.get(name).cloned()
            };
            let ty = ty.ok_or_else(|| {
                KslError::without_span(ErrorKind::ExportUnknown {
                    name: resolve(*name).to_string(),
                })
            })?;
            value_types.insert(*name, ty);
        }

        let visible = visible_types(&exports);
        for item in &unit.exports {
            match item {
                ExportItem::Value(name) => {
                    let ty = value_types.get(&name.node).ok_or_else(|| {
                        KslError::without_span(ErrorKind::ExportUnknown {
                            name: resolve(name.node).to_string(),
                        })
                    })?;
                    check_export_type(ty, &visible, name.span)?;
                },
                ExportItem::Type(name) => {
                    let target = exports.types.get(&name.node).ok_or_else(|| {
                        KslError::with_subject(
                            ErrorKind::ExportUnknown {
                                name: resolve(name.node).to_string(),
                            },
                            name.span,
                            resolve(name.node).to_string(),
                        )
                    })?;
                    if let Some(info) = self.data.types.get(&target.full) {
                        for fields in info.variants.values() {
                            for field in fields {
                                check_export_type(field, &visible, name.span)?;
                            }
                        }
                    }
                },
            }
        }

        let mut types = session.types().clone();
        for import in &unit.uses {
            for item in &import.items {
                match item {
                    UseItem::Value(name) => {
                        if let Some(ty) = imported_value_types.get(&name.node) {
                            types.insert(name.key(), ty.clone());
                        }
                    },
                    UseItem::Rename { to, .. } => {
                        if let Some(ty) = imported_value_types.get(&to.node) {
                            types.insert(to.key(), ty.clone());
                        }
                    },
                    UseItem::Type(name) => {
                        if let Some(target) = type_refs.get(&name.node) {
                            types.insert(name.key(), crate::check::Type::data(target.full));
                        }
                    },
                    UseItem::TypeRename { to, .. } => {
                        if let Some(target) = type_refs.get(&to.node) {
                            types.insert(to.key(), crate::check::Type::data(target.full));
                        }
                    },
                }
            }
        }
        for item in &unit.exports {
            match item {
                ExportItem::Value(name) => {
                    if let Some(ty) = value_types.get(&name.node) {
                        types.insert(name.key(), ty.clone());
                    }
                },
                ExportItem::Type(name) => {
                    if let Some(target) = exports.types.get(&name.node) {
                        types.insert(name.key(), crate::check::Type::data(target.full));
                    }
                },
            }
        }

        self.interfaces.insert(unit.id.clone(), exports.clone());
        self.value_types.insert(unit.id.clone(), value_types.clone());
        Ok(LinkedModule {
            id: unit.id.clone(),
            program,
            imports,
            imported_value_types,
            imported_type_names,
            exports,
            value_types,
            types,
        })
    }
}

fn visible_types(exports: &Exports) -> HashSet<Symbol> {
    let mut visible: HashSet<Symbol> =
        exports.types.values().map(|target| target.full).collect();
    // 内置 Data 类型始终可见, 否则导出时间与 I/O 相关签名会被误判为私有类型.
    visible.extend(check::builtin_data_type_names().iter().map(|name| intern(name)));
    visible
}

/// 导出签名中的 Data 类型必须已导出或为内置类型; 不透明类型均为内置.
fn check_export_type(
    ty: &crate::check::Type,
    visible: &HashSet<Symbol>,
    span: Span,
) -> Result<(), KslError> {
    use crate::check::Type;
    match ty {
        Type::App { name, args } if args.is_empty() => {
            if visible.contains(name) {
                Ok(())
            } else {
                Err(KslError::with_subject(
                    ErrorKind::ExportPrivateType {
                        name: resolve(*name).to_string(),
                    },
                    span,
                    resolve(*name).to_string(),
                ))
            }
        },
        Type::App { args, .. } => {
            for arg in args.iter() {
                check_export_type(arg, visible, span)?;
            }
            Ok(())
        },
        Type::Fn(params, ret) => {
            for param in params {
                check_export_type(param, visible, span)?;
            }
            check_export_type(ret, visible, span)
        },
        Type::I64
        | Type::U8
        | Type::U64
        | Type::F64
        | Type::Char
        | Type::Bool
        | Type::Str
        | Type::Nil
        | Type::Never
        | Type::Opaque(_) => Ok(()),
    }
}

fn insert_value_import(
    imports: &mut HashMap<Symbol, ValueRef>,
    local: Symbol,
    target: ValueRef,
    span: Span,
) -> Result<(), KslError> {
    if imports.contains_key(&local) {
        return Err(KslError::with_subject(
            ErrorKind::DuplicateImport {
                name: resolve(local).to_string(),
            },
            span,
            resolve(local).to_string(),
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
        return Err(KslError::with_subject(
            ErrorKind::DuplicateImport {
                name: resolve(local).to_string(),
            },
            span,
            resolve(local).to_string(),
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

fn local_type_names(program: &crate::ast::Program) -> Result<HashMap<Symbol, Span>, KslError> {
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

fn local_value_names(program: &crate::ast::Program) -> HashMap<Symbol, Span> {
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

/// 改写模块程序: 数据类型声明名换为完全限定名, 类型引用按改写表替换.
///
/// 导入与导出声明不是可执行语句, 改写前先行剔除.
/// 把 `@Main` 从顶层语句中取出并放到程序末尾.
///
/// `@Main` 的逻辑位置在全部顶层语句之后, 物理位置不影响语义; 依赖模块
/// 的 `@Main` 同样保留在末尾参与检查, 但执行结果只由根模块使用.
fn normalize_main(
    program: &mut crate::ast::Program,
    module: &crate::module::ModuleId,
) -> Result<(), KslError> {
    use crate::ast::{ExprKind, Special};

    let mut main = None;
    let mut stmts = Vec::with_capacity(program.stmts.len());
    for stmt in std::mem::take(&mut program.stmts) {
        if matches!(&stmt.node, ExprKind::Special(Special::Main { .. })) {
            if main.replace(stmt).is_some() {
                return Err(KslError::with_subject(
                    ErrorKind::MultipleMainStatements {
                        module: module.to_string(),
                    },
                    main.as_ref().expect("刚写入").span,
                    module.to_string(),
                ));
            }
        } else {
            stmts.push(stmt);
        }
    }
    program.stmts = stmts;
    if let Some(tail) = program.tail.take() {
        if matches!(&tail.node, ExprKind::Special(Special::Main { .. })) {
            if main.replace(tail).is_some() {
                return Err(KslError::with_subject(
                    ErrorKind::MultipleMainStatements {
                        module: module.to_string(),
                    },
                    main.as_ref().expect("刚写入").span,
                    module.to_string(),
                ));
            }
        } else {
            program.tail = Some(tail);
        }
    }
    if let Some(main) = main {
        if let Some(tail) = program.tail.take() {
            program.stmts.push(tail);
        }
        program.tail = Some(main);
    }
    Ok(())
}

fn rewrite_program(program: &mut crate::ast::Program, types: &TypeNames) {
    program.stmts.retain_mut(|stmt| {
        if matches!(
            stmt.node,
            ExprKind::Special(Special::Export { .. } | Special::Use { .. })
        ) {
            return false;
        }
        rewrite_data_names(stmt, types);
        true
    });
    if program.tail.as_ref().is_some_and(|expr| {
        matches!(
            &expr.node,
            ExprKind::Special(Special::Export { .. } | Special::Use { .. })
        )
    }) {
        program.tail = None;
    } else if let Some(tail) = &mut program.tail {
        rewrite_data_names(tail, types);
    }
    rewrite::rewrite_type_refs(program, types);
}

/// 改写数据类型声明名: 本地名换成完全限定名.
///
/// 声明名只在模块内部可见, 故与通用类型引用改写分开处理.
fn rewrite_data_names(stmt: &mut crate::ast::Expr, types: &TypeNames) {
    let ExprKind::Special(Special::Data { name, .. }) = &mut stmt.node else {
        return;
    };
    if let Some(alias) = types.get(&name.node) {
        name.node = alias.full;
    }
}
