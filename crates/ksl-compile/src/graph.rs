//! 模块加载: 文件即模块, 从主文件建立依赖图.
//!
//! 路径相对当前模块文件所在目录, 统一追加 `.ksl`.
//! 主文件所在目录为根, `.` 与 `..` 按词法规范化且不得越出根目录.
//! `@Use` 与 `@Export` 只在加载阶段从模块顶层收集.
//! 链接与检查在加载后消费该图.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

use arcstr::ArcStr;
use ksl_base::source::{Sources, Span};
use ksl_diagnostics::error::{ErrorKind, KslError};
use ksl_frontend::ast::{ExportItem, Expr, LinkDecl, Program, Special, UseItem};
use ksl_frontend::parser;
use ksl_frontend::validate::{self, SourceKind};

use crate::cache::CompileCache;
use crate::link;

/// 规范化后的模块标识: 根相对路径, 不含 `.ksl`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleId(ArcStr);

impl ModuleId {
    /// 由已规范化的根相对路径构造, 不含扩展名.
    #[must_use]
    pub fn new(name: impl Into<ArcStr>) -> Self {
        Self(name.into())
    }

    /// 模块路径文本, 不含扩展名.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// 模块内实体的完整名称, 如 `lib/a/A`.
    #[must_use]
    pub fn full_name(&self, name: &str) -> String {
        format!("{}/{name}", self.0.as_str())
    }
}

impl fmt::Display for ModuleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// 编译根来源.
///
/// 只在一次编译调用内使用.
#[derive(Debug)]
pub enum RootSource<'a> {
    /// 磁盘上的主文件; 根取主文件所在目录.
    File(&'a Path),
    /// 磁盘上的主文件; 根由调用方指定.
    FileAt { root: &'a Path, file: &'a Path },
    /// 已读取的主文件源码.
    Text { path: &'a Path, text: &'a str },
    /// 已解析的虚拟根程序; 根不落盘, `@Use` 相对 `dir` 解析, 模块身份取 `name`.
    Virtual {
        dir: &'a Path,
        name: &'a str,
        program: Program,
        cached_imports: CachedImports,
    },
}

/// 虚拟根的直接导入是否允许命中已缓存模块.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedImports {
    /// 直接导入已缓存模块时报重复加载; 交互输入使用.
    Reject,
    /// 允许直接导入已缓存模块; 编辑器重新检查使用.
    Allow,
}

/// 根相对的路径文本, 不含扩展名; 模块与插件共用.
pub(crate) fn relative_id(root: &Path, file: &Path) -> String {
    let mut relative = file.trim_prefix(root).to_path_buf();
    relative.set_extension("");
    relative.to_string_lossy().replace('\\', "/")
}

/// 根相对的模块名, 不含扩展名.
///
/// 路径先规范化, 调用方无需预先处理符号链接.
#[must_use]
pub fn module_name(root: &Path, file: &Path) -> String {
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let file = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    relative_id(&root, &file)
}

/// 一条 `@Use` 声明.
#[derive(Debug, Clone)]
pub struct ImportDecl {
    pub target: ModuleId,
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
    pub links: Vec<LinkDecl>,
}

/// 模块顶层的声明: 导入, 导出与依赖文件.
struct ModuleDecls {
    uses: Vec<ImportDecl>,
    exports: Vec<ExportItem>,
    links: Vec<LinkDecl>,
    /// 依赖文件路径与引用位置.
    deps: Vec<(PathBuf, Span)>,
}

/// 一次根加载得到的模块依赖图.
#[derive(Debug)]
pub struct ModuleGraph {
    root: ModuleId,
    /// 模块根目录; 插件路径与类型身份以此为基准.
    root_dir: PathBuf,
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
    pub fn into_modules(
        self,
    ) -> (ModuleId, PathBuf, Vec<ModuleId>, HashMap<ModuleId, ModuleUnit>) {
        (self.root, self.root_dir, self.order, self.modules)
    }
}

