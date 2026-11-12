//! 表达式检查与调用规则.

use ksl_base::name::keywords;
use ksl_base::source::Span;
use ksl_base::types::{HandlerType, Type};
use ksl_catalog::builtin;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::display::check_format_spec;
use super::infer::{InferCtx, InferTy};
use super::{Checker, InferExpr, TypeTarget, TypedKind, is_printable};
use crate::ast::{Expr, ExprKind, Head, Special};

/// 程序检查结果: 尾表达式类型, 入口类型与 typed 程序.
type CheckedProgramOutput = (Option<Type>, Option<HandlerType>, ksl_typed::CheckedProgram);

impl Checker {
    /// 检查程序并返回 typed AST.
    pub(super) fn check_program_typed(
        &mut self,
        program: &crate::ast::Program,
        main: Option<&Expr>,
        tail_is_value: bool,
    ) -> Result<ksl_typed::CheckedProgram, KslError> {
        self.check_program_inner(program, tail_is_value, main)
            .map(|(_tail, _entry, typed)| typed)
    }

    fn check_program_inner(
        &mut self,
        program: &crate::ast::Program,
        tail_is_value: bool,
        main: Option<&Expr>,
    ) -> Result<CheckedProgramOutput, KslError> {
        self.pending_keys.clear();
        let type_names = self.context.type_env.type_names();
        self.infer = InferCtx::with_type_names(type_names);
        let mut stmts = program
            .stmts
            .iter()
            .map(|stmt| self.check_value_used_typed(stmt))
            .collect::<Result<Vec<_>, _>>()?;
        let tail = match &program.tail {
            Some(tail) if tail_is_value => Some(self.check_expr(tail)?),
            Some(tail) => {
                stmts.push(self.check_value_used_typed(tail)?);
                None
            },
            None => None,
        };
        let (entry, main) = match main {
            Some(main) => {
                let (handler, typed) = self.check_main_entry(main)?;
                (Some(handler), Some(typed))
            },
            None => (None, None),
        };
        let tail_ty = tail
            .as_ref()
            .map(|tail| self.infer.finalize(&tail.ty))
            .transpose()?;
        self.validate_pending_keys()?;
        let typed = ksl_typed::Program { stmts, tail, main };
        let typed_program = super::finalize::finalize_program(typed, &mut self.infer)?;
        // 本轮的变量绑定已全部解析; 不把统一表带进下一次会话克隆.
        self.infer = InferCtx::default();
        Ok((tail_ty, entry, typed_program))
    }

    /// 检查 `@Main` 入口声明; 记录入口节点的内部类型, 供降级与执行使用.
    fn check_main_entry(&mut self, main: &Expr) -> Result<(HandlerType, InferExpr), KslError> {
        let ExprKind::Special(Special::Main { params, ret, body }) = &main.node else {
            return Err(KslError::new(ErrorKind::ModuleFormNotResolved, main.span));
        };
        let (handler, body) = self.check_main(params, ret, body, main.span)?;
        let (ty, kind) = Self::typed_main(&handler, params, body);
        let typed = InferExpr {
            kind,
            span: main.span,
            ty,
        };
        Ok((handler, typed))
    }

    /// 语句值处置并保留 typed 节点.
    pub(super) fn check_value_used_typed(
        &mut self,
        expr: &Expr,
    ) -> Result<InferExpr, KslError> {
        let typed = self.check_expr(expr)?;
        if !matches!(self.infer.shallow(&typed.ty), InferTy::Nil | InferTy::Never) {
            let found = self.infer.finalize(&typed.ty)?;
            return Err(KslError::with_subject(
                ErrorKind::UnusedValue { found },
                expr.span,
                "语句值",
            ));
        }
        Ok(typed)
    }

    /// 检查表达式, 不带期望.
    pub(super) fn check_expr(&mut self, expr: &Expr) -> Result<InferExpr, KslError> {
        self.check_expr_expect(expr, None)
    }

