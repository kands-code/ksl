//! 把推断类型解析为具体类型.

use ksl_base::types::Type;
use ksl_diagnostics::error::KslError;
use ksl_typed::{
    Binder, CheckedBinder, CheckedDataDecl, CheckedDef, CheckedExpr, CheckedMatchArm,
    CheckedPattern, CheckedProgram, CheckedVariantDecl, DataDecl, Def, Expr, ExprKind,
    FStrPart, MatchArm, Pattern, Program,
};

use super::infer::{InferCtx, InferTy};

/// 把推断类型解析为具体类型.
pub(crate) fn finalize_program(
    program: Program<InferTy>,
    infer: &mut InferCtx,
) -> Result<CheckedProgram, KslError> {
    Ok(Program {
        stmts: program
            .stmts
            .into_iter()
            .map(|expr| finalize_expr(expr, infer))
            .collect::<Result<_, _>>()?,
        tail: program
            .tail
            .map(|expr| finalize_expr(expr, infer))
            .transpose()?,
        main: program
            .main
            .map(|expr| finalize_expr(expr, infer))
            .transpose()?,
    })
}

fn finalize_expr(expr: Expr<InferTy>, infer: &mut InferCtx) -> Result<CheckedExpr, KslError> {
    let ty = infer.finalize(&expr.ty)?;
    let kind = match expr.kind {
        ExprKind::I64(value) => ExprKind::I64(value),
        ExprKind::I32(value) => ExprKind::I32(value),
        ExprKind::U8(value) => ExprKind::U8(value),
        ExprKind::U32(value) => ExprKind::U32(value),
        ExprKind::U64(value) => ExprKind::U64(value),
        ExprKind::F32(value) => ExprKind::F32(value),
        ExprKind::F64(value) => ExprKind::F64(value),
        ExprKind::Char(value) => ExprKind::Char(value),
        ExprKind::Str(value) => ExprKind::Str(value),
        ExprKind::Bool(value) => ExprKind::Bool(value),
        ExprKind::Nil => ExprKind::Nil,
        ExprKind::Ident(name) => ExprKind::Ident(name),
        ExprKind::List(items) => ExprKind::List(finalize_exprs(items, infer)?),
        ExprKind::Pair(first, second) => finalize_pair(*first, *second, infer)?,
        ExprKind::FStr(parts) => finalize_fstr(parts, infer)?,
        ExprKind::Call { callee, args } => ExprKind::Call {
            callee,
            args: finalize_exprs(args, infer)?,
        },
        ExprKind::Apply { callee, args } => finalize_apply(*callee, args, infer)?,
        ExprKind::Which { branches, fallback } => finalize_which(branches, *fallback, infer)?,
        ExprKind::DataGet { value, index } => finalize_data_get(*value, index, infer)?,
        kind @ (ExprKind::ListOp { .. }
        | ExprKind::PairOp { .. }
        | ExprKind::DictOp { .. }
        | ExprKind::SetOp { .. }
        | ExprKind::OptionOp { .. }) => finalize_op_expr(kind, infer)?,
        ExprKind::DictLiteral(entries) => {
            ExprKind::DictLiteral(finalize_exprs(entries, infer)?)
        },
        ExprKind::SetLiteral(items) => ExprKind::SetLiteral(finalize_exprs(items, infer)?),
        ExprKind::Assert { cond, message } => finalize_assert(*cond, *message, infer)?,
        kind @ (ExprKind::Panic { .. }
        | ExprKind::Todo { .. }
        | ExprKind::Unreachable { .. }) => finalize_fail(kind, infer)?,
        ExprKind::Def(def) => ExprKind::Def(finalize_def(def, infer)?),
        ExprKind::Fix(defs) => finalize_fix(defs, infer)?,
        ExprKind::Data(decl) => ExprKind::Data(finalize_data(decl, infer)?),
        ExprKind::Link => ExprKind::Link,
        ExprKind::Match { value, arms } => finalize_match(*value, arms, infer)?,
        kind @ (ExprKind::Fn { .. } | ExprKind::Handler { .. } | ExprKind::Main { .. }) => {
            finalize_callable(kind, infer)?
        },
        ExprKind::Spawn {
            handler,
            args,
            detached,
        } => finalize_spawn(*handler, args, detached, infer)?,
        ExprKind::Send {
            pid,
            index,
            message,
        } => finalize_send(*pid, index, *message, infer)?,
        ExprKind::Receive { index, timeout } => finalize_receive(index, *timeout, infer)?,
        ExprKind::SetTimer {
            pid,
            index,
            message,
            millis,
        } => finalize_set_timer(*pid, index, *message, *millis, infer)?,
        ExprKind::Halt => ExprKind::Halt,
        ExprKind::Continue { args } => ExprKind::Continue {
            args: finalize_exprs(args, infer)?,
        },
        kind @ (ExprKind::Join { .. }
        | ExprKind::Kill { .. }
        | ExprKind::PidSerial { .. }
        | ExprKind::CancelTimer { .. }
        | ExprKind::Sleep { .. }
        | ExprKind::Exit { .. }) => finalize_single_value(kind, infer)?,
        ExprKind::Let { target, value } => finalize_let(target, *value, infer)?,
        ExprKind::Block(stmts) => ExprKind::Block(finalize_exprs(stmts, infer)?),
        ExprKind::If { cond, then, els } => finalize_if(*cond, *then, *els, infer)?,
    };
    Ok(CheckedExpr {
        kind,
        span: expr.span,
        ty,
    })
}

