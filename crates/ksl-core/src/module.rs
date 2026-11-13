//! 模块加载: 文件即模块, 从主文件建立依赖图.
//!
//! 路径相对当前模块文件所在目录, 统一追加 `.ksl`; 主文件所在目录为根,
//! `.` 与 `..` 按词法规范化且不得越出根目录. 模块顶层只在此处收集
//! `@Use` / `@Export`; 链接, 检查与执行在加载后消费该图.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use crate::ast::{ExportItem, Program, Special, UseItem};
use crate::check::{DataTable, Type, TypeTable};
use crate::error::{ErrorKind, KslError};
use crate::parser;
use crate::source::{Sources, Span};
use crate::symbol::Symbol;
use crate::value::Value;

mod exec;
pub(crate) mod link;

pub use exec::{Execution, RootEnv, execute, execute_with, execute_with_cache};
pub use link::LinkedGraph;

/// 规范化后的模块标识: 根相对路径, 不含 `.ksl`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(String);

impl ModuleId {
    /// 模块路径文本, 不含扩展名.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 模块内实体的完整名称, 如 `lib/a/A`.
    #[must_use]
    pub fn full_name(&self, name: &str) -> String {
        format!("{}/{name}", self.0)
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// REPL 会话的模块缓存.
///
/// 会话内每个模块只检查与执行一次; 新模块可以依赖已缓存模块并复用其导出.
#[derive(Debug, Clone, Default)]
pub struct ModuleCache {
    pub(crate) interfaces: imbl::HashMap<ModuleId, link::Exports>,
    pub(crate) value_types: imbl::HashMap<ModuleId, HashMap<Symbol, Type>>,
    pub(crate) types: imbl::HashMap<ModuleId, TypeTable>,
    pub(crate) data: DataTable,
    pub(crate) values: imbl::HashMap<ModuleId, HashMap<Symbol, Value>>,
}

impl ModuleCache {
    /// 空缓存.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 缓存导出的运行时值; 环回收作为外部根传入.
    #[must_use]
    pub fn root_values(&self) -> Vec<Value> {
        value_roots(&self.values)
    }

    /// 已加载模块标识.
    #[must_use]
    pub(crate) fn loaded_ids(&self) -> HashSet<ModuleId> {
        self.interfaces.keys().cloned().collect()
    }

    /// 记录本次链接产生的模块接口与类型表.
    pub(crate) fn update_graph(&mut self, graph: &link::LinkedGraph) {
        for (id, module) in &graph.modules {
            if *id == graph.root {
                continue;
            }
            self.interfaces.insert(id.clone(), module.exports.clone());
            self.value_types.insert(id.clone(), module.value_types.clone());
            self.types.insert(id.clone(), module.types.clone());
        }
        self.data = graph.data.clone();
    }

    /// 记录本次执行产生的模块导出值.
    pub(crate) fn update_values(
        &mut self,
        values: imbl::HashMap<ModuleId, HashMap<Symbol, Value>>,
    ) {
        self.values = values;
    }
}

/// 模块导出值展平为根列表.
#[must_use]
pub(crate) fn value_roots(
    values: &imbl::HashMap<ModuleId, HashMap<Symbol, Value>>,
) -> Vec<Value> {
    values
        .values()
        .flat_map(|exports| exports.values().cloned())
        .collect()
}

/// 一条 `@Use` 声明.
#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub target: ModuleId,
    pub module_path: String,
    pub items: Vec<UseItem>,
    pub span: Span,
}

/// 已加载的模块.
#[derive(Debug)]
pub struct ModuleUnit {
    pub id: ModuleId,
    pub path: PathBuf,
    pub program: Program,
    pub uses: Vec<ImportDecl>,
    pub exports: Vec<ExportItem>,
}

/// 模块顶层的声明: 导入, 导出与依赖文件.
struct ModuleDecls {
    uses: Vec<ImportDecl>,
    exports: Vec<ExportItem>,
    /// 依赖文件路径与引用位置.
    deps: Vec<(PathBuf, Span)>,
}

/// 一次根加载得到的模块依赖图.
#[derive(Debug)]
pub struct ModuleGraph {
    root: ModuleId,
    order: Vec<ModuleId>,
    modules: HashMap<ModuleId, ModuleUnit>,
}

impl ModuleGraph {
    #[must_use]
    pub fn root(&self) -> &ModuleId {
        &self.root
    }

