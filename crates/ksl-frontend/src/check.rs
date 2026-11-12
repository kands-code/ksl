//! 类型检查.
//!
//! 环境驱动的单趟检查.
//! `@Data` 声明先行收集, 语句按顺序检查, 环境随语句增长.
//! 求值器可信赖检查通过的程序.

mod collection;
mod context;
mod data;
mod display;
mod env;
mod expected;
mod expr;
mod finalize;
mod infer;
mod list;
mod option;
mod pattern;
mod session;
mod special;
mod type_expr;

use context::TypeContext;
pub use context::{ModuleContext, TypeBinding, TypeBindings, TypeEnv, TypeTarget};
pub use data::{DataInfo, DataTable, TypeDeclCollector};
pub use display::{display_type, is_base_printable, is_printable};
use env::Env;
pub(crate) use infer::{InferCtx, InferTy};
use ksl_base::name::Name;
use ksl_base::source::Span;
pub use ksl_base::types::{HandlerType, PidType, Type};
use ksl_catalog::reserved;
use ksl_diagnostics::error::KslError;
pub use session::{Session, check_program};

use crate::ast::Expr;

/// 检查期带类型的表达式; 降级前由 `finalize_program` 解析为 `CheckedExpr`.
pub(crate) type InferExpr = ksl_typed::Expr<InferTy>;
/// 检查期表达式形态.
pub(crate) type TypedKind = ksl_typed::ExprKind<InferTy>;

impl Checker {
    /// 在临时作用域内检查; 无论成功与否都恢复作用域栈.
    pub(super) fn with_scope<T>(
        &mut self,
        check: impl FnOnce(&mut Self) -> Result<T, KslError>,
    ) -> Result<T, KslError> {
        self.env.push();
        let result = check(self);
        self.env.pop();
        result
    }

    /// 在指定 Handler 上下文中检查; 结束后恢复原上下文.
    pub(super) fn with_actor<T>(
        &mut self,
        actor: Option<ActorContext>,
        check: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let saved = std::mem::replace(&mut self.actor, actor);
        let result = check(self);
        self.actor = saved;
        result
    }

    /// 取当前环境可见的 Data 声明.
    ///
    /// 值的类型可能来自导入签名, 而类型名本身未导入;
    /// 此时不能读取其变体或字段.
    ///
    /// # Errors
    ///
    /// 类型不可见时报用户错误; 已绑定却缺少声明时报内部错误.
    pub(super) fn visible_data(
        &self,
        identity: &Name,
        span: Span,
    ) -> Result<&DataInfo, KslError> {
        if let Some(info) = self.context.type_env.data().get(identity) {
            return Ok(info);
        }
        if self.context.type_env.is_visible(identity) {
            return Err(KslError::internal(
                format!("缺少 Data 声明: {identity}"),
                Some(span),
            ));
        }
        Err(KslError::type_not_visible(identity, span))
    }

    /// 取必定存在的 Data 声明; 缺失属实现错误.
    ///
    /// # Errors
    ///
    /// 类型名已绑定或声明刚收集, 声明缺失时报内部错误.
    pub(super) fn required_data(
        &self,
        identity: &Name,
        span: Span,
    ) -> Result<&DataInfo, KslError> {
        self.context.type_env.data().get(identity).ok_or_else(|| {
            KslError::internal(format!("缺少 Data 声明: {identity}"), Some(span))
        })
    }

    /// 检查一个实参并把 typed 节点写入按位置索引的缓冲区.
    pub(super) fn check_typed_arg(
        &mut self,
        typed: &mut [Option<InferExpr>],
        args: &[Expr],
        index: usize,
        expected: Option<&InferTy>,
    ) -> Result<InferTy, KslError> {
        let node = self.check_expr_expect(&args[index], expected)?;
        let ty = node.ty.clone();
        typed[index] = Some(node);
        Ok(ty)
    }

    /// 收集全部实参; 每个位置由检查器写入.
    pub(super) fn collect_typed_args(typed: Vec<Option<InferExpr>>) -> Vec<InferExpr> {
        typed
            .into_iter()
            .enumerate()
            .map(|(index, node)| node.unwrap_or_else(|| panic!("实参 {index} 未检查")))
            .collect()
    }
}

/// 判断名字是否为语言保留名. 供模块链接阶段检查本地别名与 REPL 着色.
#[must_use]
pub fn is_reserved(name: &Name) -> bool {
    reserved::is_reserved(name)
}

/// 内置类型名; 供编辑器名单核对使用.
#[must_use]
pub fn builtin_type_names() -> &'static [&'static str] {
    reserved::BUILTIN_TYPE_NAMES.as_slice()
}

/// 内置 Data 类型名; 模块导出检查使用.
#[must_use]
pub fn builtin_data_type_names() -> &'static [&'static str] {
    reserved::BUILTIN_DATA_TYPE_NAMES.as_slice()
}

/// 内置不透明类型名; 模块导出检查使用.
#[must_use]
pub fn builtin_opaque_type_names() -> &'static [&'static str] {
    reserved::BUILTIN_OPAQUE_TYPE_NAMES
}

/// 校验 `@Link` 声明的可恢复失败约定.
///
/// 函数返回 `Result[T, string]` 时, 插件失败以 `Result/Err[string]` 回到调用方;
/// 错误类型不是 `string` 的 `Result` 无法映射.
/// 参数与返回值不受其他限制, 按运行时值直接传递.
/// `names` 用于按当前模块的可见名显示实际错误类型.
///
/// # Errors
///
/// 返回错误类型文本.
pub fn validate_link_return(ty: &Type, names: &TypeBindings) -> Result<(), String> {
    let Type::Fn(_, ret) = ty else {
        return Ok(());
    };
    let Type::Result(_, err) = &**ret else {
        return Ok(());
    };
    if **err == Type::Str {
        return Ok(());
    }
    Err(format!(
        "Result 的错误类型必须是 string, 实际为 {}",
        display_type(err, names)
    ))
}

/// 当前 Handler 体检查上下文.
#[derive(Debug, Clone)]
struct ActorContext {
    handler: HandlerType,
}

#[derive(Clone)]
struct Checker {
    context: TypeContext,
    env: Env,
    /// 当前模块所在目录; 非模块输入为 None.
    module_dir: Option<String>,
    /// 本次检查的类型变量与统一状态.
    infer: InferCtx,
    /// 检查结束时仍需校验的 Dict 与 Set 键类型.
    ///
    /// 键类型可能由兄弟表达式或外层期望确定, 留到本轮检查结束再校验.
    pending_keys: Vec<(InferTy, Span)>,
    /// 当前是否位于 Handler 体内; 决定 Receive/Continue 等原语是否可用.
    actor: Option<ActorContext>,
}

#[cfg(test)]
mod tests;
