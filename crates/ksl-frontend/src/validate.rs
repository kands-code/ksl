//! 语义位置校验.
//!
//! parser 只负责语法; 本阶段集中检查与类型无关的位置约束:
//! 顶层与 `@Block` 语句位置, 程序根与 `@Main` 体, Actor 原语的作用域,
//! 以及模块级形式适用的输入来源.
//!
//! 校验不改变 AST, 也不依赖类型环境与模块依赖.

use ksl_base::source::Span;
use ksl_diagnostics::error::{ErrorKind, KslError};

use crate::ast::{Expr, ExprKind, FStrAstPart, Program, Special};

/// 输入来源; 决定模块级形式是否允许.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// 文件模块.
    Module,
    /// 交互输入.
    Repl,
    /// `run --code` 或命令行代码.
    Code,
}

/// 语法位置.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    /// 模块顶层.
    Top,
    /// `@Block` 的语句位置.
    BlockStmt,
    /// 普通表达式位置.
    Expr,
}

/// Actor 上下文.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ActorScope {
    /// 不在 Handler 与 Main 体内.
    Outside,
    /// `@Handler` 体.
    Handler,
    /// `@Main` 体.
    Main,
}

/// 位置上下文.
#[derive(Clone, Copy)]
struct Ctx {
    place: Place,
    /// 是否程序根, 包括顶层语句与 `@Main` 体.
    root: bool,
    actor: ActorScope,
}

impl Ctx {
    /// 程序顶层.
    const fn top() -> Self {
        Self {
            place: Place::Top,
            root: true,
            actor: ActorScope::Outside,
        }
    }

    /// 普通子表达式: 退出顶层与语句位置, 继承根与 Actor 上下文.
    const fn child(self) -> Self {
        Self {
            place: Place::Expr,
            ..self
        }
    }

    /// `@Block` 的语句位置.
    const fn block_stmt(self) -> Self {
        Self {
            place: Place::BlockStmt,
            ..self
        }
    }

    /// 函数体: 不在程序根, 也不是 Handler 或 Main.
    const fn function() -> Self {
        Self {
            place: Place::Expr,
            root: false,
            actor: ActorScope::Outside,
        }
    }

    /// `@Handler` 体.
    const fn handler_body() -> Self {
        Self {
            place: Place::Expr,
            root: false,
            actor: ActorScope::Handler,
        }
    }

    /// `@Main` 体; 属于程序根.
    const fn main_body() -> Self {
        Self {
            place: Place::Expr,
            root: true,
            actor: ActorScope::Main,
        }
    }
}

/// 校验一段程序的语义位置约束.
///
/// # Errors
///
/// 返回首个位置错误.
pub fn validate(program: &Program, source: SourceKind) -> Result<(), KslError> {
    let ctx = Ctx::top();
    for expr in program.exprs() {
        validate_expr(expr, ctx, source)?;
    }
    Ok(())
}

fn validate_expr(expr: &Expr, ctx: Ctx, source: SourceKind) -> Result<(), KslError> {
    match &expr.node {
        ExprKind::List(items) => {
            for item in items {
                validate_expr(item, ctx, source)?;
            }
        },
        ExprKind::Pair(first, second) => {
            validate_expr(first, ctx, source)?;
            validate_expr(second, ctx, source)?;
        },
        ExprKind::Call { args, .. } => {
            for arg in args {
                validate_expr(arg, ctx, source)?;
            }
        },
        ExprKind::Apply { callee, args } => {
            validate_expr(callee, ctx, source)?;
            for arg in args {
                validate_expr(arg, ctx, source)?;
            }
        },
        ExprKind::FStr(parts) => {
            for part in parts {
                if let FStrAstPart::Interp { expr, .. } = part {
                    validate_expr(expr, ctx, source)?;
                }
            }
        },
        ExprKind::Special(special) => validate_special(special, expr.span, ctx, source)?,
        ExprKind::I64(_)
        | ExprKind::I32(_)
        | ExprKind::U8(_)
        | ExprKind::U32(_)
        | ExprKind::U64(_)
        | ExprKind::F32(_)
        | ExprKind::F64(_)
        | ExprKind::Char(_)
        | ExprKind::Str(_)
        | ExprKind::Bool(_)
        | ExprKind::Nil
        | ExprKind::Ident(_) => {},
    }
    Ok(())
}