fn finalize_op_expr(
    kind: ExprKind<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(match kind {
        ExprKind::ListOp { op, args } => ExprKind::ListOp {
            op,
            args: finalize_exprs(args, infer)?,
        },
        ExprKind::PairOp { op, args } => ExprKind::PairOp {
            op,
            args: finalize_exprs(args, infer)?,
        },
        ExprKind::DictOp { op, args } => ExprKind::DictOp {
            op,
            args: finalize_exprs(args, infer)?,
        },
        ExprKind::SetOp { op, args } => ExprKind::SetOp {
            op,
            args: finalize_exprs(args, infer)?,
        },
        ExprKind::OptionOp { op, args } => ExprKind::OptionOp {
            op,
            args: finalize_exprs(args, infer)?,
        },
        _ => unreachable!("finalize_op_expr 只处理操作类表达式"),
    })
}

fn finalize_callable(
    kind: ExprKind<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(match kind {
        ExprKind::Fn { params, ret, body } => {
            let (params, ret, body) = finalize_fn_like(params, &ret, *body, infer)?;
            ExprKind::Fn { params, ret, body }
        },
        ExprKind::Handler { params, ret, body } => {
            let (params, ret, body) = finalize_fn_like(params, &ret, *body, infer)?;
            ExprKind::Handler { params, ret, body }
        },
        ExprKind::Main { params, ret, body } => {
            let (params, ret, body) = finalize_fn_like(params, &ret, *body, infer)?;
            ExprKind::Main { params, ret, body }
        },
        _ => unreachable!("finalize_callable 只处理函数类表达式"),
    })
}

fn finalize_fail(
    kind: ExprKind<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(match kind {
        ExprKind::Panic { message } => ExprKind::Panic {
            message: finalize_boxed(*message, infer)?,
        },
        ExprKind::Todo { message } => ExprKind::Todo {
            message: finalize_boxed(*message, infer)?,
        },
        ExprKind::Unreachable { message } => ExprKind::Unreachable {
            message: finalize_boxed(*message, infer)?,
        },
        _ => unreachable!("finalize_fail 只处理失败表达式"),
    })
}