/// 加载根模块与依赖图.
///
/// 根来源可以是磁盘文件, 已读取的源码文本或已解析的虚拟程序.
///
/// # Errors
///
/// 文件读取, 路径越界, 重复导入, 循环依赖或解析失败.
pub fn load(
    root: RootSource<'_>,
    sources: &mut Sources,
    cache: &CompileCache,
) -> Result<ModuleGraph, KslError> {
    let cached = cache.loaded_ids();
    match root {
        RootSource::File(path) => load_file_with_cache(None, path, sources, &cached),
        RootSource::FileAt { root, file } => {
            load_file_with_cache(Some(root), file, sources, &cached)
        },
        RootSource::Text { path, text } => load_with_cache(None, path, text, sources, &cached),
        RootSource::Virtual {
            dir,
            name,
            program,
            cached_imports,
        } => load_virtual_with_cache(
            dir,
            name,
            program,
            sources,
            &cached,
            cached_imports == CachedImports::Reject,
        ),
    }
}

fn load_file_with_cache(
    root: Option<&Path>,
    root_file: &Path,
    sources: &mut Sources,
    cached: &HashSet<ModuleId>,
) -> Result<ModuleGraph, KslError> {
    let text = std::fs::read_to_string(root_file).map_err(|error| {
        KslError::without_span(ErrorKind::ModuleReadFailed {
            path: root_file.display().to_string(),
            message: error.to_string(),
        })
    })?;
    load_with_cache(root, root_file, &text, sources, cached)
}

/// 加载根模块并完成链接与检查, 复用会话编译缓存.
///
/// 成功后写入本次模块的接口与类型; 整体失败时调用方丢弃缓存副本.
///
/// # Errors
///
/// 加载, 链接或类型检查失败.
pub fn compile(
    root: RootSource<'_>,
    sources: &mut Sources,
    cache: &mut CompileCache,
) -> Result<link::CheckedGraph, KslError> {
    cache.refresh();
    let graph = load(root, sources, cache)?;
    let linked = link::link_and_check_cached(graph, cache)?;
    cache.update_graph(&linked);
    Ok(linked)
}

/// 把一段程序的 `@Main` 规范到顶层语句之后.
///
/// 模块加载与脚本测试共用此函数.
///
/// # Errors
///
/// 同一模块出现多个 `@Main` 时报错.
pub fn normalize_main(program: &mut Program, module: &str) -> Result<(), KslError> {
    link::normalize_main(program, module)
}

/// 取走程序尾部的 `@Main` 声明, 返回入口声明节点.
///
/// 与模块链接共用同一套归一化规则.
#[must_use]
pub fn take_main(program: &mut Program) -> Option<Expr> {
    link::take_main(program)
}

fn load_with_cache(
    root: Option<&Path>,
    root_file: &Path,
    root_text: &str,
    sources: &mut Sources,
    cached: &HashSet<ModuleId>,
) -> Result<ModuleGraph, KslError> {
    let root_file = absolute_path(root_file)?;
    let root_file = std::fs::canonicalize(&root_file).unwrap_or(root_file);
    let root = root.map_or_else(
        || root_dir(&root_file),
        |root| std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
    );
    let mut loader = Loader::new(root, sources, cached.clone());
    let root_id = loader.load_root(&root_file, root_text)?;
    Ok(loader.into_graph(root_id))
}

