//! ksl 语言的 tree-sitter 语法.
//!
//! 本 crate 导出解析器 [`LANGUAGE`], 节点类型表与高亮, 局部变量两份查询,
//! 供编辑器直接引用. 语法与 ksl-core 词法器的一致性由 `tests/drift.rs` 保证.
//!
//! 语法描述见 `grammar.js`, 外部扫描器见 `src/scanner.c`.
//! 修改语法后须重新生成解析器, 步骤见 `README.md`.

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_ksl() -> *const ();
}

/// ksl 语法的 [`LanguageFn`].
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_ksl) };

/// 节点类型表, 内容与 `src/node-types.json` 相同.
pub const NODE_TYPES: &str = include_str!("node-types.json");

/// 语法高亮查询.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../queries/highlights.scm");

/// 局部变量查询.
pub const LOCALS_QUERY: &str = include_str!("../queries/locals.scm");