    /// 依赖优先的模块顺序; 主模块在最后.
    #[must_use]
    pub fn order(&self) -> &[ModuleId] {
        &self.order
    }

    #[must_use]
    pub fn get(&self, id: &ModuleId) -> Option<&ModuleUnit> {
        self.modules.get(id)
    }

    #[must_use]
    pub fn into_modules(self) -> (ModuleId, Vec<ModuleId>, HashMap<ModuleId, ModuleUnit>) {
        (self.root, self.order, self.modules)
    }
}

/// 从主文件读取源码并加载整个依赖图.
///
/// # Errors
///
/// 文件读取, 路径越界, 重复导入, 循环依赖或解析失败.
pub fn load_file(root_file: &Path, sources: &mut Sources) -> Result<ModuleGraph, KslError> {
    load_file_with_cache(root_file, sources, &HashSet::new())
}

fn load_file_with_cache(
    root_file: &Path,
    sources: &mut Sources,
    cached: &HashSet<ModuleId>,
) -> Result<ModuleGraph, KslError> {
    let text = std::fs::read_to_string(root_file).map_err(|error| KslError {
        kind: ErrorKind::ModuleReadFailed {
            path: root_file.display().to_string(),
            message: error.to_string(),
        },
        subject: None,
        span: None,
    })?;
    load_with_cache(root_file, &text, sources, cached)
}

/// 加载主文件并完成模块链接与检查.
///
/// # Errors
///
/// 加载, 链接或类型检查失败.
pub fn load_and_check_file(
    root_file: &Path,
    sources: &mut Sources,
) -> Result<link::LinkedGraph, KslError> {
    let mut cache = ModuleCache::new();
    load_and_check_file_with_cache(root_file, sources, &mut cache)
}

/// 加载主文件并完成模块链接与检查, 复用会话模块缓存.
///
/// 成功后把本次模块的接口与类型写入缓存; 调用方在整体失败时丢弃缓存副本.
///
/// # Errors
///
/// 加载, 链接或类型检查失败.
pub fn load_and_check_file_with_cache(
    root_file: &Path,
    sources: &mut Sources,
    cache: &mut ModuleCache,
) -> Result<link::LinkedGraph, KslError> {
    let graph = load_file_with_cache(root_file, sources, &cache.loaded_ids())?;
    let linked = link::link_and_check_cached(graph, cache)?;
    cache.update_graph(&linked);
    Ok(linked)
}

/// 以内存中的程序为根模块加载并检查依赖图.
///
/// 供交互输入的行内导入使用; 根模块的导入值与类型别名由
/// [`link::LinkedGraph::import_context`] 给出.
///
/// # Errors
///
/// 加载, 链接或类型检查失败.
pub fn load_and_check_virtual(
    root_dir: &Path,
    name: &str,
    program: Program,
    sources: &mut Sources,
) -> Result<link::LinkedGraph, KslError> {
    let mut cache = ModuleCache::new();
    load_and_check_virtual_with_cache(root_dir, name, program, sources, &mut cache)
}

/// 以内存程序为根加载并检查依赖图, 复用会话模块缓存.
///
/// 根模块直接 `@Use` 已缓存模块时报错; 新模块依赖已缓存模块时复用缓存.
///
/// # Errors
///
/// 加载, 链接或类型检查失败.
pub fn load_and_check_virtual_with_cache(
    root_dir: &Path,
    name: &str,
    program: Program,
    sources: &mut Sources,
    cache: &mut ModuleCache,
) -> Result<link::LinkedGraph, KslError> {
    let graph = load_virtual_with_cache(root_dir, name, program, sources, &cache.loaded_ids())?;
    let linked = link::link_and_check_cached(graph, cache)?;
    cache.update_graph(&linked);
    Ok(linked)
}

/// 用已读取的主文件源码加载并检查整个依赖图.
///
/// # Errors
///
/// 加载, 链接或类型检查失败.
pub fn load_and_check(
    root_file: &Path,
    root_text: &str,
    sources: &mut Sources,
) -> Result<link::LinkedGraph, KslError> {
    link::link_and_check(load(root_file, root_text, sources)?)
}

/// 用已读取的主文件源码加载整个依赖图.
///
/// # Errors
///
/// 路径越界, 重复导入, 循环依赖或解析失败.
pub fn load(
    root_file: &Path,
    root_text: &str,
    sources: &mut Sources,
) -> Result<ModuleGraph, KslError> {
    load_with_cache(root_file, root_text, sources, &HashSet::new())
}

fn load_with_cache(
    root_file: &Path,
    root_text: &str,
    sources: &mut Sources,
    cached: &HashSet<ModuleId>,
) -> Result<ModuleGraph, KslError> {
    let root_file = absolute_path(root_file)?;
    let root_file = std::fs::canonicalize(&root_file).unwrap_or(root_file);
    let root = root_dir(&root_file);
    let mut loader = Loader::new(root, sources, cached.clone());
    let root_id = loader.load_root(&root_file, root_text)?;
    Ok(loader.into_graph(root_id))
}

/// 以内存中的程序为根模块加载依赖图.
///
/// 根模块不落盘: 其 `@Use` 相对 `root_dir` 解析, 模块身份取 `name`.
/// 供交互输入的行内导入使用.
///
/// # Errors
///
/// 路径越界, 重复导入, 循环依赖或解析失败.
pub fn load_virtual(
    root_dir: &Path,
    name: &str,
    program: Program,
    sources: &mut Sources,
) -> Result<ModuleGraph, KslError> {
    load_virtual_with_cache(root_dir, name, program, sources, &HashSet::new())
}

fn load_virtual_with_cache(
    root_dir: &Path,
    name: &str,
    program: Program,
    sources: &mut Sources,
    cached: &HashSet<ModuleId>,
) -> Result<ModuleGraph, KslError> {
    let root = std::fs::canonicalize(root_dir).unwrap_or_else(|_| root_dir.to_path_buf());
    let mut loader = Loader::new(root, sources, cached.clone());
    let id = ModuleId(name.to_string());
    let file = loader.root.join(name);
    let decls = loader.collect_decls(&file, &id, &program)?;
    for (target, span) in &decls.deps {
        loader.load_module(target, Some(*span), true)?;
    }
    loader.register(&id, file, program, decls);
    Ok(loader.into_graph(id))
}

fn root_dir(root_file: &Path) -> PathBuf {
    let root = root_file
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    std::fs::canonicalize(&root).unwrap_or(root)
}

/// 主文件所在目录, 即模块系统的根目录.
///
/// 交互模式的行内导入以同一目录为根, 故对宿主公开.
#[must_use]
pub fn root_dir_of(root_file: &Path) -> PathBuf {
    let absolute = absolute_path(root_file).unwrap_or_else(|_| root_file.to_path_buf());
    let absolute = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    root_dir(&absolute)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Loading,
    Loaded,
}

struct Loader<'a> {
    root: PathBuf,
    sources: &'a mut Sources,
    modules: HashMap<ModuleId, ModuleUnit>,
    states: HashMap<PathBuf, State>,
    order: Vec<ModuleId>,
    stack: Vec<PathBuf>,
    cached: HashSet<ModuleId>,
}

