//! 表达式检查与调用规则.

use super::data::check_format_spec;
use super::infer::{InferCtx, InferTy};
use super::types::Type;
use super::{Checker, TypeTable, is_printable};
use crate::ast::{Expr, ExprKind, Head, NodeKey, Special};
use crate::builtin;
use crate::error::{ErrorKind, KslError};
use crate::source::Span;
use crate::symbol::{intern, resolve};

impl Checker {
    pub(super) fn check_program(
        &mut self,
        program: &crate::ast::Program,
    ) -> Result<(), KslError> {
        self.check_program_with_tail(program).map(|_tail| ())
    }

    pub(super) fn check_program_with_tail(
        &mut self,
        program: &crate::ast::Program,
    ) -> Result<Option<Type>, KslError> {
        self.infer_types.clear();
        self.context.types.clear();
        self.infer = InferCtx::default();
        self.record_data_types(program)?;
        for stmt in &program.stmts {
            let ty = self.check_expr(stmt)?;
            // 语句值处置规则: 非 nil 值必须显式绑定或抛弃; 底类型不出值, 放行.
            if !matches!(self.infer.shallow(&ty), InferTy::Nil | InferTy::Never) {
                let found = self.infer.finalize(&ty)?;
                return Err(KslError::with_subject(
                    ErrorKind::UnusedValue { found },
                    stmt.span,
                    "语句值",
                ));
            }
        }
        let tail = program
            .tail
            .as_ref()
            .map(|tail| self.check_expr(tail))
            .transpose()?;
        let tail = tail.map(|tail| self.infer.finalize(&tail)).transpose()?;
        let raw = std::mem::take(&mut self.infer_types);
        let mut types = TypeTable::new();
        for (key, ty) in raw {
            types.insert(key, self.infer.finalize(&ty)?);
        }
        self.context.types = types;
        // 本轮的变量绑定已全部解析; 不把统一表带进下一次会话克隆.
        self.infer = InferCtx::default();
        Ok(tail)
    }

    /// 检查表达式, 不带期望.
    pub(super) fn check_expr(&mut self, expr: &Expr) -> Result<InferTy, KslError> {
        self.check_expr_expect(expr, None)
    }

    /// 记录表达式的最终类型; 失败路径不记录.
    pub(super) fn record_type(&mut self, key: NodeKey, ty: InferTy) -> InferTy {
        self.infer_types.insert(key, ty.clone());
        ty
    }

    /// 记录 `@Data` 类型名, 变体构造器签名与字段类型.
    pub(super) fn record_data_types(
        &mut self,
        program: &crate::ast::Program,
    ) -> Result<(), KslError> {
        for stmt in program.exprs() {
            let ExprKind::Special(Special::Data { name, variants }) = &stmt.node else {
                continue;
            };
            let data_ty = Type::Data(name.node);
            self.infer_types
                .insert(name.key(), InferTy::from_concrete(&data_ty));
            for variant in variants {
                let mut fields = Vec::with_capacity(variant.fields.len());
                for field in &variant.fields {
                    fields.push(self.type_from_expr(field)?);
                }
                let ctor = Type::Fn(fields, Box::new(data_ty.clone()));
                self.infer_types
                    .insert(variant.name.key(), InferTy::from_concrete(&ctor));
            }
        }
        Ok(())
    }