    /// 按期望类型检查表达式.
    ///
    /// 期望来自类型标注, 函数签名或构造器的类型参数.
    /// 整数字面量严格按后缀定型.
    /// 列表元素共享元素类型变量.
    /// 内置泛型构造器与特殊形式通过统一解析未定类型.
    /// 无期望时类型由元素或分支共同决定; 仍无法定型时报错.
    /// 集中分派各语法形态.
    pub(super) fn check_expr_expect(
        &mut self,
        expr: &Expr,
        expected: Option<&InferTy>,
    ) -> Result<InferExpr, KslError> {
        // never 是底类型, 不作为子表达式的期望.
        let expected = expected.filter(|ty| !matches!(**ty, InferTy::Never));
        if let ExprKind::Special(form) = &expr.node {
            let (ty, kind) = self.check_special_expect(form, expr.span, expected)?;
            if let Some(expected) = expected {
                self.infer.unify(expected, &ty, expr.span)?;
            }
            return Ok(InferExpr {
                kind,
                span: expr.span,
                ty,
            });
        }
        let (found, kind) = match &expr.node {
            ExprKind::I64(value) => {
                self.check_int_kind(expected, TypedKind::I64(*value), expr.span)?
            },
            ExprKind::I32(value) => self.check_typed_kind(
                expected,
                InferTy::I32,
                TypedKind::I32(*value),
                expr.span,
            )?,
            ExprKind::U8(value) => {
                self.check_typed_kind(expected, InferTy::U8, TypedKind::U8(*value), expr.span)?
            },
            ExprKind::U32(value) => self.check_typed_kind(
                expected,
                InferTy::U32,
                TypedKind::U32(*value),
                expr.span,
            )?,
            ExprKind::U64(value) => self.check_typed_kind(
                expected,
                InferTy::U64,
                TypedKind::U64(*value),
                expr.span,
            )?,
            ExprKind::F32(value) => self.check_typed_kind(
                expected,
                InferTy::F32,
                TypedKind::F32(*value),
                expr.span,
            )?,
            ExprKind::F64(value) => self.check_typed_kind(
                expected,
                InferTy::F64,
                TypedKind::F64(*value),
                expr.span,
            )?,
            ExprKind::Char(value) => (InferTy::Char, TypedKind::Char(*value)),
            ExprKind::Str(value) => (InferTy::Str, TypedKind::Str(value.clone())),
            ExprKind::Bool(value) => (InferTy::Bool, TypedKind::Bool(*value)),
            ExprKind::Nil => (InferTy::Nil, TypedKind::Nil),
            ExprKind::FStr(parts) => self.check_fstr(parts, expr.span)?,
            ExprKind::Ident(name) => self.check_ident(name, expr.span)?,
            ExprKind::List(items) => self.check_list_literal(items, expected, expr.span)?,
            ExprKind::Pair(first, second) => {
                self.check_pair_literal(first, second, expected)?
            },
            ExprKind::Call { head, args } => {
                // 内置泛型构造器的类型参数可由期望给定, 如 `Option/Some`.
                if let Some(result) =
                    self.check_builtin_ctor(head, args, expr.span, expected)?
                {
                    result
                } else {
                    self.check_call(head, args, expr.span)?
                }
            },
            ExprKind::Apply { callee, args } => self.check_apply(callee, args, expr.span)?,
            ExprKind::Special(_) => unreachable!("特殊形式已在上方处理"),
        };
        if let Some(expected) = expected {
            self.infer.unify(expected, &found, expr.span)?;
        }
        Ok(InferExpr {
            kind,
            span: expr.span,
            ty: found,
        })
    }

    fn check_ident(
        &mut self,
        name: &ksl_base::name::Name,
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let ty = self
            .env
            .lookup(name)
            .or_else(|| builtin::signature(name))
            .map(InferTy::from_concrete)
            .ok_or_else(|| KslError::unknown_name(name, span))?;
        Ok((ty, TypedKind::Ident(name.clone())))
    }

