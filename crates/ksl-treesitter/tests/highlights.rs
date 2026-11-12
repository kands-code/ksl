//! 高亮查询的关键捕获检查.
//!
//! 只覆盖容易按名字误判的场景: 大写变量名与 `^类型` 条目.
//! 语法节点本身由 `drift.rs` 保证, 这里检查查询最终给出的捕获类别.

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

/// 解析源码并返回全部高亮捕获.
fn captures(source: &str) -> Vec<(String, String)> {
    let language: tree_sitter::Language = ksl_treesitter::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&language).expect("装载 ksl 语法失败");
    let tree = parser.parse(source, None).expect("tree-sitter 解析失败");
    let query =
        Query::new(&language, ksl_treesitter::HIGHLIGHTS_QUERY).expect("编译高亮查询失败");

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut out = Vec::new();
    while let Some(matched) = matches.next() {
        for capture in matched.captures() {
            let name = query.capture_names()[capture.index as usize].to_string();
            let text = source[capture.node.byte_range()].to_string();
            out.push((name, text));
        }
    }
    out
}

/// 变量名不按首字母大小写判定类型.
#[test]
fn variable_names_are_not_types() {
    let captured = captures("@Let[a, 1]; @Let[名, 2]; @Let[A, 3];");
    for name in ["a", "名", "A"] {
        assert!(captured.contains(&("variable".to_string(), name.to_string())));
        assert!(!captured.contains(&("type".to_string(), name.to_string())));
    }
}

/// `@Export` 与 `@Use` 中的 `^类型` 必须捕获为类型.
#[test]
fn exported_types_are_captured_as_type() {
    let captured =
        captures("@Export[foo, ^Shape];\n@Use[{bar, ^Shape, ^(Circle, C), (x, y)}, \"lib\"];");
    for name in ["Shape", "Circle", "C"] {
        assert!(captured.contains(&("type".to_string(), name.to_string())));
    }
    for name in ["foo", "bar", "x", "y"] {
        assert!(captured.contains(&("variable".to_string(), name.to_string())));
        assert!(!captured.contains(&("type".to_string(), name.to_string())));
    }
}
