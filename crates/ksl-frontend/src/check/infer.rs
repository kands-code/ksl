//! 检查器内部类型变量与统一.
//!
//! `InferTy` 只存在于单次检查内, `Type` 只表示具体类型.
//! `Env`, 函数签名与 typed AST 只保存 `Type`.
//! 统一表使用 ena 的 in-place union-find, 变量绑定直接存入值槽.
//! 变量来源信息按 `VarId` 索引存于稠密数组.

use std::sync::Arc;

use ena::unify::{EqUnifyValue, InPlaceUnificationTable, UnifyKey};
use ksl_base::display::TypeNames;
use ksl_base::name::Name;
use ksl_base::source::Span;
use ksl_base::type_shape;
use ksl_base::types::Type;
use ksl_diagnostics::error::{ErrorKind, KslError};

/// 类型变量标识; 只在本模块内分配, 不暴露给用户.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct VarId(u32);

impl UnifyKey for VarId {
    type Value = Option<InferTy>;

    fn index(&self) -> u32 {
        self.0
    }

    fn from_index(index: u32) -> Self {
        Self(index)
    }

    fn tag() -> &'static str {
        "ksl_type_var"
    }
}

// 推断期类型: 具体类型的形状加上未定的类型变量.
//
// 镜像 [`Type`] 的结构: 变量可以出现在参数化类型内部.
type_shape! {
    /// 推断期类型.
    pub(crate) enum InferTy {
        Var(VarId),
    }
}

impl InferTy {
    pub(crate) fn function(params: Vec<Self>, ret: Self) -> Self {
        Self::Fn(params.into(), Arc::new(ret))
    }

    pub(crate) fn data(name: Name) -> Self {
        Self::Data(name)
    }

    pub(crate) fn list(inner: Self) -> Self {
        Self::List(Arc::new(inner))
    }

    pub(crate) fn dict(key: Self, value: Self) -> Self {
        Self::Dict(Arc::new(key), Arc::new(value))
    }

    pub(crate) fn set(inner: Self) -> Self {
        Self::Set(Arc::new(inner))
    }

    pub(crate) fn pair(first: Self, second: Self) -> Self {
        Self::Pair(Arc::new(first), Arc::new(second))
    }

    pub(crate) fn option(inner: Self) -> Self {
        Self::Option(Arc::new(inner))
    }

    pub(crate) fn result(ok: Self, err: Self) -> Self {
        Self::Result(Arc::new(ok), Arc::new(err))
    }

    /// 取出 `List[T]` 的元素类型.
    pub(crate) fn as_list(&self) -> Option<&Self> {
        match self {
            Self::List(inner) => Some(inner),
            _ => None,
        }
    }

    /// 取出 `Dict[K, V]` 的键与值类型.
    pub(crate) fn as_dict(&self) -> Option<(&Self, &Self)> {
        match self {
            Self::Dict(key, value) => Some((key, value)),
            _ => None,
        }
    }

    /// 取出 `Set[T]` 的元素类型.
    pub(crate) fn as_set(&self) -> Option<&Self> {
        match self {
            Self::Set(inner) => Some(inner),
            _ => None,
        }
    }

    /// 取出 `Pair[A, B]` 的两个分量.
    pub(crate) fn as_pair(&self) -> Option<(&Self, &Self)> {
        match self {
            Self::Pair(first, second) => Some((first, second)),
            _ => None,
        }
    }

    /// 取出 `Option[T]` 的元素类型.
    pub(crate) fn as_option(&self) -> Option<&Self> {
        match self {
            Self::Option(inner) => Some(inner),
            _ => None,
        }
    }

    /// 取出 `Result[T, E]` 的成功与错误类型.
    pub(crate) fn as_result(&self) -> Option<(&Self, &Self)> {
        match self {
            Self::Result(ok, err) => Some((ok, err)),
            _ => None,
        }
    }

