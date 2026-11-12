//! LSP 服务.
//!
//! 最小实现: 全量文本同步, 诊断发布与顶层文档符号.
//! 变更后的诊断等待短暂停顿再计算, 连续输入时最迟每秒计算一次.
//! 诊断位置统一换算为 UTF-16 code unit, 与 LSP 默认位置编码一致.

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossbeam_channel::RecvTimeoutError;
use ksl_runtime::module;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::{
    DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidCloseTextDocumentNotification, DidCloseTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolProvider, DocumentSymbolRequest, DocumentSymbolResponse, InitializeResult,
    Notification as LspNotification, Request as LspRequest, ServerCapabilities, ServerInfo,
    TextDocumentSync, TextDocumentSyncKind, Uri,
};

mod diagnostics;
mod document;
mod symbols;

use diagnostics::{publish, publish_unsupported, send_diagnostics, uri_to_path};
use document::{AnalysisCache, Document, PendingChanges, analyze, apply_changes};
use symbols::symbols_for;

/// 一次变更之后等待输入停顿的时长.
const CHANGE_DEBOUNCE: Duration = Duration::from_millis(300);
/// 连续输入时最长等待多久必须检查一次.
const CHANGE_MAX_WAIT: Duration = Duration::from_secs(1);

pub fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = Connection::stdio();
    let (id, _params) = connection.initialize_start()?;
    let capabilities = ServerCapabilities {
        text_document_sync: Some(TextDocumentSync::Kind(TextDocumentSyncKind::Full)),
        document_symbol_provider: Some(DocumentSymbolProvider::Bool(true)),
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
    loop {
        server.flush_due(&connection, Instant::now());
        let timeout = server
            .pending
            .deadline()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()));
        let msg = match timeout {
            Some(timeout) => match connection.receiver.recv_timeout(timeout) {
                Ok(msg) => msg,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match connection.receiver.recv() {
                Ok(msg) => msg,
                Err(_) => break,
            },
        };
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
    /// 已收到变更, 等待防抖或最长等待到期的文档.
    pending: PendingChanges,
    /// 按根目录复用的依赖检查结果.
    caches: HashMap<PathBuf, AnalysisCache>,
}

