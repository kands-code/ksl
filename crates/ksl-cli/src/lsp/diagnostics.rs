//! LSP 诊断构造, 发布与 URI 换算.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ksl_base::source::{SourceId, Sources, Span};
use ksl_diagnostics::error::KslError;
use lsp_server::{Connection, Message, Notification};
use lsp_types::{
    Code, Diagnostic, DiagnosticSeverity, Message as DiagnosticMessage,
    Notification as LspNotification, Position, PublishDiagnosticsNotification,
    PublishDiagnosticsParams, Range, Uri,
};

pub(super) struct PublishedDiagnostic {
    uri: Uri,
    pub(super) diagnostic: Diagnostic,
}

pub(super) fn diagnostics_for(
    sources: &Sources,
    path: &Path,
    root_source: SourceId,
    error: &KslError,
) -> Vec<PublishedDiagnostic> {
    let Some(span) = error.span() else {
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
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::Error),
        code: error.code().map(Code::String),
        source: Some("ksl".to_string()),
        message: DiagnosticMessage::String(error.title()),
        ..Diagnostic::default()
    }
}

pub(super) fn range(sources: &Sources, span: Span) -> Range {
    let (start_line, start_character) = sources.line_col_utf16(span.source, span.start());
    let (end_line, end_character) = sources.line_col_utf16(span.source, span.end());
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

pub(super) fn publish(
    connection: &Connection,
    uri: &Uri,
    version: Option<i32>,
    diagnostics: Vec<PublishedDiagnostic>,
) -> Vec<Uri> {
    let mut by_uri: HashMap<Uri, Vec<Diagnostic>> = HashMap::new();
    for item in diagnostics {
        by_uri.entry(item.uri).or_default().push(item.diagnostic);
    }
    // 当前文档始终发布一次, 没有诊断时清除旧结果.
    let current = by_uri.remove(uri).unwrap_or_default();
    send_diagnostics(connection, uri.clone(), version, current);
    let mut published = vec![uri.clone()];
    for (target, diagnostics) in by_uri {
        send_diagnostics(connection, target.clone(), None, diagnostics);
        published.push(target);
    }
    published
}

pub(super) fn send_diagnostics(
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
    let notification = Notification::new(
        PublishDiagnosticsNotification::METHOD.as_str().to_string(),
        params,
    );
    let _ = connection.sender.send(Message::Notification(notification));
}

pub(super) fn publish_unsupported(connection: &Connection, uri: Uri) {
    let diagnostic = Diagnostic {
        range: Range::default(),
        severity: Some(DiagnosticSeverity::Error),
        code: Some(Code::String("LSP001".to_string())),
        source: Some("ksl".to_string()),
        message: DiagnosticMessage::String("只支持 file URI".to_string()),
        ..Diagnostic::default()
    };
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

pub(super) fn path_to_uri(path: &Path) -> Option<Uri> {
    Uri::from_file_path(path).ok()
}

pub(super) fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().ok()
}
