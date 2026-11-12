//! Data 表, 内置 Data 类型与类型名解析.

use indexmap::IndexMap;
use ksl_base::name::{Name, keywords};
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::reserved;
use ksl_diagnostics::error::{ErrorKind, KslError};

use super::context::{TypeEnv, TypeTarget};
use crate::ast::{ExprKind, Spanned, Special, TypeExpr};

/// 单个 Data 变体的字段数上限; `@DataGet` 的下标是 u8.
pub(super) const MAX_VARIANT_FIELDS: usize = 256;

#[derive(Debug, Clone)]
pub struct DataInfo {
    pub variants: IndexMap<Name, Vec<Type>>,
}

/// 已声明的 Data 类型表.
///
/// 映射使用持久化结构, 会话克隆只做结构共享.
/// 本表只按名字查找; 声明顺序由收集阶段的临时表保持.
#[derive(Debug, Clone)]
pub struct DataTable {
    types: imbl::HashMap<Name, DataInfo>,
}

impl DataTable {
    #[must_use]
    pub fn new() -> Self {
        Self {
            types: imbl::HashMap::new(),
        }
    }

    /// 按名字查找.
    #[must_use]
    pub fn get(&self, name: &Name) -> Option<&DataInfo> {
        self.types.get(name)
    }

    /// 名字是否已声明.
    #[must_use]
    pub fn contains_key(&self, name: &Name) -> bool {
        self.types.contains_key(name)
    }

    /// 插入或更新一个类型声明.
    pub fn insert(&mut self, name: Name, info: DataInfo) {
        self.types.insert(name, info);
    }

    /// 合并另一张表; 已有名字不覆盖.
    pub fn merge(&mut self, other: &Self) {
        for (name, info) in &other.types {
            if !self.types.contains_key(name) {
                self.types.insert(name.clone(), info.clone());
            }
        }
    }
}

impl Default for DataTable {
    fn default() -> Self {
        // 内置 Data 类型与用户 Data 同样可标注与匹配.
        let mut table = Self::new();
        for data in ksl_catalog::data::builtin_data_types() {
            let variants = data
                .variants
                .iter()
                .map(|variant| (Name::new(variant.name), variant.fields.clone()))
                .collect();
            table.insert(Name::new(data.name), DataInfo { variants });
        }
        table
    }
}

/// 收集并校验程序中的 `@Data` 声明.
///
/// 声明名的真实身份由 `identity` 给出; 本地名随后作为可见名登记.
/// 字段类型在全部声明名登记后统一解析, 可引用同程序的任意声明.
pub struct TypeDeclCollector<'a> {
    env: &'a mut TypeEnv,
}

/// 收集阶段的一条未解析声明.
struct RawDecl {
    full: Name,
    span: Span,
    variants: IndexMap<Name, Vec<TypeExpr>>,
}

impl<'a> TypeDeclCollector<'a> {
    #[must_use]
    pub fn new(env: &'a mut TypeEnv) -> Self {
        Self { env }
    }

