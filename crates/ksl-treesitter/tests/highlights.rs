//! 高亮查询的关键捕获检查.
//!
//! 只覆盖容易按名字误判的场景: 大写变量名与 `^类型` 条目.
//! 语法节点本身由 `drift.rs` 保证, 本文件检查查询最终给出的捕获类别.

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
    let captured = captures(
        r#"
@Export[foo, ^Shape];
@Use[{bar, ^Shape, ^(Circle, C), (x, y)}, "lib"];
"#,
    );
    for name in ["Shape", "Circle", "C"] {
        assert!(captured.contains(&("type".to_string(), name.to_string())));
    }
    for name in ["foo", "bar", "x", "y"] {
        assert!(captured.contains(&("variable".to_string(), name.to_string())));
        assert!(!captured.contains(&("type".to_string(), name.to_string())));
    }
}

/// 核心特殊形式与容器操作使用不同捕获.
#[test]
fn special_forms_and_container_operations_are_distinct() {
    let captured = captures(
        r#"
@Let[x, 1];
@Register["worker", @Halt[]];
@Dict[];
@ListMap[{1}, @Fn[[n : i64] : i64, n]];
"#,
    );
    for name in ["Let", "Register"] {
        assert!(captured.contains(&("keyword".to_string(), name.to_string())));
    }
    for name in ["Dict", "ListMap"] {
        assert!(captured.contains(&("keyword.operator".to_string(), name.to_string())));
    }
}

/// 定义处的函数名与 Data 类型名分别捕获为函数与类型.
#[test]
fn definition_names_have_roles() {
    let captured = captures(
        r"
@Data[M, {P[]}];
@Def[draw, [] : nil, nil];
",
    );
    assert!(captured.contains(&("type".to_string(), "M".to_string())));
    assert!(captured.contains(&("constructor".to_string(), "P".to_string())));
    assert!(captured.contains(&("function".to_string(), "draw".to_string())));
}

/// `@` 与 `^` 是标点, `/` 是分隔符.
#[test]
fn prefixes_are_punctuation() {
    let captured = captures(
        r"
@Let[x, Option/None[]];
@Export[^Shape];
",
    );
    assert!(captured.contains(&("punctuation.special".to_string(), "@".to_string())));
    assert!(captured.contains(&("punctuation.special".to_string(), "^".to_string())));
    assert!(captured.contains(&("punctuation.delimiter".to_string(), "/".to_string())));
}

/// 类型位置的 `Fn` 是类型构造器, 不是特殊形式名.
#[test]
fn fn_type_constructor_is_a_type() {
    let captured = captures("@Let[f : Fn[[u64] : string], nil];");
    assert!(captured.contains(&("type".to_string(), "Fn".to_string())));
}
