//! 检查器内部类型变量与统一.
//!
//! `InferTy` 只存在于单次检查内, `Type` 只表示具体类型; `Env`, 函数签名与
//! 类型表只保存 `Type`. 统一表使用 ena 的 in-place union-find, 变量绑定直接
//! 存入值槽, 变量来源信息按 `VarId` 索引存于稠密数组.

use ena::unify::{EqUnifyValue, InPlaceUnificationTable, UnifyKey};

use super::types::{HandlerType, PidType, Type};
use crate::error::{ErrorKind, KslError};
use crate::source::Span;
use crate::symbol::resolve;

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

/// 推断期的 Actor 句柄类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PidInferType {
    pub(crate) mailboxes: Vec<InferTy>,
    pub(crate) ret: Box<InferTy>,
}

/// 推断期的 Actor 蓝图类型.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HandlerInferType {
    pub(crate) pid: Box<InferTy>,
    pub(crate) args: Vec<InferTy>,
}

/// 推断期类型: 具体类型的形状加上未定的类型变量.
///
/// 与 [`Type`] 同形是刻意为之: 变量可以出现在 `List`, `Pair` 等结构内部,
/// 因此不能用"变量或具体类型"的二元形态. 两个转换函数都穷尽匹配, 新增
/// 类型形态时编译器会强制同步.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InferTy {
    Var(VarId),
    I64,
    U8,
    U64,
    F64,
    Char,
    Bool,
    Str,
    Nil,
    Never,
    List(Box<InferTy>),
    Dict(Box<InferTy>, Box<InferTy>),
    Set(Box<InferTy>),
    Pair(Box<InferTy>, Box<InferTy>),
    OptionT(Box<InferTy>),
    ResultT(Box<InferTy>, Box<InferTy>),
    PidT(Box<PidInferType>),
    HandlerT(Box<HandlerInferType>),
    Fn(Vec<InferTy>, Box<InferTy>),
    Data(crate::symbol::Symbol),
    File,
    DateTime,
}

// ena 的值槽只用来存变量绑定; 两个已绑定值不会相遇, 因为统一前总会先 shallow.
impl EqUnifyValue for InferTy {}