fn finalize_single_value(
    kind: ExprKind<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(match kind {
        ExprKind::Join { pid } => ExprKind::Join {
            pid: finalize_boxed(*pid, infer)?,
        },
        ExprKind::Kill { pid } => ExprKind::Kill {
            pid: finalize_boxed(*pid, infer)?,
        },
        ExprKind::PidSerial { pid } => ExprKind::PidSerial {
            pid: finalize_boxed(*pid, infer)?,
        },
        ExprKind::CancelTimer { id } => ExprKind::CancelTimer {
            id: finalize_boxed(*id, infer)?,
        },
        ExprKind::Sleep { millis } => ExprKind::Sleep {
            millis: finalize_boxed(*millis, infer)?,
        },
        ExprKind::Exit { code } => ExprKind::Exit {
            code: finalize_boxed(*code, infer)?,
        },
        _ => unreachable!("finalize_single_value 只处理单值表达式"),
    })
}

fn finalize_boxed(
    expr: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<Box<CheckedExpr>, KslError> {
    Ok(Box::new(finalize_expr(expr, infer)?))
}

fn finalize_pair(
    first: Expr<InferTy>,
    second: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Pair(
        finalize_boxed(first, infer)?,
        finalize_boxed(second, infer)?,
    ))
}

fn finalize_fstr(
    parts: Vec<FStrPart<InferTy>>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::FStr(
        parts
            .into_iter()
            .map(|part| match part {
                FStrPart::Text(text) => Ok(FStrPart::Text(text)),
                FStrPart::Interp { value, spec } => Ok(FStrPart::Interp {
                    value: Box::new(finalize_expr(*value, infer)?),
                    spec,
                }),
            })
            .collect::<Result<_, KslError>>()?,
    ))
}

fn finalize_apply(
    callee: Expr<InferTy>,
    args: Vec<Expr<InferTy>>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Apply {
        callee: finalize_boxed(callee, infer)?,
        args: finalize_exprs(args, infer)?,
    })
}

fn finalize_which(
    branches: Vec<(Expr<InferTy>, Expr<InferTy>)>,
    fallback: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Which {
        branches: branches
            .into_iter()
            .map(|(cond, value)| {
                Ok((finalize_expr(cond, infer)?, finalize_expr(value, infer)?))
            })
            .collect::<Result<_, KslError>>()?,
        fallback: finalize_boxed(fallback, infer)?,
    })
}

fn finalize_data_get(
    value: Expr<InferTy>,
    index: u8,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::DataGet {
        value: finalize_boxed(value, infer)?,
        index,
    })
}

fn finalize_assert(
    cond: Expr<InferTy>,
    message: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Assert {
        cond: finalize_boxed(cond, infer)?,
        message: finalize_boxed(message, infer)?,
    })
}

fn finalize_fn_like(
    params: Vec<Binder<InferTy>>,
    ret: &InferTy,
    body: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<(Vec<CheckedBinder>, Type, Box<CheckedExpr>), KslError> {
    Ok((
        finalize_binders(params, infer)?,
        infer.finalize(ret)?,
        finalize_boxed(body, infer)?,
    ))
}

fn finalize_fix(
    defs: Vec<Def<InferTy>>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Fix(
        defs.into_iter()
            .map(|def| finalize_def(def, infer))
            .collect::<Result<_, _>>()?,
    ))
}

fn finalize_match(
    value: Expr<InferTy>,
    arms: Vec<MatchArm<InferTy>>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Match {
        value: finalize_boxed(value, infer)?,
        arms: arms
            .into_iter()
            .map(|arm| finalize_match_arm(arm, infer))
            .collect::<Result<_, _>>()?,
    })
}

fn finalize_spawn(
    handler: Expr<InferTy>,
    args: Vec<Expr<InferTy>>,
    detached: bool,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Spawn {
        handler: finalize_boxed(handler, infer)?,
        args: finalize_exprs(args, infer)?,
        detached,
    })
}