fn load_virtual_with_cache(
    root_dir: &Path,
    name: &str,
    program: Program,
    sources: &mut Sources,
    cached: &HashSet<ModuleId>,
    reject_cached_imports: bool,
) -> Result<ModuleGraph, KslError> {
    let root = std::fs::canonicalize(root_dir).unwrap_or_else(|_| root_dir.to_path_buf());
    let mut loader = Loader::new(root, sources, cached.clone());
    let id = ModuleId::new(name);
    let file = loader.root.join(name);
    let decls = loader.collect_decls(&file, &id, &program)?;
    for (target, span) in &decls.deps {
        loader.load_module(target, Some(*span), reject_cached_imports)?;
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

/// 加载中的模块; 依赖按下标逐个推进.
struct Pending {
    id: ModuleId,
    file: PathBuf,
    program: Program,
    decls: ModuleDecls,
    /// 下一个待加载依赖的下标.
    next: usize,
    /// 直接依赖命中缓存时是否报重复加载.
    deps_reject_cached: bool,
}

struct Loader<'a> {
    root: PathBuf,
    sources: &'a mut Sources,
    modules: HashMap<ModuleId, ModuleUnit>,
    states: HashMap<PathBuf, State>,
    order: Vec<ModuleId>,
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
            cached,
        }
    }

    fn into_graph(self, root: ModuleId) -> ModuleGraph {
        ModuleGraph {
            root,
            root_dir: self.root,
            order: self.order,
            modules: self.modules,
        }
    }

    /// 加载主文件: 源码由调用方给出, 免去重复读盘.
    fn load_root(&mut self, file: &Path, text: &str) -> Result<ModuleId, KslError> {
        let file = self.normalize_file(file, None)?;
        let id = self.module_id(&file);
        if self.cached.contains(&id) {
            let path = id.to_string();
            return Err(KslError::module_already_loaded(&path, None));
        }
        self.states.insert(file.clone(), State::Loading);
        let source = self.sources.add(self.source_name(&file), text.to_string());
        let program = parser::parse_text(self.sources.text(source), source)?;
        validate::validate(&program, SourceKind::Module)?;
        let decls = self.collect_decls(&file, &id, &program)?;
        let mut pending = vec![Pending {
            id: id.clone(),
            file,
            program,
            decls,
            next: 0,
            deps_reject_cached: true,
        }];
        self.drain(&mut pending)?;
        Ok(id)
    }

    /// 加载磁盘上的模块文件.
    fn load_module(
        &mut self,
        file: &Path,
        use_span: Option<Span>,
        reject_cached: bool,
    ) -> Result<ModuleId, KslError> {
        let mut pending = Vec::new();
        let id = self.enter_module(file, use_span, reject_cached, &mut pending)?;
        self.drain(&mut pending)?;
        Ok(id)
    }

    /// 读入并解析一个模块, 压入待完成帧; 已加载的模块直接返回.
    fn enter_module(
        &mut self,
        file: &Path,
        use_span: Option<Span>,
        reject_cached: bool,
        pending: &mut Vec<Pending>,
    ) -> Result<ModuleId, KslError> {
        let file = self.normalize_file(file, use_span)?;
        let id = self.module_id(&file);
        if self.cached.contains(&id) {
            if reject_cached {
                let path = id.to_string();
                return Err(KslError::module_already_loaded(
                    &path,
                    Some(use_span.expect("直接导入必有位置")),
                ));
            }
            return Ok(id);
        }
        match self.states.get(&file) {
            Some(State::Loaded) => return Ok(id),
            Some(State::Loading) => return Err(self.cycle_error(pending, &file, use_span)),
            None => {},
        }
        self.states.insert(file.clone(), State::Loading);

        let text = std::fs::read_to_string(&file).map_err(|error| {
            KslError::with_span(
                ErrorKind::ModuleReadFailed {
                    path: self.source_name(&file),
                    message: error.to_string(),
                },
                use_span,
            )
        })?;
        let source = self.sources.add(self.source_name(&file), text);
        let program = parser::parse_text(self.sources.text(source), source)?;
        validate::validate(&program, SourceKind::Module)?;

        let decls = self.collect_decls(&file, &id, &program)?;
        pending.push(Pending {
            id: id.clone(),
            file,
            program,
            decls,
            next: 0,
            deps_reject_cached: false,
        });
        Ok(id)
    }

    /// 用显式帧栈完成深度优先加载; 依赖全部登记后出栈, 再登记自身.
    fn drain(&mut self, pending: &mut Vec<Pending>) -> Result<(), KslError> {
        loop {
            let dep = pending.last().and_then(|frame| {
                frame
                    .decls
                    .deps
                    .get(frame.next)
                    .map(|(target, span)| (target.clone(), *span))
            });
            let Some((target, span)) = dep else {
                let Some(frame) = pending.pop() else {
                    break;
                };
                self.states.insert(frame.file.clone(), State::Loaded);
                self.register(&frame.id, frame.file, frame.program, frame.decls);
                continue;
            };
            let reject_cached = pending.last().expect("帧栈非空").deps_reject_cached;
            pending.last_mut().expect("帧栈非空").next += 1;
            self.enter_module(&target, Some(span), reject_cached, pending)?;
        }
        Ok(())
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
        let mut links = Vec::new();
        let mut link_values = HashSet::new();
        let mut link_types = HashSet::new();
        let mut export_statements = 0;
        let mut seen = HashMap::new();
        for stmt in program.exprs() {
            match &stmt.node {
                ksl_frontend::ast::ExprKind::Special(Special::Use { items, path }) => {
                    // 空条目 `@Use` 不加载依赖.
                    if items.is_empty() {
                        continue;
                    }
                    let target_file = self.resolve_import(file, path, stmt.span)?;
                    let target = self.module_id(&target_file);
                    if seen.insert(target.clone(), stmt.span).is_some() {
                        return Err(KslError::duplicate_use(target.as_str(), stmt.span));
                    }
                    deps.push((target_file, stmt.span));
                    uses.push(ImportDecl {
                        target,
                        items: items.clone(),
                        span: stmt.span,
                    });
                },
                ksl_frontend::ast::ExprKind::Special(Special::Link(link)) => {
                    for ty in &link.types {
                        if ksl_frontend::check::is_reserved(&ty.node) {
                            return Err(KslError::reserved_name(&ty.node, ty.span));
                        }
                        if !link_types.insert(ty.node.clone()) || link_values.contains(&ty.node)
                        {
                            return Err(KslError::link_name_conflict(&ty.node, ty.span));
                        }
                    }
                    for value in &link.values {
                        if ksl_frontend::check::is_reserved(&value.name.node) {
                            return Err(KslError::reserved_name(
                                &value.name.node,
                                value.name.span,
                            ));
                        }
                        if !link_values.insert(value.name.node.clone())
                            || link_types.contains(&value.name.node)
                        {
                            return Err(KslError::link_name_conflict(
                                &value.name.node,
                                value.name.span,
                            ));
                        }
                    }
                    links.push(link.clone());
                },
                ksl_frontend::ast::ExprKind::Special(Special::Export { items }) => {
                    export_statements += 1;
                    if export_statements > 1 {
                        return Err(KslError::multiple_export_statements(
                            id.as_str(),
                            stmt.span,
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
            links,
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
                links: decls.links,
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
            return Err(KslError::invalid_module_path(module_path, span));
        }
        let relative_text = format!("{module_path}.ksl");
        let relative = Path::new(&relative_text);
        if relative.is_absolute() {
            return Err(KslError::invalid_module_path(module_path, span));
        }
        let mut path = current_file
            .parent()
            .map_or_else(|| self.root.clone(), Path::to_path_buf);
        for component in relative.components() {
            match component {
                Component::CurDir => {},
                Component::ParentDir => {
                    if path == self.root || !path.pop() {
                        return Err(KslError::module_path_escapes_root(module_path, span));
                    }
                },
                Component::Normal(part) => path.push(part),
                Component::RootDir | Component::Prefix(_) => {
                    return Err(KslError::invalid_module_path(module_path, span));
                },
            }
        }
        if !path.starts_with(&self.root) {
            return Err(KslError::module_path_escapes_root(module_path, span));
        }
        Ok(path)
    }

    fn normalize_file(&self, file: &Path, span: Option<Span>) -> Result<PathBuf, KslError> {
        let canonical = std::fs::canonicalize(file).map_err(|error| {
            KslError::with_span(
                ErrorKind::ModuleReadFailed {
                    path: file.display().to_string(),
                    message: error.to_string(),
                },
                span,
            )
        })?;
        if !canonical.starts_with(&self.root) {
            let path = file.display().to_string();
            return Err(KslError::module_path_escapes_root_span(&path, span));
        }
        Ok(canonical)
    }

    fn module_id(&self, file: &Path) -> ModuleId {
        ModuleId::new(relative_id(&self.root, file))
    }

    fn source_name(&self, file: &Path) -> String {
        file.trim_prefix(&self.root)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn cycle_error(&self, pending: &[Pending], file: &Path, span: Option<Span>) -> KslError {
        let start = pending.iter().position(|frame| frame.file == file).unwrap_or(0);
        let mut chain: Vec<String> = pending[start..]
            .iter()
            .map(|frame| self.source_name(&frame.file))
            .collect();
        chain.push(self.source_name(file));
        KslError::with_span(ErrorKind::CyclicImport { chain }, span)
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf, KslError> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let current = std::env::current_dir().map_err(|error| {
        KslError::without_span(ErrorKind::ModuleReadFailed {
            path: path.display().to_string(),
            message: error.to_string(),
        })
    })?;
    Ok(current.join(path))
}