fn validate_special(
    special: &Special,
    span: Span,
    ctx: Ctx,
    source: SourceKind,
) -> Result<(), KslError> {
    use Special as S;
    match special {
        S::Let { .. }
        | S::Def(_)
        | S::Fix(_)
        | S::Data { .. }
        | S::Export { .. }
        | S::Use { .. }
        | S::Link(_)
        | S::Main { .. } => validate_definition_special(special, span, ctx, source),
        S::Continue { .. }
        | S::Spawn { .. }
        | S::Send { .. }
        | S::Receive { .. }
        | S::Join { .. }
        | S::Kill { .. }
        | S::PidSerial { .. }
        | S::SetTimer { .. }
        | S::CancelTimer { .. }
        | S::Halt
        | S::Sleep { .. } => validate_actor_special(special, span, ctx, source),
        _ => validate_expr_special(special, span, ctx, source),
    }
}

fn validate_definition_special(
    special: &Special,
    span: Span,
    ctx: Ctx,
    source: SourceKind,
) -> Result<(), KslError> {
    use Special as S;
    match special {
        S::Let { value, .. } => {
            require_definition(ctx, span)?;
            validate_expr(value, ctx.child(), source)
        },
        S::Def(def) => {
            require_definition(ctx, span)?;
            validate_expr(&def.body, Ctx::function(), source)
        },
        S::Fix(defs) => {
            require_definition(ctx, span)?;
            for def in defs {
                validate_expr(&def.body, Ctx::function(), source)?;
            }
            Ok(())
        },
        S::Data { .. } => require(ctx.place == Place::Top, ErrorKind::DataNotTopLevel, span),
        S::Export { .. } => {
            require(ctx.place == Place::Top, ErrorKind::ModuleFormNotTopLevel, span)?;
            require(
                source == SourceKind::Module,
                ErrorKind::ExportOutsideModule,
                span,
            )
        },
        S::Use { .. } => {
            require(ctx.place == Place::Top, ErrorKind::ModuleFormNotTopLevel, span)
        },
        S::Link(_) => {
            require(ctx.place == Place::Top, ErrorKind::ModuleFormNotTopLevel, span)?;
            require(source == SourceKind::Module, ErrorKind::LinkOutsideModule, span)
        },
        S::Main { body, .. } => {
            require(ctx.place == Place::Top, ErrorKind::ModuleFormNotTopLevel, span)?;
            require(source == SourceKind::Module, ErrorKind::MainOutsideModule, span)?;
            validate_expr(body, Ctx::main_body(), source)
        },
        _ => unreachable!("非定义类特殊形式"),
    }
}

fn validate_actor_special(
    special: &Special,
    span: Span,
    ctx: Ctx,
    source: SourceKind,
) -> Result<(), KslError> {
    use Special as S;
    match special {
        S::Continue { args } => {
            require_actor_main(ctx, ErrorKind::ContinueInMain, span)?;
            for arg in args {
                validate_expr(arg, ctx.child(), source)?;
            }
            Ok(())
        },
        S::Spawn { handler, args, .. } => {
            require_actor(ctx, span)?;
            validate_expr(handler, ctx.child(), source)?;
            for arg in args {
                validate_expr(arg, ctx.child(), source)?;
            }
            Ok(())
        },
        S::Send {
            pid,
            index,
            message,
        } => {
            require_actor(ctx, span)?;
            let inner = ctx.child();
            validate_expr(pid, inner, source)?;
            validate_expr(index, inner, source)?;
            validate_expr(message, inner, source)
        },
        S::Receive { index, timeout } => {
            require_actor(ctx, span)?;
            let inner = ctx.child();
            validate_expr(index, inner, source)?;
            validate_expr(timeout, inner, source)
        },
        S::Join { pid } | S::Kill { pid } | S::PidSerial { pid } => {
            require_actor(ctx, span)?;
            validate_expr(pid, ctx.child(), source)
        },
        S::SetTimer {
            pid,
            index,
            message,
            millis,
        } => {
            require_actor(ctx, span)?;
            let inner = ctx.child();
            validate_expr(pid, inner, source)?;
            validate_expr(index, inner, source)?;
            validate_expr(message, inner, source)?;
            validate_expr(millis, inner, source)
        },
        S::CancelTimer { id } => {
            require_actor(ctx, span)?;
            validate_expr(id, ctx.child(), source)
        },
        S::Halt => require_actor_main(ctx, ErrorKind::HaltInMain, span),
        S::Sleep { millis } => {
            require_actor(ctx, span)?;
            validate_expr(millis, ctx.child(), source)
        },
        _ => unreachable!("非 Actor 类特殊形式"),
    }
}