impl<'a> Loader<'a> {
    fn new(root: PathBuf, sources: &'a mut Sources, cached: HashSet<ModuleId>) -> Self {
        Self {
            root,
            sources,
            modules: HashMap::new(),
            states: HashMap::new(),
            order: Vec::new(),
            stack: Vec::new(),
            cached,
        }
    }

    fn into_graph(self, root: ModuleId) -> ModuleGraph {
        ModuleGraph {
            root,
            order: self.order,
            modules: self.modules,
        }
    }

    /// 加载主文件: 源码由调用方给出, 免去重复读盘.
    fn load_root(&mut self, file: &Path, text: &str) -> Result<ModuleId, KslError> {
        let file = self.normalize_file(file, None)?;
        let id = self.module_id(&file);
        if self.cached.contains(&id) {
            return Err(KslError::without_span(ErrorKind::ModuleAlreadyLoaded {
                path: id.to_string(),
            }));
        }
        self.states.insert(file.clone(), State::Loading);
        self.stack.push(file.clone());
        let source = self.sources.add(self.source_name(&file), text.to_string());
        let program = parser::parse_text(self.sources.text(source), source)?;
        let decls = self.collect_decls(&file, &id, &program)?;
        for (target, span) in &decls.deps {
            self.load_module(target, Some(*span), true)?;
        }
        self.stack.pop();
        self.states.insert(file.clone(), State::Loaded);
        self.register(&id, file, program, decls);
        Ok(id)
    }