    /// 收集并校验全部 `@Data` 声明.
    ///
    /// # Errors
    ///
    /// 声明重复, 变体重复或字段类型非法时返回首个错误.
    pub fn collect(
        &mut self,
        program: &crate::ast::Program,
        identity: impl Fn(Name) -> Name,
    ) -> Result<(), KslError> {
        // 第一趟: 登记类型名与变体, 字段保留为未解析的 TypeExpr.
        // 有序表保留声明顺序, 使第二趟的首个错误可复现.
        let mut raw: IndexMap<Name, RawDecl> = IndexMap::new();
        for stmt in program.exprs() {
            let ExprKind::Special(Special::Data { name, variants }) = &stmt.node else {
                continue;
            };
            if reserved::is_reserved(&name.node) {
                return Err(KslError::reserved_name(&name.node, name.span));
            }
            if self.env.bindings().contains_key(&name.node) {
                return Err(KslError::with_name(
                    ErrorKind::DuplicateData,
                    name.span,
                    &name.node,
                ));
            }
            let full = identity(name.node.clone());
            if self.env.data().contains_key(&full)
                || raw.values().any(|existing| existing.full == full)
            {
                return Err(KslError::with_name(
                    ErrorKind::DuplicateData,
                    name.span,
                    &name.node,
                ));
            }
            if variants.is_empty() {
                return Err(KslError::with_name(ErrorKind::EmptyData, name.span, &name.node));
            }
            let mut variant_map = IndexMap::new();
            for variant in variants {
                if variant.name.node == name.node {
                    return Err(KslError::new(
                        ErrorKind::VariantNameConflict,
                        variant.name.span,
                    ));
                }
                if reserved::is_reserved(&variant.name.node) {
                    return Err(KslError::reserved_name(&variant.name.node, variant.name.span));
                }
                if variant_map.contains_key(&variant.name.node) {
                    return Err(KslError::with_name(
                        ErrorKind::DuplicateVariant,
                        variant.name.span,
                        &variant.name.node,
                    ));
                }
                if variant.fields.len() > MAX_VARIANT_FIELDS {
                    return Err(KslError::with_name(
                        ErrorKind::TooManyVariantFields {
                            count: variant.fields.len(),
                            limit: MAX_VARIANT_FIELDS,
                        },
                        variant.name.span,
                        &variant.name.node,
                    ));
                }
                variant_map.insert(variant.name.node.clone(), variant.fields.clone());
            }
            raw.insert(
                name.node.clone(),
                RawDecl {
                    full,
                    span: name.span,
                    variants: variant_map,
                },
            );
        }
        // 第二趟: 先登记全部可见名, 使字段类型可引用同程序的任意声明.
        for (local, decl) in &raw {
            self.env
                .bind(local.clone(), TypeTarget::Data(decl.full.clone()), decl.span);
        }
        // 第三趟: 解析字段类型并写入 Data 表.
        for decl in raw.values() {
            let mut resolved = IndexMap::new();
            for (variant_name, fields) in &decl.variants {
                let mut field_types = Vec::new();
                for field in fields {
                    field_types.push(resolve_type(field, self.env)?);
                }
                resolved.insert(variant_name.clone(), field_types);
            }
            self.env
                .data_mut()
                .insert(decl.full.clone(), DataInfo { variants: resolved });
        }
        Ok(())
    }
}

/// 校验 Dict/Set 的键类型; 与运行时 Key 的取值范围一致.
pub(super) fn ensure_dict_key(ty: &Type, span: Span) -> Result<(), KslError> {
    if matches!(
        ty,
        Type::I64
            | Type::I32
            | Type::U8
            | Type::U32
            | Type::U64
            | Type::Char
            | Type::Bool
            | Type::Str
    ) {
        Ok(())
    } else {
        Err(KslError::new(
            ErrorKind::InvalidKeyType { found: ty.clone() },
            span,
        ))
    }
}

/// 把类型表达式解析为具体类型; 名字来源为内置类型与可见类型名绑定.
pub(super) fn resolve_type(ty: &TypeExpr, env: &TypeEnv) -> Result<Type, KslError> {
    match ty {
        TypeExpr::Named(name) => resolve_app(name, Vec::new(), env),
        TypeExpr::Generic { name, args } => {
            if args.is_empty() {
                return Err(unknown_type(name));
            }
            let args = args
                .iter()
                .map(|arg| resolve_type(arg, env))
                .collect::<Result<Vec<_>, _>>()?;
            resolve_app(name, args, env)
        },
        TypeExpr::Fn { params, ret, .. } => {
            let params = params
                .iter()
                .map(|param| resolve_type(param, env))
                .collect::<Result<Vec<_>, _>>()?;
            let ret = resolve_type(ret, env)?;
            Ok(Type::function(params, ret))
        },
    }
}