fn validate_expr_special(
    special: &Special,
    span: Span,
    ctx: Ctx,
    source: SourceKind,
) -> Result<(), KslError> {
    use Special as S;
    match special {
        S::Block(stmts) => {
            let inner = ctx.block_stmt();
            for stmt in stmts {
                validate_expr(stmt, inner, source)?;
            }
            Ok(())
        },
        S::If { cond, then, els } => {
            let inner = ctx.child();
            validate_expr(cond, inner, source)?;
            validate_expr(then, inner, source)?;
            validate_expr(els, inner, source)
        },
        S::Which { branches, fallback } => {
            let inner = ctx.child();
            for (cond, value) in branches {
                validate_expr(cond, inner, source)?;
                validate_expr(value, inner, source)?;
            }
            validate_expr(fallback, inner, source)
        },
        S::Fn { body, .. } => validate_expr(body, Ctx::function(), source),
        S::Exit { code } => {
            require(ctx.root, ErrorKind::ExitOutsideRoot, span)?;
            validate_expr(code, ctx.child(), source)
        },
        S::Handler { body, .. } => validate_expr(body, Ctx::handler_body(), source),
        S::DataGet { value, index } => {
            let inner = ctx.child();
            validate_expr(value, inner, source)?;
            validate_expr(index, inner, source)
        },
        S::Match { value, arms } => {
            let inner = ctx.child();
            validate_expr(value, inner, source)?;
            for arm in arms {
                validate_expr(&arm.body, inner, source)?;
            }
            Ok(())
        },
        S::ListOp { args, .. }
        | S::PairOp { args, .. }
        | S::DictOp { args, .. }
        | S::SetOp { args, .. }
        | S::OptionOp { args, .. } => {
            for arg in args {
                validate_expr(arg, ctx.child(), source)?;
            }
            Ok(())
        },
        S::Dict { entries } => {
            for entry in entries {
                validate_expr(entry, ctx.child(), source)?;
            }
            Ok(())
        },
        S::Set { items } => {
            for item in items {
                validate_expr(item, ctx.child(), source)?;
            }
            Ok(())
        },
        S::Assert { cond, message } => {
            let inner = ctx.child();
            validate_expr(cond, inner, source)?;
            validate_expr(message, inner, source)
        },
        S::Panic { message } | S::Todo { message } | S::Unreachable { message } => {
            validate_expr(message, ctx.child(), source)
        },
        _ => unreachable!("未分类的特殊形式"),
    }
}

/// 定义形式只允许顶层或 `@Block` 语句位置.
fn require_definition(ctx: Ctx, span: Span) -> Result<(), KslError> {
    require(
        matches!(ctx.place, Place::Top | Place::BlockStmt),
        ErrorKind::DefinitionNotAllowed,
        span,
    )
}

/// Actor 原语族只允许 `@Handler` 与 `@Main` 体.
fn require_actor(ctx: Ctx, span: Span) -> Result<(), KslError> {
    require(
        ctx.actor != ActorScope::Outside,
        ErrorKind::ActorPrimitiveOutsideHandler,
        span,
    )
}

/// `@Halt` 与 `@Continue` 在 `@Main` 体内报错.
fn require_actor_main(ctx: Ctx, main_error: ErrorKind, span: Span) -> Result<(), KslError> {
    match ctx.actor {
        ActorScope::Handler => Ok(()),
        ActorScope::Main => Err(KslError::new(main_error, span)),
        ActorScope::Outside => {
            Err(KslError::new(ErrorKind::ActorPrimitiveOutsideHandler, span))
        },
    }
}

/// 条件不成立时报错.
fn require(condition: bool, kind: ErrorKind, span: Span) -> Result<(), KslError> {
    if condition {
        Ok(())
    } else {
        Err(KslError::new(kind, span))
    }
}