    fn check_list_literal(
        &mut self,
        items: &[Expr],
        expected: Option<&InferTy>,
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let elem = expected
            .and_then(InferTy::as_list)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, "List 元素"));
        let mut typed = Vec::with_capacity(items.len());
        for item in items {
            let item = self.check_expr_expect(item, Some(&elem))?;
            self.infer.unify(&elem, &item.ty, item.span)?;
            typed.push(item);
        }
        Ok((InferTy::list(elem), TypedKind::List(typed)))
    }

    fn check_pair_literal(
        &mut self,
        first: &Expr,
        second: &Expr,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let (a, b) = if let Some((a, b)) = expected.and_then(InferTy::as_pair) {
            (a.clone(), b.clone())
        } else {
            (
                self.infer.fresh(first.span, "Pair 首分量"),
                self.infer.fresh(second.span, "Pair 次分量"),
            )
        };
        let first = self.check_expr_expect(first, Some(&a))?;
        self.infer.unify(&a, &first.ty, first.span)?;
        let second = self.check_expr_expect(second, Some(&b))?;
        self.infer.unify(&b, &second.ty, second.span)?;
        Ok((
            InferTy::pair(a, b),
            TypedKind::Pair(Box::new(first), Box::new(second)),
        ))
    }

    fn check_int_kind(
        &mut self,
        expected: Option<&InferTy>,
        kind: TypedKind,
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        Ok((self.check_int_literal(expected, span)?, kind))
    }

    fn check_typed_kind(
        &mut self,
        expected: Option<&InferTy>,
        found: InferTy,
        kind: TypedKind,
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        Ok((self.check_typed_literal(expected, found, span)?, kind))
    }

    /// 格式化字符串: 插值先解析为具体类型, 再校验可打印性与格式参数.
    pub(super) fn check_fstr(
        &mut self,
        parts: &[crate::ast::FStrAstPart],
        _span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let mut typed = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                crate::ast::FStrAstPart::Text(text) => {
                    typed.push(ksl_typed::FStrPart::Text(text.clone()));
                },
                crate::ast::FStrAstPart::Interp { expr, spec } => {
                    let value = self.check_expr(expr)?;
                    let ty = self.infer.finalize(&value.ty)?;
                    if !is_printable(&ty) {
                        return Err(KslError::new(
                            ErrorKind::UnprintableType { found: ty },
                            expr.span,
                        ));
                    }
                    if let Some(spec) = spec {
                        check_format_spec(spec, &ty, expr.span)?;
                    }
                    typed.push(ksl_typed::FStrPart::Interp {
                        value: Box::new(value),
                        spec: spec.clone(),
                    });
                },
            }
        }
        Ok((InferTy::Str, TypedKind::FStr(typed)))
    }

    /// 整数字面量: 无后缀即 i64; i32 与 u8 必须显式写后缀.
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

    /// 带后缀的标量字面量: 类型由后缀决定, 期望只能与之相同或是未定变量.
    fn check_typed_literal(
        &mut self,
        expected: Option<&InferTy>,
        ty: InferTy,
        span: Span,
    ) -> Result<InferTy, KslError> {
        match expected {
            Some(InferTy::Var(_)) | None => Ok(ty),
            Some(other) if *other == ty => Ok(ty),
            Some(other) => Err(self.infer.mismatch_error(other, &ty, span)),
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
    ) -> Result<Option<(InferTy, TypedKind)>, KslError> {
        let Some(qualifier) = &head.qualifier else {
            return Ok(None);
        };
        let variant = head.name.node.clone();
        let (ty, args, qualifier) = if qualifier.node.as_str() == keywords().option_.as_str() {
            let (ty, args) = match variant {
                ref v if v.as_str() == keywords().some_.as_str() => {
                    self.check_option_some(args, span, expected)?
                },
                ref v if v.as_str() == keywords().none_.as_str() => {
                    self.check_option_none(args, span, expected)?
                },
                _ => {
                    return Err(KslError::with_subject(
                        ErrorKind::UnknownName,
                        head.name.span,
                        format!("Option/{}", variant.as_str()),
                    ));
                },
            };
            (ty, args, keywords().option_.clone())
        } else if qualifier.node.as_str() == keywords().result_.as_str() {
            let (ty, args) = match variant {
                ref v if v.as_str() == keywords().ok_.as_str() => {
                    self.check_result_ok(args, span, expected)?
                },
                ref v if v.as_str() == keywords().err_.as_str() => {
                    self.check_result_err(args, span, expected)?
                },
                _ => {
                    return Err(KslError::with_subject(
                        ErrorKind::UnknownName,
                        head.name.span,
                        format!("Result/{}", variant.as_str()),
                    ));
                },
            };
            (ty, args, keywords().result_.clone())
        } else {
            return Ok(None);
        };
        Ok(Some((
            ty,
            TypedKind::Call {
                callee: ksl_typed::Callee::Data {
                    ty: qualifier,
                    variant,
                },
                args,
            },
        )))
    }

    fn check_option_some(
        &mut self,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, Vec<InferExpr>), KslError> {
        Self::check_arity(args, 1, span)?;
        let elem = expected
            .and_then(InferTy::as_option)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, "Option/Some 元素"));
        let option = InferTy::option(elem.clone());
        self.unify_expected(expected, &option, span)?;
        let arg = self.check_expr_expect(&args[0], Some(&elem))?;
        self.infer.unify(&elem, &arg.ty, args[0].span)?;
        Ok((option, vec![arg]))
    }

    fn check_option_none(
        &mut self,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, Vec<InferExpr>), KslError> {
        Self::check_arity(args, 0, span)?;
        let elem = expected
            .and_then(InferTy::as_option)
            .cloned()
            .unwrap_or_else(|| self.infer.fresh(span, "Option/None 元素"));
        let option = InferTy::option(elem);
        self.unify_expected(expected, &option, span)?;
        Ok((option, Vec::new()))
    }

    fn check_result_ok(
        &mut self,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, Vec<InferExpr>), KslError> {
        Self::check_arity(args, 1, span)?;
        let (ok, err) = expected.and_then(InferTy::as_result).map_or_else(
            || {
                (
                    self.infer.fresh(span, "Result/Ok 成功类型"),
                    self.infer.fresh(span, "Result/Ok 错误类型"),
                )
            },
            |(ok, err)| (ok.clone(), err.clone()),
        );
        let result = InferTy::result(ok.clone(), err);
        self.unify_expected(expected, &result, span)?;
        let arg = self.check_expr_expect(&args[0], Some(&ok))?;
        self.infer.unify(&ok, &arg.ty, args[0].span)?;
        Ok((result, vec![arg]))
    }

    fn check_result_err(
        &mut self,
        args: &[Expr],
        span: Span,
        expected: Option<&InferTy>,
    ) -> Result<(InferTy, Vec<InferExpr>), KslError> {
        Self::check_arity(args, 1, span)?;
        let (ok, err) = expected.and_then(InferTy::as_result).map_or_else(
            || {
                (
                    self.infer.fresh(span, "Result/Err 成功类型"),
                    self.infer.fresh(span, "Result/Err 错误类型"),
                )
            },
            |(ok, err)| (ok.clone(), err.clone()),
        );
        let result = InferTy::result(ok, err.clone());
        self.unify_expected(expected, &result, span)?;
        let arg = self.check_expr_expect(&args[0], Some(&err))?;
        self.infer.unify(&err, &arg.ty, args[0].span)?;
        Ok((result, vec![arg]))
    }

    /// 后缀调用: 被调是任意表达式, 先定型再校验实参.
    ///
    /// 只有函数类型可调用; never 也不可调用.
    pub(super) fn check_apply(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let callee = self.check_expr(callee)?;
        let callee_ty = self.infer.finalize(&callee.ty)?;
        match callee_ty {
            Type::Fn(params, ret) => {
                let (ty, args) = self.check_args(&params, &ret, args, span)?;
                Ok((
                    ty,
                    TypedKind::Apply {
                        callee: Box::new(callee),
                        args,
                    },
                ))
            },
            other => Err(KslError::new(
                ErrorKind::NotCallable { found: other },
                callee.span,
            )),
        }
    }

    /// 普通调用: 解析头部, 校验实参个数与类型.
    pub(super) fn check_call(
        &mut self,
        head: &Head,
        args: &[Expr],
        span: Span,
    ) -> Result<(InferTy, TypedKind), KslError> {
        let name = &head.name.node;
        // 模块目录是编译期常量: 检查时展开为字面量, 不进入运行时.
        // 交互输入与 `run --code` 不是模块, 报错.
        if head.qualifier.is_none()
            && builtin::id(name) == Some(builtin::BuiltinId::GetModuleDirectory)
        {
            Self::check_arity(args, 0, span)?;
            let Some(module_dir) = self.module_dir.clone() else {
                return Err(KslError::new(
                    ErrorKind::ModuleDirectoryOutsideModule,
                    head.name.span,
                ));
            };
            return Ok((InferTy::Str, TypedKind::Str(module_dir)));
        }
        let (params, ret, callee) = if let Some(qualifier) = &head.qualifier {
            // 类型限定名: 先按可见名找真实类型, 再查 Data 构造器.
            let Some(TypeTarget::Data(full)) = self.context.type_env.resolve(&qualifier.node)
            else {
                return Err(KslError::unknown_type(&qualifier.node, qualifier.span));
            };
            let info = self.required_data(&full, qualifier.span)?;
            let Some(fields) = info.variants.get(name) else {
                return Err(KslError::with_subject(
                    ErrorKind::UnknownName,
                    head.name.span,
                    format!("{}/{}", qualifier.node.as_str(), name.as_str()),
                ));
            };
            (
                fields.clone(),
                Type::data(full.clone()),
                ksl_typed::Callee::Data {
                    ty: full,
                    variant: name.clone(),
                },
            )
        } else if let Some(ty) = self.env.lookup(name).cloned() {
            match ty {
                Type::Fn(params, ret) => (
                    params.to_vec(),
                    (*ret).clone(),
                    ksl_typed::Callee::Named(name.clone()),
                ),
                other => {
                    return Err(KslError::with_name(
                        ErrorKind::NotCallable { found: other },
                        head.name.span,
                        name,
                    ));
                },
            }
        } else if let Some(Type::Fn(params, ret)) = builtin::signature(name) {
            (
                params.to_vec(),
                (**ret).clone(),
                ksl_typed::Callee::Builtin(
                    builtin::id(name).expect("内置签名存在时编号必然存在"),
                ),
            )
        } else {
            return Err(KslError::unknown_name(name, span));
        };
        let (ty, args) = self.check_args(&params, &ret, args, span)?;
        Ok((ty, TypedKind::Call { callee, args }))
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
    ) -> Result<(InferTy, Vec<InferExpr>), KslError> {
        Self::check_arity(args, params.len(), span)?;
        let mut typed = Vec::with_capacity(args.len());
        for (arg, param) in args.iter().zip(params) {
            let expected = InferTy::from_concrete(param);
            typed.push(self.check_expr_expect(arg, Some(&expected))?);
        }
        Ok((InferTy::from_concrete(ret), typed))
    }
}
