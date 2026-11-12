//! 漂移测试: tree-sitter 语法与 ksl 词法器的记号流必须完全一致.
//!
//! 两者分叉时编辑器高亮会与解释器语义脱节, 且不会报错.
//! 语料固定放在本 crate 的 `test/drift`, 不依赖仓库其他目录, 外部改动不会破坏测试.

use std::path::{Path, PathBuf};

use ksl_base::source::Sources;
use ksl_frontend::lexer::{TokenKind, lex};
use tree_sitter::{Node, Parser};

/// 记号类别.
///
/// tree-sitter 节点与 ksl 记号各自映射到同一组类别后比对, 省去两套命名的一一对应.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Category {
    /// 标识符, 含特殊形式名与字面关键字.
    Ident,
    /// 丢弃记号 `_`.
    Discard,
    I64,
    U8,
    U64,
    F64,
    Str,
    RawStr,
    FStr,
    Char,
    /// `@`.
    At,
    /// `^`.
    Caret,
    /// 类型限定名中的斜杠.
    Slash,
    /// 括号与分隔符.
    Punct,
}

/// ksl 记号到类别的映射.
fn ksl_category(kind: &TokenKind) -> Category {
    match kind {
        TokenKind::Ident(_) => Category::Ident,
        TokenKind::Discard => Category::Discard,
        TokenKind::I64(_) => Category::I64,
        TokenKind::U8(_) => Category::U8,
        TokenKind::U64(_) => Category::U64,
        TokenKind::F64(_) => Category::F64,
        TokenKind::Str(_) => Category::Str,
        TokenKind::RawStr(_) => Category::RawStr,
        TokenKind::FStr(_) => Category::FStr,
        TokenKind::Char(_) => Category::Char,
        TokenKind::At => Category::At,
        TokenKind::Caret => Category::Caret,
        TokenKind::Slash => Category::Slash,
        TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LParen
        | TokenKind::RParen
        | TokenKind::Colon
        | TokenKind::Semi
        | TokenKind::Comma => Category::Punct,
        // 文件结束记号在比对前已被滤除, 出现该记号说明比对流程有误.
        TokenKind::Eof => unreachable!("文件结束记号不参与比对"),
    }
}

/// tree-sitter 节点到类别的映射.
fn ts_category(kind: &str, text: &str) -> Category {
    match kind {
        "identifier" => Category::Ident,
        "discard" => Category::Discard,
        "integer" => Category::I64,
        "u8_integer" => Category::U8,
        "u64_integer" => Category::U64,
        "float" => Category::F64,
        "string" => Category::Str,
        "raw_string" => Category::RawStr,
        "fstring" => Category::FStr,
        "char_literal" => Category::Char,
        "@" => Category::At,
        "^" => Category::Caret,
        "/" => Category::Slash,
        "[" | "]" | "{" | "}" | "(" | ")" | ":" | ";" | "," => Category::Punct,
        // 其余节点为特殊形式名与字面关键字, 词法层同样按标识符处理.
        _ if text == "_" => Category::Discard,
        _ => Category::Ident,
    }
}

/// 语料: 本 crate 内的固定漂移用例.
fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("test/drift");
    let mut files = Vec::new();
    collect_ksl(&root, &mut files);
    files.sort();
    files
}

/// 递归收集目录下的 `.ksl` 文件.
fn collect_ksl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_ksl(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "ksl") {
            out.push(path);
        }
    }
}

/// 收集解析树的叶子记号; 注释是 extras, 不参与比对.
fn leaves(root: Node<'_>, source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.child_count() == 0 && node.kind() != "comment" {
            let text = &source[node.start_byte()..node.end_byte()];
            out.push((node.kind().to_string(), text.to_string()));
        }
        if cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return out;
            }
        }
    }
}

/// 语法必须能无错解析全部语料.
#[test]
fn tree_sitter_parses_corpus_without_errors() {
    let files = corpus();
    assert!(!files.is_empty(), "语料不应为空");
    for path in &files {
        let source = std::fs::read_to_string(path).expect("读取语料失败");
        let tree = parse(&source);
        assert!(
            !tree.root_node().has_error(),
            "{}: tree-sitter 解析出现错误节点",
            path.display()
        );
    }
}

/// 两侧的记号流必须逐个一致.
#[test]
fn tree_sitter_token_stream_matches_ksl_lexer() {
    let files = corpus();
    assert!(!files.is_empty(), "语料不应为空");
    for path in &files {
        let source = std::fs::read_to_string(path).expect("读取语料失败");
        compare(path, &source);
    }
}

