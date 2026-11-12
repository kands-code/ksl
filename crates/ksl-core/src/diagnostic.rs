//! 诊断渲染.
//!
//! 以 annotate-snippets 为统一出口, 保证终端错误形态一致;
//! 位置换算统一走 [`Sources`], 渲染本身只做格式编排.
//! 四阶段的错误形态统一, 故只留一个渲染入口.

use annotate_snippets::{AnnotationKind, Group, Level, Renderer, Snippet};

use crate::error::{KslError, RuntimeError};
use crate::source::{Sources, Span};

/// 把错误渲染为纯文本诊断, 供终端输出.
///
/// 报告首行为 `error[EXXXX]: 标题`, 其后是具体信息与出错名字;
/// 高亮出错源码位置.
#[must_use]
pub fn render_error(sources: &Sources, err: &KslError) -> String {
    match err.span {
        Some(span) => render(sources, span, err.code(), err.title()),
        None => Renderer::plain()
            .render(&[Group::with_title(
                Level::ERROR.primary_title(err.title()).id(err.code()),
            )])
            .clone(),
    }
}

/// 把 Actor 错误渲染为纯文本诊断; 明确失败不影响进程退出码.
#[must_use]
pub fn render_actor_error(sources: &Sources, err: &RuntimeError) -> String {
    let note = Renderer::plain()
        .render(&[Group::with_title(
            Level::NOTE.primary_title("Actor 失败, 不影响退出码"),
        )])
        .clone();
    format!("{note}\n{}", render_runtime_error(sources, err))
}

/// 把运行时错误渲染为纯文本诊断, 供终端输出.
///
/// ksl 级错误高亮出错源码位置; 内部错误无位置, 只输出标题, 标题注明
/// 其属实现缺陷.
#[must_use]
pub fn render_runtime_error(sources: &Sources, err: &RuntimeError) -> String {
    match err {
        RuntimeError::Runtime(err) => render_error(sources, err),
        // 退出不是错误: 命令行先行拦截, 此处只为分支完整.
        RuntimeError::Exit(code) => Renderer::plain()
            .render(&[Group::with_title(
                Level::NOTE.primary_title(format!("程序以退出码 {code} 退出")),
            )])
            .clone(),
        RuntimeError::BrokenPipe => Renderer::plain()
            .render(&[Group::with_title(Level::NOTE.primary_title("输出流已关闭"))])
            .clone(),
        RuntimeError::Internal(message) => Renderer::plain()
            .render(&[Group::with_title(
                Level::ERROR.primary_title(format!("内部错误: {message}")),
            )])
            .clone(),
    }
}

/// 错误共用的渲染骨架: 标题加带高亮的源码切片.
///
/// 传入的是完整文件, 片段首行即第 1 行.
fn render(sources: &Sources, span: Span, code: String, title: String) -> String {
    let line_start = 1;
    let report = [Level::ERROR.primary_title(title).id(code).element(
        Snippet::source(sources.text(span.source))
            .path(sources.name(span.source))
            .line_start(line_start)
            .annotation(AnnotationKind::Primary.span(span.start..span.end)),
    )];
    Renderer::plain().render(&report).clone()
}