    /// 按期望类型检查表达式.
    ///
    /// 期望来自类型标注, 函数签名或构造器的类型参数. 整数字面量严格按
    /// 后缀定型; 列表元素共享元素类型变量; 内置泛型构造器与特殊形式
    /// 通过统一解析未定类型. 无期望且无上下文时可以推断的类型由元素或
    /// 分支共同决定, 仍无法定型时报错.
    pub(super) fn check_expr_expect(
        &mut self,
        expr: &Expr,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        // never 是底类型, 不作为子表达式的期望; 汇合由 join 处理.
        let expected = expected.filter(|ty| !matches!(**ty, InferTy::Never));
        let found = match &expr.node {
            ExprKind::I64(_) => self.check_int_literal(expected, expr.span)?,
            ExprKind::U8(_) => match expected {
                Some(InferTy::U8 | InferTy::Var(_)) | None => InferTy::U8,
                Some(other) => {
                    return Err(self.infer.mismatch_error(other, &InferTy::U8, expr.span));
                },
            },
            ExprKind::U64(_) => match expected {
                Some(InferTy::U64 | InferTy::Var(_)) | None => InferTy::U64,
                Some(other) => {
                    return Err(self.infer.mismatch_error(other, &InferTy::U64, expr.span));
                },
            },
            ExprKind::F64(_) => match expected {
                Some(InferTy::F64 | InferTy::Var(_)) | None => InferTy::F64,
                Some(other) => {
                    return Err(self.infer.mismatch_error(other, &InferTy::F64, expr.span));
                },
            },
            ExprKind::Char(_) => InferTy::Char,
            ExprKind::Str(_) => InferTy::Str,
            ExprKind::Bool(_) => InferTy::Bool,
            ExprKind::Nil => InferTy::Nil,
            ExprKind::FStr(parts) => self.check_fstr(parts)?,
            ExprKind::Ident(name) => self
                .env
                .lookup(*name)
                .or_else(|| builtin::signature(*name))
                .map(InferTy::from_concrete)
                .ok_or_else(|| {
                    KslError::with_subject(
                        ErrorKind::UnknownName,
                        expr.span,
                        resolve(*name).to_string(),
                    )
                })?,
            ExprKind::List(items) => {
                let elem = match expected {
                    Some(InferTy::List(elem)) => (**elem).clone(),
                    _ => self.infer.fresh(expr.span, "列表元素"),
                };
                for item in items {
                    let item_ty = self.check_expr_expect(item, Some(&elem))?;
                    self.infer.unify(&elem, &item_ty, item.span)?;
                }
                let list = InferTy::List(Box::new(elem));
                if let Some(expected) = expected {
                    self.infer.unify(expected, &list, expr.span)?;
                }
                list
            },
            ExprKind::Pair(first, second) => {
                let (a, b) = match expected {
                    Some(InferTy::Pair(a, b)) => ((**a).clone(), (**b).clone()),
                    _ => (
                        self.infer.fresh(first.span, "Pair 首分量"),
                        self.infer.fresh(second.span, "Pair 次分量"),
                    ),
                };
                let first_ty = self.check_expr_expect(first, Some(&a))?;
                self.infer.unify(&a, &first_ty, first.span)?;
                let second_ty = self.check_expr_expect(second, Some(&b))?;
                self.infer.unify(&b, &second_ty, second.span)?;
                let pair = InferTy::Pair(Box::new(a), Box::new(b));
                if let Some(expected) = expected {
                    self.infer.unify(expected, &pair, expr.span)?;
                }
                pair
            },
            ExprKind::Call { head, args } => {
                // 内置泛型构造器的类型参数可由期望给定, 如 `Option/Some`.
                if let Some(ty) = self.check_builtin_ctor(head, args, expr.span, expected)? {
                    return Ok(self.record_type(expr.key(), ty));
                }
                self.check_call(head, args, expr.span)?
            },
            ExprKind::Special(form) => {
                let ty = self.check_special_expect(form, expr.span, expected)?;
                if let Some(expected) = expected {
                    self.infer.unify(expected, &ty, expr.span)?;
                }
                return Ok(self.record_type(expr.key(), ty));
            },
        };
        if let Some(expected) = expected {
            self.infer.unify(expected, &found, expr.span)?;
        }
        Ok(self.record_type(expr.key(), found))
    }

    /// 格式化字符串: 插值先解析为具体类型, 再校验可打印性与格式参数.
    pub(super) fn check_fstr(
        &mut self,
        parts: &[crate::ast::FStrAstPart],
    ) -> Result<InferTy, KslError> {
        for part in parts {
            if let crate::ast::FStrAstPart::Interp { expr, spec } = part {
                let ty = self.check_expr(expr)?;
                let ty = self.infer.finalize(&ty)?;
                if !is_printable(&ty) {
                    return Err(KslError::new(
                        ErrorKind::UnprintableType { found: ty },
                        expr.span,
                    ));
                }
                if let Some(spec) = spec {
                    check_format_spec(spec, &ty, expr.span)?;
                }
            }
        }
        Ok(InferTy::Str)
    }

    /// 整数字面量: 无后缀即 i64; u8 必须显式写 `u` 后缀.
    pub(super) fn check_int_literal(
        &mut self,
        expected: Option<&InferTy>,
        span: Span,
    ) -> Result<InferTy, KslError> {
        match expected {
            Some(InferTy::I64 | InferTy::Var(_)) | None => Ok(InferTy::I64),
            Some(other) => Err(self.infer.mismatch_error(other, &InferTy::I64, span)),
        }
    }