    /// 加载磁盘上的模块文件.
    fn load_module(
        &mut self,
        file: &Path,
        use_span: Option<Span>,
        direct: bool,
    ) -> Result<ModuleId, KslError> {
        let file = self.normalize_file(file, use_span)?;
        let id = self.module_id(&file);
        if self.cached.contains(&id) {
            if direct {
                return Err(KslError::new(
                    ErrorKind::ModuleAlreadyLoaded {
                        path: id.to_string(),
                    },
                    use_span.expect("直接导入必有位置"),
                ));
            }
            return Ok(id);
        }
        if let Some(state) = self.states.get(&file) {
            if *state == State::Loaded {
                return Ok(id);
            }
            return Err(self.cycle_error(&file, use_span));
        }
        self.states.insert(file.clone(), State::Loading);
        self.stack.push(file.clone());

        let text = std::fs::read_to_string(&file).map_err(|error| KslError {
            kind: ErrorKind::ModuleReadFailed {
                path: self.source_name(&file),
                message: error.to_string(),
            },
            subject: None,
            span: use_span,
        })?;
        let source = self.sources.add(self.source_name(&file), text);
        let program = parser::parse_text(self.sources.text(source), source)?;

        let decls = self.collect_decls(&file, &id, &program)?;
        // 依赖在当前模块仍处于加载中时加载, 环检测因此得以成立.
        for (target, span) in &decls.deps {
            self.load_module(target, Some(*span), false)?;
        }
        self.stack.pop();
        self.states.insert(file.clone(), State::Loaded);
        self.register(&id, file, program, decls);
        Ok(id)
    }

    /// 收集模块的导入导出声明, 并给出依赖文件的路径.
    ///
    /// 同一模块只允许一个导出声明; 重复导入按规范化后的目标判重.
    fn collect_decls(
        &self,
        file: &Path,
        id: &ModuleId,
        program: &Program,
    ) -> Result<ModuleDecls, KslError> {
        let mut uses = Vec::new();
        let mut deps = Vec::new();
        let mut exports = Vec::new();
        let mut export_statements = 0;
        let mut seen = HashMap::new();
        for stmt in program.exprs() {
            match &stmt.node {
                crate::ast::ExprKind::Special(Special::Use { items, path }) => {
                    // 空条目 @Use 是 noop, 不加载依赖.
                    if items.is_empty() {
                        continue;
                    }
                    let target_file = self.resolve_import(file, path, stmt.span)?;
                    let target = self.module_id(&target_file);
                    if seen.insert(target.clone(), stmt.span).is_some() {
                        return Err(KslError::with_subject(
                            ErrorKind::DuplicateUse {
                                path: target.as_str().to_string(),
                            },
                            stmt.span,
                            target.as_str().to_string(),
                        ));
                    }
                    deps.push((target_file, stmt.span));
                    uses.push(ImportDecl {
                        target,
                        module_path: path.clone(),
                        items: items.clone(),
                        span: stmt.span,
                    });
                },
                crate::ast::ExprKind::Special(Special::Export { items }) => {
                    export_statements += 1;
                    if export_statements > 1 {
                        return Err(KslError::with_subject(
                            ErrorKind::MultipleExportStatements {
                                module: id.to_string(),
                            },
                            stmt.span,
                            id.to_string(),
                        ));
                    }
                    exports.extend(items.iter().cloned());
                },
                _ => {},
            }
        }
        Ok(ModuleDecls {
            uses,
            exports,
            deps,
        })
    }

