//! LSP 服务.
//!
//! 最小 LSP 服务: 全量文本同步, 诊断发布与顶层文档符号. 诊断位置统一换算
//! 为 UTF-16 code unit, 与 LSP 默认位置编码一致.

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use ksl_core::ast::{Expr, ExprKind, Special};
use ksl_core::error::KslError;
use ksl_core::source::{SourceId, Sources, Span};
use ksl_core::symbol::resolve;
use ksl_core::{module, parser};
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
    Notification as LspNotification, PublishDiagnostics,
};
use lsp_types::request::{DocumentSymbolRequest, Request as LspRequest};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    InitializeResult, NumberOrString, OneOf, Position, PublishDiagnosticsParams, Range,
    ServerCapabilities, ServerInfo, SymbolKind, TextDocumentContentChangeEvent,
    TextDocumentSyncCapability, TextDocumentSyncKind, Uri,
};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};

/// URI 路径中不能直接出现的字节; 保留路径分隔符, 盘符冒号与 URL 非保留字符.
const PATH_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b':')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = Connection::stdio();
    let (id, _params) = connection.initialize_start()?;
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
        document_symbol_provider: Some(OneOf::Left(true)),
        ..ServerCapabilities::default()
    };
    let result = InitializeResult {
        capabilities,
        server_info: Some(ServerInfo {
            name: "ksl".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };
    connection.initialize_finish(id, serde_json::to_value(result)?)?;

    let mut server = Server::default();
    for msg in &connection.receiver {
        match msg {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                server.handle_request(&connection, request);
            },
            Message::Notification(notification) => {
                if notification.method == "exit" {
                    break;
                }
                server.handle_notification(&connection, notification);
            },
            Message::Response(_) => {},
        }
    }
    // 先释放连接以关闭写通道, 否则写线程会在 join 时等待通道断开.
    drop(connection);
    io_threads.join()?;
    Ok(())
}

#[derive(Default)]
struct Server {
    documents: HashMap<Uri, Document>,
}

struct Document {
    text: String,
    version: i32,
    path: PathBuf,
    /// 上次为该文档发布过诊断的 URI, 包含根文档与依赖文件.
    published: Vec<Uri>,
}

struct PublishedDiagnostic {
    uri: Uri,
    diagnostic: Diagnostic,
}

impl Server {
    fn handle_request(&self, connection: &Connection, request: Request) {
        if request.method == DocumentSymbolRequest::METHOD {
            let id = request.id.clone();
            if let Ok(params) = serde_json::from_value::<DocumentSymbolParams>(request.params) {
                let response = Response::new_ok(id, self.document_symbols(&params));
                let _ = connection.sender.send(response.into());
            } else {
                let response = Response::new_err(
                    id,
                    ErrorCode::InvalidParams as i32,
                    "documentSymbol 参数非法".to_string(),
                );
                let _ = connection.sender.send(response.into());
            }
            return;
        }
        let response = Response::new_err(
            request.id,
            ErrorCode::MethodNotFound as i32,
            format!("未实现的方法: {}", request.method),
        );
        let _ = connection.sender.send(response.into());
    }

    fn document_symbols(
        &self,
        params: &DocumentSymbolParams,
    ) -> Option<DocumentSymbolResponse> {
        let document = self.documents.get(&params.text_document.uri)?;
        Some(DocumentSymbolResponse::Nested(symbols_for(
            &document.path,
            &document.text,
        )))
    }

    fn handle_notification(&mut self, connection: &Connection, notification: Notification) {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                if let Ok(params) =
                    serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
                {
                    self.open(connection, params);
                }
            },
            DidChangeTextDocument::METHOD => {
                if let Ok(params) =
                    serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
                {
                    self.change(connection, params);
                }
            },
            DidCloseTextDocument::METHOD => {
                if let Ok(params) =
                    serde_json::from_value::<DidCloseTextDocumentParams>(notification.params)
                {
                    self.close(connection, params);
                }
            },
            _ => {},
        }
    }

    fn open(&mut self, connection: &Connection, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(path) = uri_to_path(&uri) else {
            publish_unsupported(connection, uri);
            return;
        };
        if let Some(old) = self.documents.remove(&uri) {
            for target in old.published {
                send_diagnostics(connection, target, None, Vec::new());
            }
        }
        let text = params.text_document.text;
        let version = params.text_document.version;
        let diagnostics = analyze(&path, &text);
        let published = publish(connection, &uri, Some(version), diagnostics);
        self.documents.insert(
            uri,
            Document {
                text,
                version,
                path,
                published,
            },
        );
    }

    fn change(&mut self, connection: &Connection, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(document) = self.documents.get_mut(&uri) else {
            return;
        };
        document.text = apply_changes(&document.text, params.content_changes);
        document.version = params.text_document.version;
        let diagnostics = analyze(&document.path, &document.text);
        let published = publish(connection, &uri, Some(document.version), diagnostics);
        for old in std::mem::take(&mut document.published) {
            if !published.contains(&old) {
                send_diagnostics(connection, old, None, Vec::new());
            }
        }
        document.published = published;
    }

    fn close(&mut self, connection: &Connection, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        let Some(document) = self.documents.remove(&uri) else {
            return;
        };
        for old in document.published {
            send_diagnostics(connection, old, None, Vec::new());
        }
    }
}

