//! 特殊形式的检查规则.

mod actor;
mod binding;
mod data_get;
mod function;
mod handler;

use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::infer::InferTy;
use super::{Checker, TypeTarget, TypedKind};
use crate::ast::{Expr, ExprKind, Special};

impl Checker {
    /// 检查特殊形式并返回类型与 typed 形态.
    pub(super) fn check_special_expect(
        &mut self,
        form: &Special,
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let result = match form {
            Special::Let { .. } | Special::Block(_) => {
                self.check_binding_special(form, span, expected)?
            },
            Special::Fn { .. }
            | Special::Def(_)
            | Special::Fix(_)
            | Special::Main { .. }
            | Special::Handler { .. } => self.check_function_special(form, span, expected)?,
            Special::Continue { .. }
            | Special::Spawn { .. }
            | Special::Send { .. }
            | Special::Receive { .. }
            | Special::Join { .. }
            | Special::Kill { .. }
            | Special::PidSerial { .. }
            | Special::SetTimer { .. }
            | Special::CancelTimer { .. }
            | Special::Halt
            | Special::Sleep { .. } => self.check_actor_special(form, span, expected)?,
            Special::Data { .. } | Special::Link(_) | Special::Export { .. } => {
                self.check_module_special(form, span, expected)?
            },
            _ => self.check_value_special(form, span, expected)?,
        };
        Ok(result.expect("全部特殊形式应已 typed 化"))
    }

    fn check_binding_special(
        &mut self,
        form: &Special,
        _span: Span,
        expected: Option<&InferTy>,
    ) -> Result<Option<(InferTy, TypedKind)>, KslError> {
        use ksl_typed::{Binder, ExprKind as Typed};
        Ok(match form {
            Special::Let { target, value } => {
                let (ty, value) = self.bind_target_typed(target, value)?;
                if let Some(name) = &target.name {
                    self.bind_value(name, ty.clone())?;
                }
                Some((
                    InferTy::Nil,
                    Typed::Let {
                        target: Binder {
                            name: target.name.as_ref().map(|name| name.node.clone()),
                            ty: InferTy::from_concrete(&ty),
                        },
                        value: Box::new(value),
                    },
                ))
            },
            Special::Block(stmts) => {
                let result = self.with_scope(|checker| {
                    let Some((last, init)) = stmts.split_last() else {
                        return Ok((InferTy::Nil, Typed::Block(Vec::new())));
                    };
                    let mut typed = Vec::with_capacity(stmts.len());
                    for stmt in init {
                        typed.push(checker.check_value_used_typed(stmt)?);
                    }
                    let last = checker.check_expr_expect(last, expected)?;
                    let ty = last.ty.clone();
                    typed.push(last);
                    Ok((ty, Typed::Block(typed)))
                })?;
                Some(result)
            },
            _ => unreachable!("特殊形式分组不匹配"),
        })
    }

    fn check_function_special(
        &mut self,
        form: &Special,
        span: Span,
        _expected: Option<&InferTy>,
    ) -> Result<Option<(InferTy, TypedKind)>, KslError> {
        Ok(match form {
            Special::Fn { params, ret, body } => {
                let (ty, kind) = self.check_fn(params, ret, body)?;
                Some((ty, kind))
            },
            Special::Def(def) => {
                let (ty, kind) = self.check_def(def)?;
                Some((ty, kind))
            },
            Special::Fix(defs) => {
                let (ty, kind) = self.check_fix(defs)?;
                Some((ty, kind))
            },
            Special::Main { params, ret, body } => {
                let (handler, body) = self.check_main(params, ret, body, span)?;
                Some(Self::typed_main(&handler, params, body))
            },
            Special::Handler { params, ret, body } => {
                let (ty, kind) = self.check_handler(params, ret, body)?;
                Some((ty, kind))
            },
            _ => unreachable!("特殊形式分组不匹配"),
        })
    }

