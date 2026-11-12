//! 诊断渲染.
//!
//! 以 annotate-snippets 为统一出口, 保证终端错误形态一致.
//! 位置换算统一走 [`Sources`], 渲染本身只做格式编排.
//! 各阶段的错误共用渲染骨架, 不分别实现.

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};
use ksl_base::source::{Sources, Span};

use crate::error::{KslError, RuntimeError};

/// 把错误渲染为纯文本诊断, 供终端输出.
///
/// 报告首行为 `error[EXXXX]: 标题`, 其后是具体信息与出错名字.
/// 高亮出错源码位置.
#[must_use]
pub fn render_error(sources: &Sources, err: &KslError) -> String {
    let note = err.kind.note();
    if let Some(span) = err.span {
        render(sources, span, err.code(), err.title(), note.as_deref())
    } else {
        let title = Level::ERROR.primary_title(err.title()).id(err.code());
        let group = match note {
            Some(note) => Group::with_title(title).element(Level::NOTE.message(note)),
            None => Group::with_title(title),
        };
        Renderer::plain().render(&[group])
    }
}

/// 把 Actor 错误渲染为纯文本诊断; 明确失败不影响进程退出码.
#[must_use]
pub fn render_actor_error(sources: &Sources, err: &RuntimeError) -> String {
    let note = Renderer::plain().render(&[Group::with_title(
        Level::NOTE.primary_title("Actor 失败, 不影响退出码"),
    )]);
    format!("{note}\n{}", render_runtime_error(sources, err))
}

/// 把运行时错误渲染为纯文本诊断, 供终端输出.
///
/// ksl 级错误高亮出错源码位置. 内部错误无位置, 只输出标题, 并注明为内部错误.
#[must_use]
pub fn render_runtime_error(sources: &Sources, err: &RuntimeError) -> String {
    match err {
        RuntimeError::Runtime(err) => render_error(sources, err),
        // 退出不是错误: 命令行先行拦截, 此分支仅为穷尽匹配.
        RuntimeError::Exit(code) => Renderer::plain().render(&[Group::with_title(
            Level::NOTE.primary_title(format!("程序以退出码 {code} 退出")),
        )]),
        RuntimeError::BrokenPipe => Renderer::plain()
            .render(&[Group::with_title(Level::NOTE.primary_title("输出流已关闭"))]),
        RuntimeError::Internal(message) => Renderer::plain().render(&[Group::with_title(
            Level::ERROR.primary_title(format!("内部错误: {message}")),
        )]),
    }
}

/// 错误共用的渲染骨架: 标题加带高亮的源码切片.
///
/// 源码取整个文件, 行号从第 1 行开始计数.
fn render(
    sources: &Sources,
    span: Span,
    code: String,
    title: String,
    note: Option<&str>,
) -> String {
    let line_start = 1;
    let group = Level::ERROR.primary_title(title).id(code).element(
        Snippet::source(sources.text(span.source))
            .path(sources.name(span.source))
            .line_start(line_start)
            .annotation(AnnotationKind::Primary.span(span.start..span.end)),
    );
    let group = match note {
        Some(note) => group.element(Level::NOTE.message(note)),
        None => group,
    };
    let report = [group];
    Renderer::plain().render(&report)
}