fn apply_changes(text: &str, changes: Vec<TextDocumentContentChangeEvent>) -> String {
    let mut text = text.to_string();
    for change in changes {
        match change.range {
            None => text = change.text,
            Some(range) => {
                let mut sources = Sources::default();
                let source = sources.add("<lsp>", text.as_str());
                let start = sources.offset_at_utf16(
                    source,
                    range.start.line as usize,
                    range.start.character as usize,
                );
                let end = sources.offset_at_utf16(
                    source,
                    range.end.line as usize,
                    range.end.character as usize,
                );
                if start <= end && end <= text.len() {
                    text.replace_range(start..end, &change.text);
                }
            },
        }
    }
    text
}

fn analyze(path: &Path, text: &str) -> Vec<PublishedDiagnostic> {
    if path.exists() {
        analyze_on_disk(path, text)
    } else {
        analyze_virtual(path, text)
    }
}

fn analyze_on_disk(path: &Path, text: &str) -> Vec<PublishedDiagnostic> {
    let mut sources = Sources::default();
    match module::load_and_check(path, text, &mut sources) {
        Ok(_) => Vec::new(),
        Err(error) => diagnostics_for(&sources, path, SourceId::default(), &error),
    }
}

fn analyze_virtual(path: &Path, text: &str) -> Vec<PublishedDiagnostic> {
    let mut sources = Sources::default();
    let source = sources.add(path.to_string_lossy(), text);
    let program = match parser::parse_text(sources.text(source), source) {
        Ok(program) => program,
        Err(error) => return diagnostics_for(&sources, path, source, &error),
    };
    let root_dir = module::root_dir_of(path);
    let name = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("main");
    match module::load_and_check_virtual(&root_dir, name, program, &mut sources) {
        Ok(_) => Vec::new(),
        Err(error) => diagnostics_for(&sources, path, source, &error),
    }
}

fn symbols_for(path: &Path, text: &str) -> Vec<DocumentSymbol> {
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
/// `lsp-types` 保留了协议中已废弃的 `deprecated` 字段, 且该结构没有
/// `Default`; 废弃状态统一由 `tags` 表达, 这里固定填 None.
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
                        SymbolKind::CONSTRUCTOR,
                        range(sources, variant.name.span),
                        range(sources, variant.name.span),
                        None,
                    )
                })
                .collect();
            vec![document_symbol(
                resolve(name.node).to_string(),
                "Data",
                SymbolKind::ENUM,
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
                    SymbolKind::VARIABLE,
                    range(sources, expr.span),
                    range(sources, name.span),
                    None,
                )]
            })
        },
        _ => Vec::new(),
    }
}

fn def_symbol(sources: &Sources, def: &ksl_core::ast::Def, span: Span) -> DocumentSymbol {
    document_symbol(
        resolve(def.name.node).to_string(),
        "函数",
        SymbolKind::FUNCTION,
        range(sources, span),
        range(sources, def.name.span),
        None,
    )
}

fn diagnostics_for(
    sources: &Sources,
    path: &Path,
    root_source: SourceId,
    error: &KslError,
) -> Vec<PublishedDiagnostic> {
    let Some(span) = error.span else {
        let Some(uri) = path_to_uri(path) else {
            return Vec::new();
        };
        return vec![PublishedDiagnostic {
            uri,
            diagnostic: diagnostic(Range::default(), error),
        }];
    };
    let uri = if span.source == root_source {
        path_to_uri(path)
    } else {
        source_uri(sources, path, span.source)
    };
    let Some(uri) = uri else {
        return Vec::new();
    };
    vec![PublishedDiagnostic {
        uri,
        diagnostic: diagnostic(range(sources, span), error),
    }]
}

fn diagnostic(range: Range, error: &KslError) -> Diagnostic {
    Diagnostic::new(
        range,
        Some(DiagnosticSeverity::ERROR),
        Some(NumberOrString::String(error.code())),
        Some("ksl".to_string()),
        error.title(),
        None,
        None,
    )
}