    fn check_actor_special(
        &mut self,
        form: &Special,
        span: Span,
        _expected: Option<&InferTy>,
    ) -> Result<Option<(InferTy, TypedKind)>, KslError> {
        Ok(match form {
            Special::Continue { args } => {
                let (ty, kind) = self.check_continue(args, span)?;
                Some((ty, kind))
            },
            Special::Spawn {
                handler,
                args,
                detached,
            } => {
                let (ty, kind) = self.check_spawn(handler, args, *detached, span)?;
                Some((ty, kind))
            },
            Special::Send {
                pid,
                index,
                message,
            } => {
                let (ty, kind) = self.check_send(pid, index, message, span)?;
                Some((ty, kind))
            },
            Special::Receive { index, timeout } => {
                let (ty, kind) = self.check_receive(index, timeout, span)?;
                Some((ty, kind))
            },
            Special::Join { pid } => {
                self.actor();
                let (ty, kind) = self.check_join(pid)?;
                Some((ty, kind))
            },
            Special::Kill { pid } => {
                self.actor();
                let (ty, kind) = self.check_kill(pid)?;
                Some((ty, kind))
            },
            Special::PidSerial { pid } => {
                self.actor();
                let (ty, kind) = self.check_pid_serial(pid)?;
                Some((ty, kind))
            },
            Special::SetTimer {
                pid,
                index,
                message,
                millis,
            } => {
                let (ty, kind) = self.check_set_timer(pid, index, message, millis, span)?;
                Some((ty, kind))
            },
            Special::CancelTimer { id } => {
                let (ty, kind) = self.check_cancel_timer(id, span)?;
                Some((ty, kind))
            },
            Special::Halt => Some(Self::check_halt(self)),
            Special::Sleep { millis } => {
                let (ty, kind) = self.check_sleep(millis, span)?;
                Some((ty, kind))
            },
            _ => unreachable!("特殊形式分组不匹配"),
        })
    }

    fn check_module_special(
        &mut self,
        form: &Special,
        span: Span,
        _expected: Option<&InferTy>,
    ) -> Result<Option<(InferTy, TypedKind)>, KslError> {
        use ksl_typed::ExprKind as Typed;
        Ok(match form {
            Special::Data { name, .. } => {
                let Some(TypeTarget::Data(full)) = self.context.type_env.resolve(&name.node)
                else {
                    return Err(KslError::unknown_type(&name.node, name.span));
                };
                let info = self.required_data(&full, name.span)?;
                let variants = info
                    .variants
                    .iter()
                    .map(|(variant, fields)| ksl_typed::VariantDecl {
                        name: variant.clone(),
                        fields: fields.iter().map(InferTy::from_concrete).collect(),
                    })
                    .collect();
                Some((
                    InferTy::Nil,
                    Typed::Data(ksl_typed::DataDecl {
                        name: full,
                        variants,
                    }),
                ))
            },
            Special::Link(_) => Some((InferTy::Nil, Typed::Link)),
            Special::Export { .. } | Special::Use { .. } => {
                return Err(KslError::new(ErrorKind::ModuleFormNotResolved, span));
            },
            _ => unreachable!("特殊形式分组不匹配"),
        })
    }