fn finalize_send(
    pid: Expr<InferTy>,
    index: u8,
    message: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Send {
        pid: finalize_boxed(pid, infer)?,
        index,
        message: finalize_boxed(message, infer)?,
    })
}

fn finalize_receive(
    index: u8,
    timeout: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Receive {
        index,
        timeout: finalize_boxed(timeout, infer)?,
    })
}

fn finalize_set_timer(
    pid: Expr<InferTy>,
    index: u8,
    message: Expr<InferTy>,
    millis: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::SetTimer {
        pid: finalize_boxed(pid, infer)?,
        index,
        message: finalize_boxed(message, infer)?,
        millis: finalize_boxed(millis, infer)?,
    })
}

fn finalize_let(
    target: Binder<InferTy>,
    value: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::Let {
        target: CheckedBinder {
            name: target.name,
            ty: infer.finalize(&target.ty)?,
        },
        value: finalize_boxed(value, infer)?,
    })
}

fn finalize_if(
    cond: Expr<InferTy>,
    then: Expr<InferTy>,
    els: Expr<InferTy>,
    infer: &mut InferCtx,
) -> Result<ExprKind<Type>, KslError> {
    Ok(ExprKind::If {
        cond: finalize_boxed(cond, infer)?,
        then: finalize_boxed(then, infer)?,
        els: finalize_boxed(els, infer)?,
    })
}

fn finalize_exprs(
    exprs: Vec<Expr<InferTy>>,
    infer: &mut InferCtx,
) -> Result<Vec<CheckedExpr>, KslError> {
    exprs
        .into_iter()
        .map(|expr| finalize_expr(expr, infer))
        .collect()
}

fn finalize_binders(
    binders: Vec<Binder<InferTy>>,
    infer: &mut InferCtx,
) -> Result<Vec<CheckedBinder>, KslError> {
    binders
        .into_iter()
        .map(|binder| {
            Ok(CheckedBinder {
                name: binder.name,
                ty: infer.finalize(&binder.ty)?,
            })
        })
        .collect()
}

fn finalize_def(def: Def<InferTy>, infer: &mut InferCtx) -> Result<CheckedDef, KslError> {
    Ok(CheckedDef {
        name: def.name,
        params: finalize_binders(def.params, infer)?,
        ret: infer.finalize(&def.ret)?,
        body: Box::new(finalize_expr(*def.body, infer)?),
    })
}

fn finalize_data(
    decl: DataDecl<InferTy>,
    infer: &mut InferCtx,
) -> Result<CheckedDataDecl, KslError> {
    Ok(CheckedDataDecl {
        name: decl.name,
        variants: decl
            .variants
            .into_iter()
            .map(|variant| {
                Ok(CheckedVariantDecl {
                    name: variant.name,
                    fields: variant
                        .fields
                        .into_iter()
                        .map(|field| infer.finalize(&field))
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<_, _>>()?,
    })
}

fn finalize_match_arm(
    arm: MatchArm<InferTy>,
    infer: &mut InferCtx,
) -> Result<CheckedMatchArm, KslError> {
    Ok(CheckedMatchArm {
        pattern: finalize_pattern(arm.pattern, infer)?,
        body: Box::new(finalize_expr(*arm.body, infer)?),
    })
}

fn finalize_pattern(
    pattern: Pattern<InferTy>,
    infer: &mut InferCtx,
) -> Result<CheckedPattern, KslError> {
    Ok(match pattern {
        Pattern::Wildcard => Pattern::Wildcard,
        Pattern::Variant {
            qualifier,
            name,
            fields,
        } => Pattern::Variant {
            qualifier,
            name,
            fields: fields
                .into_iter()
                .map(|field| {
                    field
                        .map(|binder| {
                            Ok(CheckedBinder {
                                name: binder.name,
                                ty: infer.finalize(&binder.ty)?,
                            })
                        })
                        .transpose()
                })
                .collect::<Result<_, _>>()?,
        },
    })
}
