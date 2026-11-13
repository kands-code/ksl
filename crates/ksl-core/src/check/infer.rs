//! 检查器内部类型变量与统一.
//!
//! `InferTy` 只存在于单次检查内, `Type` 只表示具体类型; `Env`, 函数签名与
//! 类型表只保存 `Type`. 统一表使用 ena 的 in-place union-find, 变量绑定直接
//! 存入值槽, 变量来源信息按 `VarId` 索引存于稠密数组.

use std::sync::{Arc, LazyLock};

use ena::unify::{EqUnifyValue, InPlaceUnificationTable, UnifyKey};

use super::types::Type;
use crate::error::{ErrorKind, KslError};
use crate::source::Span;
use crate::symbol::{Symbol, keywords, resolve};

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

/// 推断期类型: 具体类型的形状加上未定的类型变量.
///
/// 镜像 [`Type`] 的结构: 变量可以出现在参数化类型内部. 两个转换函数都
/// 穷尽匹配, 新增类型形态时编译器会强制同步.
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
    Fn(Vec<InferTy>, Box<InferTy>),
    /// 参数化类型应用; 参数与 [`Type`] 一样用 `Arc` 共享.
    App {
        name: Symbol,
        args: Arc<[InferTy]>,
    },
    Opaque(Symbol),
}

impl InferTy {
    /// 构造参数化类型应用.
    pub(crate) fn app(name: Symbol, args: Vec<Self>) -> Self {
        Self::App {
            name,
            args: if args.is_empty() {
                empty_args()
            } else {
                Arc::from(args)
            },
        }
    }

    /// 构造无参数的 Data 类型.
    pub(crate) fn data(name: Symbol) -> Self {
        Self::app(name, Vec::new())
    }

    /// 构造 `List[T]`.
    pub(crate) fn list(inner: Self) -> Self {
        Self::app(keywords().list_, vec![inner])
    }

    /// 构造 `Dict[K, V]`.
    pub(crate) fn dict(key: Self, value: Self) -> Self {
        Self::app(crate::symbol::intern("Dict"), vec![key, value])
    }

    /// 构造 `Set[T]`.
    pub(crate) fn set(inner: Self) -> Self {
        Self::app(crate::symbol::intern("Set"), vec![inner])
    }

    /// 构造 `Pair[A, B]`.
    pub(crate) fn pair(first: Self, second: Self) -> Self {
        Self::app(keywords().pair_, vec![first, second])
    }

    /// 构造 `Option[T]`.
    pub(crate) fn option(inner: Self) -> Self {
        Self::app(keywords().option_, vec![inner])
    }

    /// 构造 `Result[T, E]`.
    pub(crate) fn result(ok: Self, err: Self) -> Self {
        Self::app(keywords().result_, vec![ok, err])
    }

    /// 取出 `List[T]` 的元素类型.
    pub(crate) fn as_list(&self) -> Option<&Self> {
        let [inner] = self.app_args(keywords().list_)? else {
            return None;
        };
        Some(inner)
    }

    /// 取出 `Dict[K, V]` 的键与值类型.
    pub(crate) fn as_dict(&self) -> Option<(&Self, &Self)> {
        let [key, value] = self.app_args(crate::symbol::intern("Dict"))? else {
            return None;
        };
        Some((key, value))
    }

    /// 取出 `Set[T]` 的元素类型.
    pub(crate) fn as_set(&self) -> Option<&Self> {
        let [inner] = self.app_args(crate::symbol::intern("Set"))? else {
            return None;
        };
        Some(inner)
    }

    /// 取出 `Pair[A, B]` 的两个分量.
    pub(crate) fn as_pair(&self) -> Option<(&Self, &Self)> {
        let [first, second] = self.app_args(keywords().pair_)? else {
            return None;
        };
        Some((first, second))
    }

    /// 取出 `Option[T]` 的元素类型.
    pub(crate) fn as_option(&self) -> Option<&Self> {
        let [inner] = self.app_args(keywords().option_)? else {
            return None;
        };
        Some(inner)
    }

    /// 取出 `Result[T, E]` 的成功与错误类型.
    pub(crate) fn as_result(&self) -> Option<(&Self, &Self)> {
        let [ok, err] = self.app_args(keywords().result_)? else {
            return None;
        };
        Some((ok, err))
    }

    /// 是否为 Pid 类型.
    pub(crate) fn is_pid(&self) -> bool {
        self.app_args(keywords().pid_)
            .is_some_and(|args| (2..=17).contains(&args.len()))
    }

    fn app_args(&self, name: Symbol) -> Option<&[Self]> {
        match self {
            Self::App { name: actual, args } if *actual == name => Some(args),
            _ => None,
        }
    }
}

fn empty_args() -> Arc<[InferTy]> {
    static EMPTY: LazyLock<Arc<[InferTy]>> = LazyLock::new(|| Arc::from(Vec::new()));
    Arc::clone(&EMPTY)
}

// ena 的值槽只用来存变量绑定; 两个已绑定值不会相遇, 因为统一前总会先 shallow.
impl EqUnifyValue for InferTy {}

impl InferTy {
    /// 把具体类型包装为推断类型.
    ///
    /// 穷尽匹配; 新增 `Type` 形态时必须同步.
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
            Type::Fn(params, ret) => Self::Fn(
                params.iter().map(Self::from_concrete).collect(),
                Box::new(Self::from_concrete(ret)),
            ),
            Type::App { name, args } => {
                Self::app(*name, args.iter().map(Self::from_concrete).collect())
            },
            Type::Opaque(name) => Self::Opaque(*name),
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
            (
                InferTy::App {
                    name: a_name,
                    args: a_args,
                },
                InferTy::App {
                    name: b_name,
                    args: b_args,
                },
            ) if a_name == b_name && a_args.len() == b_args.len() => {
                for (left, right) in a_args.iter().zip(b_args.iter()) {
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
            InferTy::Fn(params, ret) => Ok(Type::Fn(
                params
                    .iter()
                    .map(|param| self.finalize(param))
                    .collect::<Result<_, _>>()?,
                Box::new(self.finalize(&ret)?),
            )),
            InferTy::App { name, args } => Ok(Type::app(
                name,
                args.iter()
                    .map(|arg| self.finalize(arg))
                    .collect::<Result<_, _>>()?,
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
            InferTy::Opaque(name) => Ok(Type::opaque(name)),
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
            InferTy::Fn(params, ret) => {
                let params = params
                    .iter()
                    .map(|param| self.render(param))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Fn[[{params}] : {}]", self.render(&ret))
            },
            InferTy::App { name, args } => {
                let name = resolve(name);
                let args = args
                    .iter()
                    .map(|arg| self.render(arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                if args.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}[{args}]")
                }
            },
            InferTy::Opaque(name) => resolve(name).to_string(),
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
            InferTy::App { args, .. } => args.iter().any(|arg| self.has_unresolved(arg)),
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
            InferTy::App { args, .. } => args.iter().any(|arg| self.occurs(var, arg)),
            InferTy::Fn(params, ret) => {
                params.iter().any(|param| self.occurs(var, param)) || self.occurs(var, &ret)
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests;