fn range(sources: &Sources, span: Span) -> Range {
    let (start_line, start_character) = sources.line_col_utf16(span.source, span.start);
    let (end_line, end_character) = sources.line_col_utf16(span.source, span.end);
    Range::new(
        position(start_line, start_character),
        position(end_line, end_character),
    )
}

fn position(line: usize, character: usize) -> Position {
    Position::new(to_u32(line), to_u32(character))
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn publish(
    connection: &Connection,
    uri: &Uri,
    version: Option<i32>,
    diagnostics: Vec<PublishedDiagnostic>,
) -> Vec<Uri> {
    let mut by_uri: HashMap<String, (Uri, Vec<Diagnostic>)> = HashMap::new();
    for item in diagnostics {
        by_uri
            .entry(item.uri.as_str().to_string())
            .or_insert_with(|| (item.uri, Vec::new()))
            .1
            .push(item.diagnostic);
    }
    // 当前文档始终发布一次, 没有诊断时用于清除旧结果.
    let current = by_uri
        .remove(uri.as_str())
        .map_or_else(Vec::new, |(_, diagnostics)| diagnostics);
    send_diagnostics(connection, uri.clone(), version, current);
    let mut published = vec![uri.clone()];
    for (_, (target, diagnostics)) in by_uri {
        send_diagnostics(connection, target.clone(), None, diagnostics);
        published.push(target);
    }
    published
}

fn send_diagnostics(
    connection: &Connection,
    uri: Uri,
    version: Option<i32>,
    diagnostics: Vec<Diagnostic>,
) {
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version,
    };
    let notification = Notification::new(PublishDiagnostics::METHOD.to_string(), params);
    let _ = connection.sender.send(Message::Notification(notification));
}

fn publish_unsupported(connection: &Connection, uri: Uri) {
    let diagnostic = Diagnostic::new(
        Range::default(),
        Some(DiagnosticSeverity::ERROR),
        Some(NumberOrString::String("LSP001".to_string())),
        Some("ksl".to_string()),
        "只支持 file URI".to_string(),
        None,
        None,
    );
    send_diagnostics(connection, uri, None, vec![diagnostic]);
}

fn source_uri(sources: &Sources, path: &Path, source: SourceId) -> Option<Uri> {
    let name = Path::new(sources.name(source));
    let source_path = if name.is_absolute() {
        name.to_path_buf()
    } else {
        path.parent()?.join(name)
    };
    path_to_uri(&source_path)
}

fn path_to_uri(path: &Path) -> Option<Uri> {
    let text = path.to_string_lossy().replace('\\', "/");
    let encoded = utf8_percent_encode(&text, PATH_SET).to_string();
    let uri = if encoded.starts_with('/') {
        format!("file://{encoded}")
    } else {
        format!("file:///{encoded}")
    };
    Uri::from_str(&uri).ok()
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    let text = uri.as_str();
    let rest = text.strip_prefix("file://")?;
    let path = if rest.starts_with('/') {
        rest
    } else {
        let slash = rest.find('/')?;
        &rest[slash..]
    };
    let decoded = percent_decode_str(path).decode_utf8().ok()?;
    Some(PathBuf::from(decoded.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_round_trip_preserves_unicode() {
        let path = PathBuf::from("/tmp/ksl lsp/测试.ksl");
        let uri = path_to_uri(&path).expect("路径应可转换为 URI");
        assert!(uri.as_str().contains("%20"));
        assert_eq!(uri_to_path(&uri), Some(path));
    }

    #[test]
    fn extracts_top_level_document_symbols() {
        let text = r#"
            @Data[Shape, {Point[], Circle[f64]}];
            @Def[add, [x : i64] : i64, x];
            @Fix[
                @Def[isEven, [n : u8] : bool, true],
                @Def[isOdd, [n : u8] : bool, false]
            ];
            @Let[name : string, "x"];
        "#;
        let symbols = symbols_for(Path::new("/tmp/test.ksl"), text);
        let names: Vec<_> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, ["Shape", "add", "isEven", "isOdd", "name"]);
        let children: Vec<_> = symbols[0]
            .children
            .as_ref()
            .expect("Data 应有变体")
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect();
        assert_eq!(children, ["Point", "Circle"]);
    }

    #[test]
    fn applies_utf16_ranges() {
        let change = TextDocumentContentChangeEvent {
            range: Some(Range::new(Position::new(0, 1), Position::new(0, 3))),
            range_length: None,
            text: "X".to_string(),
        };
        assert_eq!(apply_changes("a😀b\n", vec![change]), "aXb\n");
        let full = TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: "new".to_string(),
        };
        assert_eq!(apply_changes("old", vec![full]), "new");
    }
}