/// 解析一次类型应用: 具名类型视为零参数应用.
fn resolve_app(name: &Spanned<Name>, args: Vec<Type>, env: &TypeEnv) -> Result<Type, KslError> {
    let kw = keywords();
    let node = name.node.clone();
    if args.is_empty() {
        let ty = if node == kw.i64_ {
            Type::I64
        } else if node == kw.i32_ {
            Type::I32
        } else if node == kw.u8_ {
            Type::U8
        } else if node == kw.u32_ {
            Type::U32
        } else if node == kw.u64_ {
            Type::U64
        } else if node == kw.f32_ {
            Type::F32
        } else if node == kw.f64_ {
            Type::F64
        } else if node == kw.char_ {
            Type::Char
        } else if node == kw.bool_ {
            Type::Bool
        } else if node == kw.string_ {
            Type::Str
        } else if node == kw.nil_ {
            Type::Nil
        } else if let Some(target) = env.resolve(&node) {
            target.to_type()
        } else {
            return Err(unknown_type(name));
        };
        return Ok(ty);
    }
    if env.resolve(&node).is_some() {
        // 具名类型与不透明类型不带类型参数.
        return Err(unknown_type(name));
    }
    if node == kw.pid_ {
        // 最后一个参数是返回类型, 前面是各信箱的消息类型.
        if !(2..=17).contains(&args.len()) {
            return Err(KslError::new(
                ErrorKind::InvalidPidType {
                    mailboxes: args.len().saturating_sub(1),
                },
                name.span,
            ));
        }
        let mut args = args;
        let ret = args.pop().expect("Pid 至少两个参数");
        return Ok(Type::pid(args, ret));
    }
    if node == kw.handler_ {
        let mut args = args.into_iter();
        let Some(pid) = args.next() else {
            return Err(KslError::new(ErrorKind::InvalidHandlerType, name.span));
        };
        if !pid.is_pid() {
            return Err(KslError::new(ErrorKind::InvalidHandlerType, name.span));
        }
        return Ok(Type::handler(pid, args.collect()));
    }
    if node == kw.dict_ {
        let [key, value] = args.as_slice() else {
            return Err(type_arity(name.span, 2, args.len()));
        };
        ensure_dict_key(key, name.span)?;
        return Ok(Type::dict(key.clone(), value.clone()));
    }
    if node == kw.set_ {
        let [inner] = args.as_slice() else {
            return Err(type_arity(name.span, 1, args.len()));
        };
        ensure_dict_key(inner, name.span)?;
        return Ok(Type::set(inner.clone()));
    }
    match args.as_slice() {
        [inner] if node == kw.list_ => Ok(Type::list(inner.clone())),
        [a, b] if node == kw.pair_ => Ok(Type::pair(a.clone(), b.clone())),
        [inner] if node == kw.option_ => Ok(Type::option(inner.clone())),
        [ok, err] if node == kw.result_ => Ok(Type::result(ok.clone(), err.clone())),
        _ if node == kw.list_ || node == kw.option_ => {
            Err(type_arity(name.span, 1, args.len()))
        },
        _ if node == kw.pair_ || node == kw.result_ => {
            Err(type_arity(name.span, 2, args.len()))
        },
        _ => Err(unknown_type(name)),
    }
}

/// 类型构造器实参个数不符.
fn type_arity(span: Span, expected: usize, found: usize) -> KslError {
    KslError::new(ErrorKind::ArityMismatch { expected, found }, span)
}

fn unknown_type(name: &Spanned<Name>) -> KslError {
    KslError::unknown_type(&name.node, name.span)
}

/// 校验名字可绑定: 内置名一律保留.
pub(super) fn check_bindable(name: &Spanned<Name>) -> Result<(), KslError> {
    if reserved::is_reserved(&name.node) {
        return Err(KslError::reserved_name(&name.node, name.span));
    }
    Ok(())
}