    /// 内置泛型构造器: `Option/Some`, `Option/None`, `Result/Ok`, `Result/Err`.
    /// 未定的类型参数从期望取得; 无期望且参数无法由实参确定时报错.
    /// 返回 None 表示不是内置泛型构造器, 走常规调用路径.
    pub(super) fn check_builtin_ctor(
        &mut self,
        head: &Head,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<Option<InferTy>, KslError> {
        let Some(qualifier) = &head.qualifier else {
            return Ok(None);
        };
        let variant = head.name.node;
        let is_option = qualifier.node == intern("Option");
        let is_result = qualifier.node == intern("Result");
        if !is_option && !is_result {
            return Ok(None);
        }
        let want = |count: usize| -> Result<(), KslError> {
            if args.len() != count {
                return Err(KslError::new(
                    ErrorKind::ArityMismatch {
                        expected: count,
                        found: args.len(),
                    },
                    span,
                ));
            }
            Ok(())
        };
        if is_option {
            match variant {
                v if v == intern("Some") => {
                    want(1)?;
                    let elem = match expected {
                        Some(InferTy::OptionT(inner)) => (**inner).clone(),
                        _ => self.infer.fresh(span, "Option/Some 元素"),
                    };
                    let option = InferTy::OptionT(Box::new(elem.clone()));
                    if let Some(expected) = expected {
                        self.infer.unify(expected, &option, span)?;
                    }
                    let arg_ty = self.check_expr_expect(&args[0], Some(&elem))?;
                    self.infer.unify(&elem, &arg_ty, args[0].span)?;
                    Ok(Some(option))
                },
                v if v == intern("None") => {
                    want(0)?;
                    let elem = match expected {
                        Some(InferTy::OptionT(inner)) => (**inner).clone(),
                        _ => self.infer.fresh(span, "Option/None 元素"),
                    };
                    let option = InferTy::OptionT(Box::new(elem));
                    if let Some(expected) = expected {
                        self.infer.unify(expected, &option, span)?;
                    }
                    Ok(Some(option))
                },
                _ => Err(KslError::with_subject(
                    ErrorKind::UnknownName,
                    head.name.span,
                    format!("Option/{}", resolve(variant)),
                )),
            }
        } else {
            match variant {
                v if v == intern("Ok") => {
                    want(1)?;
                    let (ok, err) = match expected {
                        Some(InferTy::ResultT(ok, err)) => ((**ok).clone(), (**err).clone()),
                        _ => (
                            self.infer.fresh(span, "Result/Ok 成功类型"),
                            self.infer.fresh(span, "Result/Ok 错误类型"),
                        ),
                    };
                    let result = InferTy::ResultT(Box::new(ok.clone()), Box::new(err));
                    if let Some(expected) = expected {
                        self.infer.unify(expected, &result, span)?;
                    }
                    let arg_ty = self.check_expr_expect(&args[0], Some(&ok))?;
                    self.infer.unify(&ok, &arg_ty, args[0].span)?;
                    Ok(Some(result))
                },
                v if v == intern("Err") => {
                    want(1)?;
                    let (ok, err) = match expected {
                        Some(InferTy::ResultT(ok, err)) => ((**ok).clone(), (**err).clone()),
                        _ => (
                            self.infer.fresh(span, "Result/Err 成功类型"),
                            self.infer.fresh(span, "Result/Err 错误类型"),
                        ),
                    };
                    let result = InferTy::ResultT(Box::new(ok), Box::new(err.clone()));
                    if let Some(expected) = expected {
                        self.infer.unify(expected, &result, span)?;
                    }
                    let arg_ty = self.check_expr_expect(&args[0], Some(&err))?;
                    self.infer.unify(&err, &arg_ty, args[0].span)?;
                    Ok(Some(result))
                },
                _ => Err(KslError::with_subject(
                    ErrorKind::UnknownName,
                    head.name.span,
                    format!("Result/{}", resolve(variant)),
                )),
            }
        }
    }

    /// 普通调用: 解析头部, 校验实参个数与类型.
    pub(super) fn check_call(
        &mut self,
        head: &Head,
        args: &[Expr],
        span: Span,
    ) -> Result<InferTy, KslError> {
        let name = &head.name.node;
        let callee = if let Some(qualifier) = &head.qualifier {
            // 受限名字: Data 构造器调用.
            let Some(info) = self.context.data.types.get(&qualifier.node) else {
                return Err(KslError::with_subject(
                    ErrorKind::UnknownType,
                    qualifier.span,
                    resolve(qualifier.node).to_string(),
                ));
            };
            let Some(fields) = info.variants.get(name) else {
                return Err(KslError::with_subject(
                    ErrorKind::UnknownName,
                    head.name.span,
                    format!("{}/{}", qualifier.node, name),
                ));
            };
            (fields.clone(), Type::Data(qualifier.node))
        } else if let Some(ty) = self.env.lookup(*name).cloned() {
            match ty {
                Type::Fn(params, ret) => (params, *ret),
                other => {
                    return Err(KslError::with_subject(
                        ErrorKind::NotCallable { found: other },
                        head.name.span,
                        resolve(*name).to_string(),
                    ));
                },
            }
        } else if let Some(Type::Fn(params, ret)) = builtin::signature(*name) {
            (params.clone(), (**ret).clone())
        } else {
            return Err(KslError::with_subject(
                ErrorKind::UnknownName,
                span,
                resolve(*name).to_string(),
            ));
        };
        self.check_args(&callee.0, &callee.1, args, span)
    }

    /// 校验特殊形式的固定实参个数.
    pub(super) fn check_arity(
        args: &[Expr],
        expected: usize,
        span: Span,
    ) -> Result<(), KslError> {
        if args.len() == expected {
            Ok(())
        } else {
            Err(KslError::new(
                ErrorKind::ArityMismatch {
                    expected,
                    found: args.len(),
                },
                span,
            ))
        }
    }

    /// 校验实参个数与类型, 结果为被调用者的返回类型.
    /// 实参按形参类型作为期望检查, 字面量据此解释.
    pub(super) fn check_args(
        &mut self,
        params: &[Type],
        ret: &Type,
        args: &[Expr],
        span: Span,
    ) -> Result<InferTy, KslError> {
        Self::check_arity(args, params.len(), span)?;
        for (arg, param) in args.iter().zip(params) {
            let expected = InferTy::from_concrete(param);
            self.check_expr_expect(arg, Some(&expected))?;
        }
        Ok(InferTy::from_concrete(ret))
    }
}
