//! LSP 文档状态, 检查缓存与文本变更.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ksl_base::source::Sources;
use ksl_compile::{CachedImports, CompileCache, RootSource, compile, module_name};
use ksl_frontend::parser;
use ksl_frontend::validate::{self, SourceKind};
use lsp_types::{TextDocumentContentChangeEvent, Uri};

use super::diagnostics::{PublishedDiagnostic, diagnostics_for};
use super::{CHANGE_DEBOUNCE, CHANGE_MAX_WAIT};

pub(super) struct Document {
    pub(super) text: String,
    pub(super) version: i32,
    pub(super) path: PathBuf,
    /// 模块根目录; 同一根目录下的文档复用检查缓存.
    pub(super) root: PathBuf,
    /// 上次为该文档发布过诊断的 URI, 包含根文档与依赖文件.
    pub(super) published: Vec<Uri>,
}

/// 待检查文档的变更时间.
///
/// 正常在最后一次变更后停顿 `CHANGE_DEBOUNCE` 时检查;
/// 连续输入时最迟在首次变更后 `CHANGE_MAX_WAIT` 检查.
#[derive(Default)]
pub(super) struct PendingChanges {
    entries: HashMap<Uri, PendingChange>,
}

struct PendingChange {
    first: Instant,
    last: Instant,
}

impl PendingChange {
    fn new(now: Instant) -> Self {
        Self {
            first: now,
            last: now,
        }
    }

    pub(super) fn deadline(&self) -> Instant {
        (self.last + CHANGE_DEBOUNCE).min(self.first + CHANGE_MAX_WAIT)
    }
}

impl PendingChanges {
    pub(super) fn mark(&mut self, uri: Uri, now: Instant) {
        self.entries
            .entry(uri)
            .and_modify(|pending| pending.last = now)
            .or_insert_with(|| PendingChange::new(now));
    }

    pub(super) fn remove(&mut self, uri: &Uri) {
        self.entries.remove(uri);
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.entries.values().map(PendingChange::deadline).min()
    }

    pub(super) fn take_due(&mut self, now: Instant) -> Vec<Uri> {
        let due: Vec<Uri> = self
            .entries
            .iter()
            .filter(|(_, pending)| pending.deadline() <= now)
            .map(|(uri, _)| uri.clone())
            .collect();
        for uri in &due {
            self.entries.remove(uri);
        }
        due
    }
}

pub(super) fn apply_changes(
    text: &str,
    changes: Vec<TextDocumentContentChangeEvent>,
) -> String {
    let mut text = text.to_string();
    for change in changes {
        let (range, new_text) = match change {
            TextDocumentContentChangeEvent::TextDocumentContentChangePartial(partial) => {
                (Some(partial.range), partial.text)
            },
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(whole) => {
                (None, whole.text)
            },
        };
        let Some(range) = range else {
            text = new_text;
            continue;
        };
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
            text.replace_range(start..end, &new_text);
        }
    }
    text
}

pub(super) fn analyze(
    path: &Path,
    root: &Path,
    text: &str,
    cache: &mut CompileCache,
) -> Vec<PublishedDiagnostic> {
    let mut sources = Sources::default();
    let source = sources.add(path.to_string_lossy(), text);
    let program = match parser::parse_text(sources.text(source), source) {
        Ok(program) => program,
        Err(error) => return diagnostics_for(&sources, path, source, &error),
    };
    // LSP 打开的多为模块文件; 用 Module 允许 @Export/@Main/@Link,
    // 位置错误的报告与其它输入共用同一套规则.
    if let Err(error) = validate::validate(&program, SourceKind::Module) {
        return diagnostics_for(&sources, path, source, &error);
    }
    let name = module_name(root, path);
    // 当前文档可能已作为其他文档的依赖进入缓存,
    // 其接口与 Data 声明未必与编辑中的源码一致; 重新检查前丢弃整个缓存.
    if cache.is_loaded(&name) {
        cache.invalidate();
    }
    let root = RootSource::Virtual {
        dir: root,
        name: &name,
        program,
        cached_imports: CachedImports::Allow,
    };
    match compile(root, &mut sources, cache) {
        Ok(_) => Vec::new(),
        Err(error) => diagnostics_for(&sources, path, source, &error),
    }
}
