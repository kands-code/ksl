//! 模块测试公共代码: 临时目录, 加载辅助与插件文件.
//!
//! 各测试目标只使用部分辅助, 模块整体放宽未使用项检查.

#![allow(dead_code)]

use ksl_base::source::Sources;
use ksl_base::testutil::{TempDir, write, write_path};
use ksl_compile::*;
use ksl_diagnostics::error::KslError;

macro_rules! assert_error_kind {
    ($error:expr, $pattern:pat) => {{
        let error = $error;
        assert!(
            matches!(error.kind(), Some(kind) if matches!(kind, $pattern)),
            "错误种类不符: {error}"
        );
    }};
}

pub(crate) use assert_error_kind;

/// 多个用例共用的模块文本.
pub(crate) const LIB_A_DATA: &str = r"@Data[A, {C[i64]}]; @Export[^A];";
pub(crate) const LIB_A_DATA_EMPTY: &str = r"@Data[A, {C[]}]; @Export[^A];";
pub(crate) const MAKE_ONE: &str = r"@Let[make, @Fn[[] : i64, 1]]; @Export[make];";
/// 单变体 Data 类型与返回该类型的值.
pub(crate) const BOX_MODULE: &str =
    r"@Data[Box, {Wrap[i64]}]; @Let[get, @Fn[[] : Box, Box/Wrap[7]]]; @Export[get, ^Box];";

pub(crate) fn temp_root(tag: &str) -> TempDir {
    TempDir::new(&format!("ksl_compile_{tag}_"))
}

/// 写入一组模块文件并返回主文件路径.
pub(crate) fn write_files(
    tag: &str,
    files: &[(&str, &str)],
    main: &str,
) -> (TempDir, std::path::PathBuf) {
    let root = temp_root(tag);
    for (relative, text) in files {
        write(&root, relative, text);
    }
    let path = write_path(&root, "main.ksl", main);
    (root, path)
}

fn text_root<'a>(path: &'a std::path::Path, text: &'a str) -> RootSource<'a> {
    RootSource::Text { path, text }
}

pub(crate) fn load_unlinked(tag: &str, files: &[(&str, &str)], main: &str) -> ModuleGraph {
    let (_root, path) = write_files(tag, files, main);
    let mut sources = Sources::default();
    load(text_root(&path, main), &mut sources, &CompileCache::new()).expect("模块应可加载")
}

pub(crate) fn load_unlinked_error(tag: &str, files: &[(&str, &str)], main: &str) -> KslError {
    let (_root, path) = write_files(tag, files, main);
    let mut sources = Sources::default();
    load(text_root(&path, main), &mut sources, &CompileCache::new()).expect_err("模块应报错")
}

pub(crate) fn checked_text(path: &std::path::Path, text: &str) -> CheckedGraph {
    let mut sources = Sources::default();
    compile(text_root(path, text), &mut sources, &mut CompileCache::new())
        .expect("模块应通过链接与检查")
}

pub(crate) fn checked_text_error(path: &std::path::Path, text: &str) -> KslError {
    let mut sources = Sources::default();
    compile(text_root(path, text), &mut sources, &mut CompileCache::new())
        .expect_err("模块应报错")
}

pub(crate) fn load_ok(tag: &str, files: &[(&str, &str)], main: &str) -> CheckedGraph {
    let (_root, path) = write_files(tag, files, main);
    checked_text(&path, main)
}

pub(crate) fn load_error(tag: &str, files: &[(&str, &str)], main: &str) -> KslError {
    let (_root, path) = write_files(tag, files, main);
    checked_text_error(&path, main)
}

pub(crate) fn compile_virtual(
    dir: &std::path::Path,
    name: &str,
    program: ksl_frontend::ast::Program,
    sources: &mut Sources,
    cache: &mut CompileCache,
    cached_imports: CachedImports,
) -> Result<CheckedGraph, KslError> {
    compile(
        RootSource::Virtual {
            dir,
            name,
            program,
            cached_imports,
        },
        sources,
        cache,
    )
}

pub(crate) fn main_module(graph: &CheckedGraph) -> &CheckedModule {
    graph
        .get(&ModuleId::new("main".to_string()))
        .expect("主模块应存在")
}

pub(crate) fn order_of(order: &[ModuleId]) -> Vec<&str> {
    order.iter().map(ModuleId::as_str).collect()
}

pub(crate) fn virtual_program(
    sources: &mut Sources,
    name: &str,
    text: &str,
) -> ksl_frontend::ast::Program {
    let source = sources.add(name, text);
    ksl_frontend::parser::parse_text(sources.text(source), source).expect("虚拟根应可解析")
}

/// 写入模块文件, 并在指定目录下放置空插件文件.
///
/// 链接阶段只要求插件路径存在, 不读取内容.
pub(crate) fn write_files_with_plugin(
    tag: &str,
    module_dir: &str,
    plugin: &str,
    files: &[(&str, &str)],
    main: &str,
) -> (TempDir, std::path::PathBuf) {
    let root = temp_root(tag);
    for (relative, text) in files {
        write(&root, relative, text);
    }
    let file = ksl_base::plugin::plugin_file(plugin).expect("测试插件名应合法");
    let _ = write_path(&root, &format!("{module_dir}plugin/{file}"), "");
    let path = write_path(&root, "main.ksl", main);
    (root, path)
}

pub(crate) fn load_ok_with_plugin(
    tag: &str,
    module_dir: &str,
    plugin: &str,
    files: &[(&str, &str)],
    main: &str,
) -> CheckedGraph {
    let (_root, path) = write_files_with_plugin(tag, module_dir, plugin, files, main);
    let mut sources = Sources::default();
    compile(text_root(&path, main), &mut sources, &mut CompileCache::new())
        .expect("模块应通过链接与检查")
}

pub(crate) fn load_error_with_plugin(
    tag: &str,
    module_dir: &str,
    plugin: &str,
    files: &[(&str, &str)],
    main: &str,
) -> KslError {
    let (_root, path) = write_files_with_plugin(tag, module_dir, plugin, files, main);
    let mut sources = Sources::default();
    compile(text_root(&path, main), &mut sources, &mut CompileCache::new())
        .expect_err("模块应报错")
}
