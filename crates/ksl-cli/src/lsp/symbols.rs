//! LSP 顶层文档符号.

use std::path::Path;

use ksl_base::source::{Sources, Span};
use ksl_base::symbol::resolve;
use ksl_frontend::ast::{Def, Expr, ExprKind, Special};
use ksl_frontend::parser;
use lsp_types::{DocumentSymbol, Range, SymbolKind};

use super::diagnostics::range;

pub(super) fn symbols_for(path: &Path, text: &str) -> Vec<DocumentSymbol> {
    let mut sources = Sources::default();
    let source = sources.add(path.to_string_lossy(), text);
    let Ok(program) = parser::parse_text(sources.text(source), source) else {
        return Vec::new();
    };
    program
        .exprs()
        .flat_map(|expr| symbols_for_expr(&sources, expr))
        .collect()
}

/// 构造文档符号.
///
/// `deprecated` 字段已被协议废弃, 废弃状态由 `tags` 表达, 固定为 None.
#[allow(deprecated)]
fn document_symbol(
    name: String,
    detail: &str,
    kind: SymbolKind,
    range: Range,
    selection_range: Range,
    children: Option<Vec<DocumentSymbol>>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail: Some(detail.to_string()),
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children,
    }
}

fn symbols_for_expr(sources: &Sources, expr: &Expr) -> Vec<DocumentSymbol> {
    match &expr.node {
        ExprKind::Special(Special::Data { name, variants }) => {
            let children = variants
                .iter()
                .map(|variant| {
                    document_symbol(
                        resolve(variant.name.node).to_string(),
                        "构造器",
                        SymbolKind::Constructor,
                        range(sources, variant.name.span),
                        range(sources, variant.name.span),
                        None,
                    )
                })
                .collect();
            vec![document_symbol(
                resolve(name.node).to_string(),
                "Data",
                SymbolKind::Enum,
                range(sources, expr.span),
                range(sources, name.span),
                Some(children),
            )]
        },
        ExprKind::Special(Special::Def(def)) => {
            vec![def_symbol(sources, def, expr.span)]
        },
        ExprKind::Special(Special::Fix(defs)) => defs
            .iter()
            .map(|def| {
                let span =
                    Span::new(def.name.span.source, def.name.span.start, def.body.span.end);
                def_symbol(sources, def, span)
            })
            .collect(),
        ExprKind::Special(Special::Let { target, .. }) => {
            target.name.as_ref().map_or_else(Vec::new, |name| {
                vec![document_symbol(
                    resolve(name.node).to_string(),
                    "绑定",
                    SymbolKind::Variable,
                    range(sources, expr.span),
                    range(sources, name.span),
                    None,
                )]
            })
        },
        _ => Vec::new(),
    }
}

fn def_symbol(sources: &Sources, def: &Def, span: Span) -> DocumentSymbol {
    document_symbol(
        resolve(def.name.node).to_string(),
        "函数",
        SymbolKind::Function,
        range(sources, span),
        range(sources, def.name.span),
        None,
    )
}