impl InferTy {
    /// 把具体类型包装为推断类型.
    ///
    /// 穷尽匹配是刻意的: 新增 `Type` 形态时此处必须同步, 避免两套枚举漂移.
    pub(crate) fn from_concrete(ty: &Type) -> Self {
        match ty {
            Type::I64 => Self::I64,
            Type::U8 => Self::U8,
            Type::U64 => Self::U64,
            Type::F64 => Self::F64,
            Type::Char => Self::Char,
            Type::Bool => Self::Bool,
            Type::Str => Self::Str,
            Type::Nil => Self::Nil,
            Type::Never => Self::Never,
            Type::List(inner) => Self::List(Box::new(Self::from_concrete(inner))),
            Type::Dict(key, value) => Self::Dict(
                Box::new(Self::from_concrete(key)),
                Box::new(Self::from_concrete(value)),
            ),
            Type::Set(inner) => Self::Set(Box::new(Self::from_concrete(inner))),
            Type::Pair(first, second) => Self::Pair(
                Box::new(Self::from_concrete(first)),
                Box::new(Self::from_concrete(second)),
            ),
            Type::OptionT(inner) => Self::OptionT(Box::new(Self::from_concrete(inner))),
            Type::ResultT(ok, err) => Self::ResultT(
                Box::new(Self::from_concrete(ok)),
                Box::new(Self::from_concrete(err)),
            ),
            Type::PidT(pid) => Self::PidT(Box::new(PidInferType {
                mailboxes: pid.mailboxes.iter().map(Self::from_concrete).collect(),
                ret: Box::new(Self::from_concrete(pid.ret.as_ref())),
            })),
            Type::HandlerT(handler) => Self::HandlerT(Box::new(HandlerInferType {
                pid: Box::new(Self::from_concrete(handler.pid.as_ref())),
                args: handler.args.iter().map(Self::from_concrete).collect(),
            })),
            Type::Fn(params, ret) => Self::Fn(
                params.iter().map(Self::from_concrete).collect(),
                Box::new(Self::from_concrete(ret)),
            ),
            Type::Data(name) => Self::Data(*name),
            Type::File => Self::File,
            Type::DateTime => Self::DateTime,
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
}

impl InferCtx {
    /// 新建一个类型变量.
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
            (InferTy::List(a), InferTy::List(b))
            | (InferTy::Set(a), InferTy::Set(b))
            | (InferTy::OptionT(a), InferTy::OptionT(b)) => self.unify(&a, &b, span),
            (InferTy::Dict(ak, av), InferTy::Dict(bk, bv)) => {
                self.unify(&ak, &bk, span)?;
                self.unify(&av, &bv, span)
            },
            (InferTy::Pair(a1, a2), InferTy::Pair(b1, b2))
            | (InferTy::ResultT(a1, a2), InferTy::ResultT(b1, b2)) => {
                self.unify(&a1, &b1, span)?;
                self.unify(&a2, &b2, span)
            },
            (InferTy::PidT(a), InferTy::PidT(b)) => {
                if a.mailboxes.len() != b.mailboxes.len() {
                    return Err(self.mismatch_error(
                        &InferTy::PidT(a),
                        &InferTy::PidT(b),
                        span,
                    ));
                }
                for (left, right) in a.mailboxes.iter().zip(&b.mailboxes) {
                    self.unify(left, right, span)?;
                }
                self.unify(&a.ret, &b.ret, span)
            },
            (InferTy::HandlerT(a), InferTy::HandlerT(b)) => {
                self.unify(&a.pid, &b.pid, span)?;
                if a.args.len() != b.args.len() {
                    return Err(self.mismatch_error(
                        &InferTy::HandlerT(a),
                        &InferTy::HandlerT(b),
                        span,
                    ));
                }
                for (left, right) in a.args.iter().zip(&b.args) {
                    self.unify(left, right, span)?;
                }
                Ok(())
            },
            (InferTy::Fn(a_params, a_ret), InferTy::Fn(b_params, b_ret)) => {
                if a_params.len() != b_params.len() {
                    return Err(self.mismatch_error(
                        &InferTy::Fn(a_params, a_ret),
                        &InferTy::Fn(b_params, b_ret),
                        span,
                    ));
                }
                for (left, right) in a_params.iter().zip(&b_params) {
                    self.unify(left, right, span)?;
                }
                self.unify(&a_ret, &b_ret, span)
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
                    span: Span::new(crate::source::SourceId::default(), 0, 0),
                    context: "类型变量",
                });
                Err(KslError::new(
                    ErrorKind::UndeterminedType {
                        detail: info.context.to_string(),
                    },
                    info.span,
                ))
            },
            InferTy::List(inner) => Ok(Type::List(Box::new(self.finalize(&inner)?))),
            InferTy::Dict(key, value) => Ok(Type::Dict(
                Box::new(self.finalize(&key)?),
                Box::new(self.finalize(&value)?),
            )),
            InferTy::Set(inner) => Ok(Type::Set(Box::new(self.finalize(&inner)?))),
            InferTy::Pair(first, second) => Ok(Type::Pair(
                Box::new(self.finalize(&first)?),
                Box::new(self.finalize(&second)?),
            )),
            InferTy::OptionT(inner) => Ok(Type::OptionT(Box::new(self.finalize(&inner)?))),
            InferTy::ResultT(ok, err) => Ok(Type::ResultT(
                Box::new(self.finalize(&ok)?),
                Box::new(self.finalize(&err)?),
            )),
            InferTy::PidT(pid) => Ok(Type::PidT(Box::new(PidType {
                mailboxes: pid
                    .mailboxes
                    .iter()
                    .map(|mailbox| self.finalize(mailbox))
                    .collect::<Result<_, _>>()?,
                ret: Box::new(self.finalize(&pid.ret)?),
            }))),
            InferTy::HandlerT(handler) => Ok(Type::HandlerT(Box::new(HandlerType {
                pid: Box::new(self.finalize(&handler.pid)?),
                args: handler
                    .args
                    .iter()
                    .map(|arg| self.finalize(arg))
                    .collect::<Result<_, _>>()?,
            }))),
            InferTy::Fn(params, ret) => Ok(Type::Fn(
                params
                    .iter()
                    .map(|param| self.finalize(param))
                    .collect::<Result<_, _>>()?,
                Box::new(self.finalize(&ret)?),
            )),
            InferTy::I64 => Ok(Type::I64),
            InferTy::U8 => Ok(Type::U8),
            InferTy::U64 => Ok(Type::U64),
            InferTy::F64 => Ok(Type::F64),
            InferTy::Char => Ok(Type::Char),
            InferTy::Bool => Ok(Type::Bool),
            InferTy::Str => Ok(Type::Str),
            InferTy::Nil => Ok(Type::Nil),
            InferTy::Never => Ok(Type::Never),
            InferTy::Data(name) => Ok(Type::Data(name)),
            InferTy::File => Ok(Type::File),
            InferTy::DateTime => Ok(Type::DateTime),
        }
    }

    /// 渲染推断类型; 未解析变量显示为 `_`, 供错误消息使用.
    pub(crate) fn render(&mut self, ty: &InferTy) -> String {
        match self.shallow(ty) {
            InferTy::Var(_) => "_".to_string(),
            InferTy::I64 => "i64".to_string(),
            InferTy::U8 => "u8".to_string(),
            InferTy::U64 => "u64".to_string(),
            InferTy::F64 => "f64".to_string(),
            InferTy::Char => "char".to_string(),
            InferTy::Bool => "bool".to_string(),
            InferTy::Str => "string".to_string(),
            InferTy::Nil => "nil".to_string(),
            InferTy::Never => "never".to_string(),
            InferTy::List(inner) => format!("List[{}]", self.render(&inner)),
            InferTy::Dict(key, value) => {
                format!("Dict[{}, {}]", self.render(&key), self.render(&value))
            },
            InferTy::Set(inner) => format!("Set[{}]", self.render(&inner)),
            InferTy::Pair(first, second) => {
                format!("Pair[{}, {}]", self.render(&first), self.render(&second))
            },
            InferTy::OptionT(inner) => format!("Option[{}]", self.render(&inner)),
            InferTy::ResultT(ok, err) => {
                format!("Result[{}, {}]", self.render(&ok), self.render(&err))
            },
            InferTy::PidT(pid) => {
                let mut args: Vec<String> = pid
                    .mailboxes
                    .iter()
                    .map(|mailbox| self.render(mailbox))
                    .collect();
                args.push(self.render(&pid.ret));
                format!("Pid[{}]", args.join(", "))
            },
            InferTy::HandlerT(handler) => {
                let mut args = Vec::with_capacity(handler.args.len() + 1);
                args.push(self.render(&handler.pid));
                args.extend(handler.args.iter().map(|arg| self.render(arg)));
                format!("Handler[{}]", args.join(", "))
            },
            InferTy::Fn(params, ret) => {
                let params = params
                    .iter()
                    .map(|param| self.render(param))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Fn[[{params}] : {}]", self.render(&ret))
            },
            InferTy::Data(name) => resolve(name).to_string(),
            InferTy::File => "File".to_string(),
            InferTy::DateTime => "DateTime".to_string(),
        }
    }

    /// 构造类型冲突错误: 双方都已具体时报 `TypeMismatch`, 否则报推断期冲突.
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
        match self.shallow(ty) {
            InferTy::Var(_) => true,
            InferTy::List(inner) | InferTy::Set(inner) | InferTy::OptionT(inner) => {
                self.has_unresolved(&inner)
            },
            InferTy::Dict(key, value) => {
                self.has_unresolved(&key) || self.has_unresolved(&value)
            },
            InferTy::Pair(first, second) | InferTy::ResultT(first, second) => {
                self.has_unresolved(&first) || self.has_unresolved(&second)
            },
            InferTy::PidT(pid) => {
                pid.mailboxes.iter().any(|mailbox| self.has_unresolved(mailbox))
                    || self.has_unresolved(&pid.ret)
            },
            InferTy::HandlerT(handler) => {
                self.has_unresolved(&handler.pid)
                    || handler.args.iter().any(|arg| self.has_unresolved(arg))
            },
            InferTy::Fn(params, ret) => {
                params.iter().any(|param| self.has_unresolved(param))
                    || self.has_unresolved(&ret)
            },
            _ => false,
        }
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
        match self.shallow(ty) {
            InferTy::Var(other) => self.table.find(var) == self.table.find(other),
            InferTy::List(inner) | InferTy::Set(inner) | InferTy::OptionT(inner) => {
                self.occurs(var, &inner)
            },
            InferTy::Dict(key, value) => self.occurs(var, &key) || self.occurs(var, &value),
            InferTy::Pair(first, second) | InferTy::ResultT(first, second) => {
                self.occurs(var, &first) || self.occurs(var, &second)
            },
            InferTy::PidT(pid) => {
                pid.mailboxes.iter().any(|mailbox| self.occurs(var, mailbox))
                    || self.occurs(var, &pid.ret)
            },
            InferTy::HandlerT(handler) => {
                self.occurs(var, &handler.pid)
                    || handler.args.iter().any(|arg| self.occurs(var, arg))
            },
            InferTy::Fn(params, ret) => {
                params.iter().any(|param| self.occurs(var, param)) || self.occurs(var, &ret)
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