    /// 遍历直接子类型; 标量, Data, Opaque 与变量没有子类型.
    pub(crate) fn for_each_child(&self, visit: &mut impl FnMut(&Self)) {
        match self {
            Self::Fn(params, ret) => {
                for param in params.iter() {
                    visit(param);
                }
                visit(ret);
            },
            Self::List(inner) | Self::Set(inner) | Self::Option(inner) => visit(inner),
            Self::Dict(key, value) | Self::Pair(key, value) | Self::Result(key, value) => {
                visit(key);
                visit(value);
            },
            Self::Pid { mailboxes, ret } => {
                for mailbox in mailboxes.iter() {
                    visit(mailbox);
                }
                visit(ret);
            },
            Self::Handler { pid, args } => {
                visit(pid);
                for arg in args.iter() {
                    visit(arg);
                }
            },
            Self::Var(_)
            | Self::I64
            | Self::I32
            | Self::U8
            | Self::U32
            | Self::U64
            | Self::F32
            | Self::F64
            | Self::Char
            | Self::Bool
            | Self::Str
            | Self::Nil
            | Self::Never
            | Self::Data(_)
            | Self::Opaque(_) => {},
        }
    }
}

// ena 的值槽只用来存变量绑定.
// 统一前先调用 shallow 解析, 两个已绑定值不会相遇.
impl EqUnifyValue for InferTy {}