impl Server {
    fn handle_request(&self, connection: &Connection, request: Request) {
        if request.method == DocumentSymbolRequest::METHOD.as_str() {
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
        Some(DocumentSymbolResponse::DocumentSymbolList(symbols_for(
            &document.path,
            &document.text,
        )))
    }

    fn handle_notification(&mut self, connection: &Connection, notification: Notification) {
        match notification.method.as_str() {
            method if method == DidOpenTextDocumentNotification::METHOD.as_str() => {
                if let Ok(params) =
                    serde_json::from_value::<DidOpenTextDocumentParams>(notification.params)
                {
                    self.open(connection, params);
                }
            },
            method if method == DidChangeTextDocumentNotification::METHOD.as_str() => {
                if let Ok(params) =
                    serde_json::from_value::<DidChangeTextDocumentParams>(notification.params)
                {
                    self.change(params);
                }
            },
            method if method == DidCloseTextDocumentNotification::METHOD.as_str() => {
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
        self.pending.remove(&uri);
        let Some(path) = uri_to_path(&uri) else {
            publish_unsupported(connection, uri);
            return;
        };
        let root = module::root_dir_of(&path);
        if let Some(old) = self.documents.remove(&uri) {
            for target in old.published {
                send_diagnostics(connection, target, None, Vec::new());
            }
        }
        self.documents.insert(
            uri.clone(),
            Document {
                text: params.text_document.text,
                version: params.text_document.version,
                path,
                root,
                published: Vec::new(),
            },
        );
        self.refresh(connection, &uri);
    }

    fn change(&mut self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.text_document_identifier.uri;
        let Some(document) = self.documents.get_mut(&uri) else {
            return;
        };
        document.text = apply_changes(&document.text, params.content_changes);
        document.version = params.text_document.version;
        self.pending.mark(uri, Instant::now());
    }

    fn close(&mut self, connection: &Connection, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.pending.remove(&uri);
        let Some(document) = self.documents.remove(&uri) else {
            return;
        };
        let root = document.root;
        for old in document.published {
            send_diagnostics(connection, old, None, Vec::new());
        }
        if !self.documents.values().any(|other| other.root == root) {
            self.caches.remove(&root);
        }
    }

    /// 立即检查并发布一个文档的诊断.
    fn refresh(&mut self, connection: &Connection, uri: &Uri) {
        let published = {
            let Some(document) = self.documents.get(uri) else {
                return;
            };
            let version = document.version;
            let cache = self.caches.entry(document.root.clone()).or_default();
            let diagnostics = analyze(&document.path, &document.root, &document.text, cache);
            publish(connection, uri, Some(version), diagnostics)
        };
        let Some(document) = self.documents.get_mut(uri) else {
            return;
        };
        for old in std::mem::take(&mut document.published) {
            if !published.contains(&old) {
                send_diagnostics(connection, old, None, Vec::new());
            }
        }
        document.published = published;
    }

    /// 检查所有已到期的 pending 文档.
    fn flush_due(&mut self, connection: &Connection, now: Instant) {
        for uri in self.pending.take_due(now) {
            self.refresh(connection, &uri);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use lsp_types::{
        Position, Range, TextDocumentContentChangeEvent, TextDocumentContentChangePartial,
        TextDocumentContentChangeWholeDocument, Uri,
    };

    use super::diagnostics::path_to_uri;
    use super::document::dependency_path;
    use super::*;

    #[test]
    fn file_uri_conversion() {
        let path = PathBuf::from("/tmp/ksl lsp/测试.ksl");
        let uri = path_to_uri(&path).expect("路径应可转换为 URI");
        assert!(uri.as_str().contains("%20"));
        assert_eq!(uri_to_path(&uri), Some(path));

        #[cfg(not(windows))]
        {
            let uri = Uri::parse("file://server/share/x.ksl").expect("URI 应可解析");
            assert_eq!(uri_to_path(&uri), None);
        }
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
        let change: TextDocumentContentChangeEvent = TextDocumentContentChangePartial::new(
            Range::new(Position::new(0, 1), Position::new(0, 3)),
            None,
            "X".to_string(),
        )
        .into();
        assert_eq!(apply_changes("a😀b\n", vec![change]), "aXb\n");
        let full: TextDocumentContentChangeEvent =
            TextDocumentContentChangeWholeDocument::new("new".to_string()).into();
        assert_eq!(apply_changes("old", vec![full]), "new");
    }

    #[test]
    fn pending_changes_wait_for_debounce_or_max_wait() {
        let now = Instant::now();
        let uri = Uri::parse("file:///tmp/a.ksl").expect("URI 应可解析");
        let mut pending = PendingChanges::default();
        pending.mark(uri.clone(), now);
        assert_eq!(pending.deadline(), Some(now + CHANGE_DEBOUNCE));

        pending.mark(uri.clone(), now + Duration::from_millis(200));
        assert_eq!(
            pending.deadline(),
            Some(now + Duration::from_millis(200) + CHANGE_DEBOUNCE)
        );

        pending.mark(uri.clone(), now + Duration::from_millis(900));
        assert_eq!(pending.deadline(), Some(now + CHANGE_MAX_WAIT));
        let before = now + Duration::from_millis(999);
        assert_eq!(pending.take_due(before), Vec::<Uri>::new());
        assert_eq!(pending.take_due(now + CHANGE_MAX_WAIT), vec![uri]);
    }

    #[test]
    fn reanalyzing_open_document_reuses_dependency_cache() {
        let root = ksl_base::testutil::TempDir::new("ksl_lsp_reanalyze_");
        let lib = root.path().join("lib");
        std::fs::create_dir_all(&lib).expect("依赖目录应可创建");
        std::fs::write(lib.join("a.ksl"), r"@Data[A, {C[]}]; @Export[^A];")
            .expect("依赖文件应可写入");
        let path = root.path().join("main.ksl");
        let text = r#"@Use[{^A}, "lib/a"]; @Data[B, {D[]}]; @Export[^B];"#;
        std::fs::write(&path, text).expect("文档应可写入");
        let root_dir = module::root_dir_of(&path);
        let mut cache = AnalysisCache::default();
        for _ in 0..2 {
            let diagnostics = analyze(&path, &root_dir, text, &mut cache);
            assert!(diagnostics.is_empty(), "文档不应产生诊断");
        }
    }

    #[test]
    fn reopening_module_previously_loaded_as_dependency_discards_cache() {
        let root = ksl_base::testutil::TempDir::new("ksl_lsp_cached_root_");
        let dep = root.path().join("a.ksl");
        let text = r"@Data[A, {C[]}]; @Export[^A];";
        std::fs::write(&dep, text).expect("依赖文件应可写入");
        let main = root.path().join("main.ksl");
        let main_text = r#"@Use[{^A}, "a"];"#;
        std::fs::write(&main, main_text).expect("文档应可写入");
        let root_dir = module::root_dir_of(&main);
        let mut cache = AnalysisCache::default();
        assert!(analyze(&main, &root_dir, main_text, &mut cache).is_empty());
        // a.ksl 已作为 main.ksl 的依赖缓存; 作为根重新检查时须丢弃该缓存.
        assert!(analyze(&dep, &root_dir, text, &mut cache).is_empty());
    }

    #[test]
    fn dependency_path_appends_extension_without_losing_dots() {
        assert_eq!(
            dependency_path(Path::new("/tmp/root"), "lib/a.b"),
            PathBuf::from("/tmp/root/lib/a.b.ksl")
        );
    }
}