/// 语料必须是能通过类型检查的程序.
///
/// 记号流比对只覆盖词法, 程序本身失效时不会暴露.
/// 类型检查同时覆盖模块解析与导入导出, 语料过期时本测试会报错.
#[test]
fn drift_corpus_passes_type_check() {
    let files = corpus();
    assert!(!files.is_empty(), "语料不应为空");
    for path in &files {
        let mut sources = Sources::default();
        if let Err(err) = ksl_runtime::module::load_and_check_file(path, &mut sources) {
            panic!("{}: 语料未通过类型检查: {err}", path.display());
        }
    }
}

/// 语料必须覆盖全部记号类别.
///
/// 缺少某类时记号流比对会静默失去覆盖面; 新增记号类别而未补语料时在此报出.
#[test]
fn corpus_covers_every_token_category() {
    const ALL: &[Category] = &[
        Category::Ident,
        Category::Discard,
        Category::I64,
        Category::U8,
        Category::U64,
        Category::F64,
        Category::Str,
        Category::RawStr,
        Category::FStr,
        Category::Char,
        Category::At,
        Category::Caret,
        Category::Slash,
        Category::Punct,
    ];
    let mut seen = std::collections::HashSet::new();
    for path in corpus() {
        let source = std::fs::read_to_string(&path).expect("读取语料失败");
        let mut sources = Sources::default();
        let id = sources.add("<漂移测试>", source.as_str());
        for token in lex(source.as_str(), id)
            .expect("ksl 词法失败")
            .iter()
            .filter(|token| !matches!(token.kind, TokenKind::Eof))
        {
            seen.insert(ksl_category(&token.kind));
        }
    }
    for category in ALL {
        assert!(seen.contains(category), "语料未覆盖记号类别 {category:?}");
    }
}

/// 词法非法的形态必须被语法拒绝.
///
/// 这些形态 ksl-frontend 判为词法错误; 语法若照单全收, 编辑器的错误提示会与解释器脱节.
#[test]
fn tree_sitter_rejects_invalid_literals() {
    const INVALID: &[&str] = &[
        "''",
        "'ab'",
        "'\\q'",
        "\"a\\q\"",
        "f\"a}b\"",
        "f\"a\\qb\"",
        "r\"a\"##",
    ];
    for snippet in INVALID {
        let tree = parse(snippet);
        assert!(
            tree.root_node().has_error(),
            "语法接受了词法非法的形态: {snippet:?}"
        );
    }
}

fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = ksl_treesitter::LANGUAGE.into();
    parser.set_language(&language).expect("装载 ksl 语法失败");
    parser.parse(source, None).expect("tree-sitter 解析失败")
}

/// 比对一份源码在两侧的记号流.
fn compare(path: &Path, source: &str) {
    let mut sources = Sources::default();
    let id = sources.add("<漂移测试>", source);
    let tokens =
        lex(source, id).unwrap_or_else(|err| panic!("{}: ksl 词法失败: {err}", path.display()));
    let expected: Vec<_> = tokens
        .iter()
        .filter(|token| !matches!(token.kind, TokenKind::Eof))
        .collect();

    let tree = parse(source);
    let actual = leaves(tree.root_node(), source);

    for index in 0..expected.len().max(actual.len()) {
        let Some(token) = expected.get(index) else {
            panic!(
                "{}: tree-sitter 多出记号 {index}: {:?}",
                path.display(),
                actual[index]
            );
        };
        let Some((kind, text)) = actual.get(index) else {
            panic!(
                "{}: tree-sitter 缺少记号 {index}: {:?}",
                path.display(),
                token.kind
            );
        };

        let ksl_text = &source[token.span.start..token.span.end];
        let (line, col) = sources.line_col(id, token.span.start);
        assert_eq!(
            ksl_text,
            text,
            "{}:{}:{}: 第 {index} 个记号文本不一致: ksl {ksl_text:?}, tree-sitter {text:?}",
            path.display(),
            line + 1,
            col + 1
        );
        assert_eq!(
            ksl_category(&token.kind),
            ts_category(kind, text),
            "{}:{}:{}: 第 {index} 个记号类别不一致: ksl {:?}, tree-sitter {kind:?}",
            path.display(),
            line + 1,
            col + 1,
            token.kind
        );
    }
}