    fn check_value_special(
        &mut self,
        form: &Special,
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<Option<(InferTy, TypedKind)>, KslError> {
        use ksl_typed::ExprKind as Typed;
        Ok(match form {
            Special::If { cond, then, els } => {
                let cond = self.check_expr_expect(cond, Some(&InferTy::Bool))?;
                let result = expected
                    .cloned()
                    .unwrap_or_else(|| self.infer.fresh(span, "@If 结果"));
                let then = self.check_expr_expect(then, Some(&result))?;
                self.infer.unify(&result, &then.ty, then.span)?;
                let els = self.check_expr_expect(els, Some(&result))?;
                self.infer.unify(&result, &els.ty, els.span)?;
                let then_never = matches!(self.infer.shallow(&then.ty), InferTy::Never);
                let els_never = matches!(self.infer.shallow(&els.ty), InferTy::Never);
                let ty = if then_never && els_never {
                    InferTy::Never
                } else {
                    result
                };
                Some((
                    ty,
                    Typed::If {
                        cond: Box::new(cond),
                        then: Box::new(then),
                        els: Box::new(els),
                    },
                ))
            },
            Special::Which { branches, fallback } => {
                let (ty, branches, fallback) =
                    self.check_which_typed(branches, fallback, expected, span)?;
                Some((ty, Typed::Which { branches, fallback }))
            },
            Special::Exit { code } => {
                let code = self.check_expr_expect(code, Some(&InferTy::U8))?;
                Some((
                    InferTy::Never,
                    Typed::Exit {
                        code: Box::new(code),
                    },
                ))
            },
            Special::Assert { cond, message } => {
                let cond = self.check_expr_expect(cond, Some(&InferTy::Bool))?;
                let message = self.check_expr_expect(message, Some(&InferTy::Str))?;
                Some((
                    InferTy::Nil,
                    Typed::Assert {
                        cond: Box::new(cond),
                        message: Box::new(message),
                    },
                ))
            },
            kind @ (Special::Panic { .. }
            | Special::Todo { .. }
            | Special::Unreachable { .. }) => self.check_fail_special(kind, span)?,
            Special::DataGet { value, index } => {
                let (ty, value, index) = self.check_data_get(value, index)?;
                Some((
                    ty,
                    Typed::DataGet {
                        value: Box::new(value),
                        index,
                    },
                ))
            },
            Special::ListOp { op, args } => {
                Some(self.check_list_op(*op, args, span, expected)?)
            },
            Special::PairOp { op, args } => {
                Some(self.check_pair_op(*op, args, span, expected)?)
            },
            Special::Dict { entries } => {
                Some(self.check_dict_literal(entries, span, expected)?)
            },
            Special::Set { items } => Some(self.check_set_literal(items, span, expected)?),
            Special::DictOp { op, args } => {
                Some(self.check_dict_op(*op, args, span, expected)?)
            },
            Special::SetOp { op, args } => Some(self.check_set_op(*op, args, span, expected)?),
            Special::OptionOp { op, args } => {
                Some(self.check_option_op(*op, args, span, expected)?)
            },
            Special::Match { value, arms } => {
                let (ty, kind) = self.check_match_typed(value, arms, expected)?;
                Some((ty, kind))
            },
            _ => unreachable!("特殊形式分组不匹配"),
        })
    }

    fn check_fail_special(
        &mut self,
        form: &Special,
        _span: Span,
    ) -> Result<Option<(InferTy, TypedKind)>, KslError> {
        use ksl_typed::ExprKind as Typed;
        let (Special::Panic { message }
        | Special::Todo { message }
        | Special::Unreachable { message }) = form
        else {
            unreachable!("非失败类特殊形式");
        };
        let message = Box::new(self.check_expr_expect(message, Some(&InferTy::Str))?);
        let kind = match form {
            Special::Panic { .. } => Typed::Panic { message },
            Special::Todo { .. } => Typed::Todo { message },
            Special::Unreachable { .. } => Typed::Unreachable { message },
            _ => unreachable!("非失败类特殊形式"),
        };
        Ok(Some((InferTy::Never, kind)))
    }

    /// 从表达式取索引; 索引必须是带 `u8` 后缀的整数字面量.
    fn literal_index(expr: &Expr, what: &'static str) -> Result<usize, KslError> {
        match &expr.node {
            ExprKind::U8(value) => Ok(usize::from(*value)),
            ExprKind::I64(_) => Err(KslError::with_subject(
                ErrorKind::TypeMismatch {
                    expected: Type::U8,
                    found: Type::I64,
                },
                expr.span,
                what,
            )),
            _ => Err(KslError::with_subject(
                ErrorKind::U8LiteralRequired,
                expr.span,
                what,
            )),
        }
    }
}