    /// 登记模块单元; 调用方负责先加载其依赖.
    fn register(&mut self, id: &ModuleId, file: PathBuf, program: Program, decls: ModuleDecls) {
        self.order.push(id.clone());
        self.modules.insert(
            id.clone(),
            ModuleUnit {
                id: id.clone(),
                path: file,
                program,
                uses: decls.uses,
                exports: decls.exports,
            },
        );
    }

    fn resolve_import(
        &self,
        current_file: &Path,
        module_path: &str,
        span: Span,
    ) -> Result<PathBuf, KslError> {
        if module_path.is_empty() {
            return Err(KslError::with_subject(
                ErrorKind::InvalidModulePath {
                    path: module_path.to_string(),
                },
                span,
                module_path.to_string(),
            ));
        }
        let relative_text = format!("{module_path}.ksl");
        let relative = Path::new(&relative_text);
        if relative.is_absolute() {
            return Err(KslError::with_subject(
                ErrorKind::InvalidModulePath {
                    path: module_path.to_string(),
                },
                span,
                module_path.to_string(),
            ));
        }
        let mut path = current_file
            .parent()
            .map_or_else(|| self.root.clone(), Path::to_path_buf);
        for component in relative.components() {
            match component {
                Component::CurDir => {},
                Component::ParentDir => {
                    if path == self.root || !path.pop() {
                        return Err(KslError::with_subject(
                            ErrorKind::ModulePathEscapesRoot {
                                path: module_path.to_string(),
                            },
                            span,
                            module_path.to_string(),
                        ));
                    }
                },
                Component::Normal(part) => path.push(part),
                Component::RootDir | Component::Prefix(_) => {
                    return Err(KslError::with_subject(
                        ErrorKind::InvalidModulePath {
                            path: module_path.to_string(),
                        },
                        span,
                        module_path.to_string(),
                    ));
                },
            }
        }
        if !path.starts_with(&self.root) {
            return Err(KslError::with_subject(
                ErrorKind::ModulePathEscapesRoot {
                    path: module_path.to_string(),
                },
                span,
                module_path.to_string(),
            ));
        }
        Ok(path)
    }

    fn normalize_file(&self, file: &Path, span: Option<Span>) -> Result<PathBuf, KslError> {
        let canonical = std::fs::canonicalize(file).map_err(|error| KslError {
            kind: ErrorKind::ModuleReadFailed {
                path: file.display().to_string(),
                message: error.to_string(),
            },
            subject: None,
            span,
        })?;
        if !canonical.starts_with(&self.root) {
            return Err(KslError {
                kind: ErrorKind::ModulePathEscapesRoot {
                    path: file.display().to_string(),
                },
                subject: None,
                span,
            });
        }
        Ok(canonical)
    }

    fn module_id(&self, file: &Path) -> ModuleId {
        let relative = file.trim_prefix(&self.root);
        let mut relative = relative.to_path_buf();
        relative.set_extension("");
        let text = relative.to_string_lossy().replace('\\', "/");
        ModuleId(text)
    }

    fn source_name(&self, file: &Path) -> String {
        file.trim_prefix(&self.root)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn cycle_error(&self, file: &Path, span: Option<Span>) -> KslError {
        let start = self.stack.iter().position(|path| path == file).unwrap_or(0);
        let mut chain: Vec<String> = self.stack[start..]
            .iter()
            .map(|path| self.source_name(path))
            .collect();
        chain.push(self.source_name(file));
        KslError {
            kind: ErrorKind::CyclicImport { chain },
            subject: None,
            span,
        }
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf, KslError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let current = std::env::current_dir().map_err(|error| KslError {
        kind: ErrorKind::ModuleReadFailed {
            path: path.display().to_string(),
            message: error.to_string(),
        },
        subject: None,
        span: None,
    })?;
    Ok(current.join(path))
}

#[cfg(test)]
mod tests;