impl InferTy {
    /// 覆盖 `Type` 的全部形态.
    pub(crate) fn from_concrete(ty: &Type) -> Self {
        match ty {
            Type::I64 => Self::I64,
            Type::I32 => Self::I32,
            Type::U8 => Self::U8,
            Type::U32 => Self::U32,
            Type::U64 => Self::U64,
            Type::F32 => Self::F32,
            Type::F64 => Self::F64,
            Type::Char => Self::Char,
            Type::Bool => Self::Bool,
            Type::Str => Self::Str,
            Type::Nil => Self::Nil,
            Type::Never => Self::Never,
            Type::Fn(params, ret) => Self::Fn(
                params.iter().map(Self::from_concrete).collect(),
                Arc::new(Self::from_concrete(ret)),
            ),
            Type::List(inner) => Self::list(Self::from_concrete(inner)),
            Type::Dict(key, value) => {
                Self::dict(Self::from_concrete(key), Self::from_concrete(value))
            },
            Type::Set(inner) => Self::set(Self::from_concrete(inner)),
            Type::Pair(first, second) => {
                Self::pair(Self::from_concrete(first), Self::from_concrete(second))
            },
            Type::Option(inner) => Self::option(Self::from_concrete(inner)),
            Type::Result(ok, err) => {
                Self::result(Self::from_concrete(ok), Self::from_concrete(err))
            },
            Type::Pid { mailboxes, ret } => Self::Pid {
                mailboxes: mailboxes.iter().map(Self::from_concrete).collect(),
                ret: Arc::new(Self::from_concrete(ret)),
            },
            Type::Handler { pid, args } => Self::Handler {
                pid: Arc::new(Self::from_concrete(pid)),
                args: args.iter().map(Self::from_concrete).collect(),
            },
            Type::Data(name) => Self::Data(name.clone()),
            Type::Opaque(name) => Self::Opaque(name.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct VarInfo {
    span: Span,
    context: &'static str,
}

/// 类型变量上下文: union-find 与来源信息.
#[derive(Clone, Debug, Default)]
pub(crate) struct InferCtx {
    table: InPlaceUnificationTable<VarId>,
    /// 按 `VarId` 索引; 与 `table` 的键分配顺序一致.
    info: Vec<VarInfo>,
    /// 本轮检查的类型名显示表; 推断期错误消息使用.
    type_names: TypeNames,
}

impl InferCtx {
    /// 以指定类型名显示表构造上下文.
    pub(crate) fn with_type_names(type_names: TypeNames) -> Self {
        Self {
            type_names,
            ..Self::default()
        }
    }

    pub(crate) fn fresh(&mut self, span: Span, context: &'static str) -> InferTy {
        let var = self.table.new_key(None);
        debug_assert_eq!(var.index() as usize, self.info.len());
        self.info.push(VarInfo { span, context });
        InferTy::Var(var)
    }

    /// 解析绑定链; 未绑定的变量返回其根.
    pub(crate) fn shallow(&mut self, ty: &InferTy) -> InferTy {
        let InferTy::Var(var) = ty else {
            return ty.clone();
        };
        let root = self.table.find(*var);
        match self.table.probe_value(root) {
            Some(bound) => self.shallow(&bound),
            None => InferTy::Var(root),
        }
    }

    /// 统一两个类型; 失败时保留现有绑定, 由调用方丢弃整个检查结果.
    pub(crate) fn unify(
        &mut self,
        left: &InferTy,
        right: &InferTy,
        span: Span,
    ) -> Result<(), KslError> {
        let left = self.shallow(left);
        let right = self.shallow(right);
        if matches!(left, InferTy::Never) || matches!(right, InferTy::Never) {
            return Ok(());
        }
        match (left, right) {
            (InferTy::Var(a), InferTy::Var(b)) => {
                let a = self.table.find(a);
                let b = self.table.find(b);
                if a != b && self.table.unify_var_var(a, b).is_err() {
                    return Err(self.mismatch_error(&InferTy::Var(a), &InferTy::Var(b), span));
                }
                Ok(())
            },
            (InferTy::Var(var), ty) | (ty, InferTy::Var(var)) => self.bind(var, &ty, span),
            (InferTy::Fn(a_params, a_ret), InferTy::Fn(b_params, b_ret)) => {
                if a_params.len() != b_params.len() {
                    return Err(self.mismatch_error(
                        &InferTy::Fn(a_params, a_ret),
                        &InferTy::Fn(b_params, b_ret),
                        span,
                    ));
                }
                for (left, right) in a_params.iter().zip(b_params.iter()) {
                    self.unify(left, right, span)?;
                }
                self.unify(&a_ret, &b_ret, span)
            },
            (InferTy::List(a), InferTy::List(b))
            | (InferTy::Set(a), InferTy::Set(b))
            | (InferTy::Option(a), InferTy::Option(b)) => self.unify(&a, &b, span),
            (InferTy::Dict(a_key, a_value), InferTy::Dict(b_key, b_value))
            | (InferTy::Pair(a_key, a_value), InferTy::Pair(b_key, b_value))
            | (InferTy::Result(a_key, a_value), InferTy::Result(b_key, b_value)) => {
                self.unify(&a_key, &b_key, span)?;
                self.unify(&a_value, &b_value, span)
            },
            (
                InferTy::Pid {
                    mailboxes: a_mailboxes,
                    ret: a_ret,
                },
                InferTy::Pid {
                    mailboxes: b_mailboxes,
                    ret: b_ret,
                },
            ) => {
                if a_mailboxes.len() != b_mailboxes.len() {
                    return Err(self.mismatch_error(
                        &InferTy::Pid {
                            mailboxes: a_mailboxes,
                            ret: a_ret,
                        },
                        &InferTy::Pid {
                            mailboxes: b_mailboxes,
                            ret: b_ret,
                        },
                        span,
                    ));
                }
                for (left, right) in a_mailboxes.iter().zip(b_mailboxes.iter()) {
                    self.unify(left, right, span)?;
                }
                self.unify(&a_ret, &b_ret, span)
            },
            (
                InferTy::Handler {
                    pid: a_pid,
                    args: a_args,
                },
                InferTy::Handler {
                    pid: b_pid,
                    args: b_args,
                },
            ) => {
                if a_args.len() != b_args.len() {
                    return Err(self.mismatch_error(
                        &InferTy::Handler {
                            pid: a_pid,
                            args: a_args,
                        },
                        &InferTy::Handler {
                            pid: b_pid,
                            args: b_args,
                        },
                        span,
                    ));
                }
                self.unify(&a_pid, &b_pid, span)?;
                for (left, right) in a_args.iter().zip(b_args.iter()) {
                    self.unify(left, right, span)?;
                }
                Ok(())
            },
            (left, right) if left == right => Ok(()),
            (left, right) => Err(self.mismatch_error(&left, &right, span)),
        }
    }

    /// 解析类型变量到具体类型; 仍有未解析变量时报错.
    pub(crate) fn finalize(&mut self, ty: &InferTy) -> Result<Type, KslError> {
        match self.shallow(ty) {
            InferTy::Var(var) => {
                let info = self.info.get(var.index() as usize).copied().unwrap_or(VarInfo {
                    span: Span::new(ksl_base::source::SourceId::default(), 0, 0),
                    context: "类型变量",
                });
                Err(KslError::new(
                    ErrorKind::UndeterminedType {
                        detail: info.context.to_string(),
                    },
                    info.span,
                ))
            },
            InferTy::I64 => Ok(Type::I64),
            InferTy::I32 => Ok(Type::I32),
            InferTy::U8 => Ok(Type::U8),
            InferTy::U32 => Ok(Type::U32),
            InferTy::U64 => Ok(Type::U64),
            InferTy::F32 => Ok(Type::F32),
            InferTy::F64 => Ok(Type::F64),
            InferTy::Char => Ok(Type::Char),
            InferTy::Bool => Ok(Type::Bool),
            InferTy::Str => Ok(Type::Str),
            InferTy::Nil => Ok(Type::Nil),
            InferTy::Never => Ok(Type::Never),
            InferTy::Fn(params, ret) => Ok(Type::function(
                params
                    .iter()
                    .map(|param| self.finalize(param))
                    .collect::<Result<_, _>>()?,
                self.finalize(&ret)?,
            )),
            InferTy::List(inner) => Ok(Type::list(self.finalize(&inner)?)),
            InferTy::Dict(key, value) => {
                Ok(Type::dict(self.finalize(&key)?, self.finalize(&value)?))
            },
            InferTy::Set(inner) => Ok(Type::set(self.finalize(&inner)?)),
            InferTy::Pair(first, second) => {
                Ok(Type::pair(self.finalize(&first)?, self.finalize(&second)?))
            },
            InferTy::Option(inner) => Ok(Type::option(self.finalize(&inner)?)),
            InferTy::Result(ok, err) => {
                Ok(Type::result(self.finalize(&ok)?, self.finalize(&err)?))
            },
            InferTy::Pid { mailboxes, ret } => Ok(Type::pid(
                mailboxes
                    .iter()
                    .map(|mailbox| self.finalize(mailbox))
                    .collect::<Result<_, _>>()?,
                self.finalize(&ret)?,
            )),
            InferTy::Handler { pid, args } => Ok(Type::handler(
                self.finalize(&pid)?,
                args.iter()
                    .map(|arg| self.finalize(arg))
                    .collect::<Result<_, _>>()?,
            )),
            InferTy::Data(name) => Ok(Type::data(name)),
            InferTy::Opaque(name) => Ok(Type::opaque(name)),
        }
    }

    /// 渲染推断类型; 未解析变量显示为 `_`, 供错误消息使用.
    pub(crate) fn render(&mut self, ty: &InferTy) -> String {
        match self.shallow(ty) {
            InferTy::Var(_) => "_".to_string(),
            InferTy::I64 => "i64".to_string(),
            InferTy::I32 => "i32".to_string(),
            InferTy::U8 => "u8".to_string(),
            InferTy::U32 => "u32".to_string(),
            InferTy::U64 => "u64".to_string(),
            InferTy::F32 => "f32".to_string(),
            InferTy::F64 => "f64".to_string(),
            InferTy::Char => "char".to_string(),
            InferTy::Bool => "bool".to_string(),
            InferTy::Str => "string".to_string(),
            InferTy::Nil => "nil".to_string(),
            InferTy::Never => "never".to_string(),
            InferTy::Fn(params, ret) => {
                let params = params
                    .iter()
                    .map(|param| self.render(param))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Fn[[{params}] : {}]", self.render(&ret))
            },
            InferTy::List(inner) => format!("List[{}]", self.render(&inner)),
            InferTy::Dict(key, value) => {
                format!("Dict[{}, {}]", self.render(&key), self.render(&value))
            },
            InferTy::Set(inner) => format!("Set[{}]", self.render(&inner)),
            InferTy::Pair(first, second) => {
                format!("Pair[{}, {}]", self.render(&first), self.render(&second))
            },
            InferTy::Option(inner) => format!("Option[{}]", self.render(&inner)),
            InferTy::Result(ok, err) => {
                format!("Result[{}, {}]", self.render(&ok), self.render(&err))
            },
            InferTy::Pid { mailboxes, ret } => {
                let mailboxes = mailboxes
                    .iter()
                    .map(|mailbox| self.render(mailbox))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Pid[{mailboxes}, {}]", self.render(&ret))
            },
            InferTy::Handler { pid, args } => {
                let args = args.iter().map(|arg| self.render(arg)).collect::<Vec<_>>();
                if args.is_empty() {
                    format!("Handler[{}]", self.render(&pid))
                } else {
                    format!("Handler[{}, {}]", self.render(&pid), args.join(", "))
                }
            },
            InferTy::Data(name) | InferTy::Opaque(name) => {
                self.type_names.resolve(&name).as_str().to_string()
            },
        }
    }

    /// 构造类型冲突错误. 双方具体时报 `TypeMismatch`, 否则报推断期冲突.
    pub(crate) fn mismatch_error(
        &mut self,
        left: &InferTy,
        right: &InferTy,
        span: Span,
    ) -> KslError {
        if !self.has_unresolved(left)
            && !self.has_unresolved(right)
            && let (Ok(left), Ok(right)) = (self.finalize(left), self.finalize(right))
        {
            return KslError::new(
                ErrorKind::TypeMismatch {
                    expected: left,
                    found: right,
                },
                span,
            );
        }
        KslError::new(
            ErrorKind::InferMismatch {
                expected: self.render(left),
                found: self.render(right),
            },
            span,
        )
    }

    fn has_unresolved(&mut self, ty: &InferTy) -> bool {
        if matches!(self.shallow(ty), InferTy::Var(_)) {
            return true;
        }
        let ty = self.shallow(ty);
        let mut unresolved = false;
        ty.for_each_child(&mut |child| {
            if !unresolved && self.has_unresolved(child) {
                unresolved = true;
            }
        });
        unresolved
    }

    fn bind(&mut self, var: VarId, ty: &InferTy, span: Span) -> Result<(), KslError> {
        let root = self.table.find(var);
        if self.occurs(root, ty) {
            return Err(KslError::new(
                ErrorKind::UndeterminedType {
                    detail: "递归类型".to_string(),
                },
                span,
            ));
        }
        if self.table.unify_var_value(root, Some(ty.clone())).is_err() {
            return Err(self.mismatch_error(&InferTy::Var(root), ty, span));
        }
        Ok(())
    }

    fn occurs(&mut self, var: VarId, ty: &InferTy) -> bool {
        let ty = self.shallow(ty);
        if let InferTy::Var(other) = ty {
            return self.table.find(var) == self.table.find(other);
        }
        let mut occurs = false;
        ty.for_each_child(&mut |child| {
            if !occurs && self.occurs(var, child) {
                occurs = true;
            }
        });
        occurs
    }
}

#[cfg(test)]
mod tests;
