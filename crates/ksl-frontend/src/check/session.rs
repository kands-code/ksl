//! 检查会话与模块上下文入口.

use std::collections::HashMap;

use ksl_base::name::Name;
use ksl_base::types::Type;
use ksl_diagnostics::error::KslError;

use super::context::{ModuleContext, TypeContext};
use super::data::TypeDeclCollector;
use super::env::Env;
use super::infer::InferCtx;
use super::{Checker, TypeBindings, TypeEnv};
use crate::ast::{Expr, TypeExpr};

/// 检查整个程序: 顶层语句序列加可选的尾表达式.
///
/// 语句值处置规则只对带分号的语句生效.
/// 尾表达式的值即程序值, 类型不受限制, 无需绑定.
///
/// # Errors
pub fn check_program(program: &crate::ast::Program) -> Result<(), KslError> {
    Session::new().check(program)
}

/// 交互会话: 作用域与数据类型声明跨调用保留.
///
/// 交互模式下逐行求值, 后一行可见前一行定义的顶层名字.
/// 数据类型声明同样跨行累积.
#[derive(Clone)]
pub struct Session {
    checker: Checker,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// 新会话: 空环境.
    #[must_use]
    pub fn new() -> Self {
        Self {
            checker: Checker {
                context: TypeContext::default(),
                env: Env::default(),
                module_dir: None,
                infer: InferCtx::default(),
                pending_keys: Vec::new(),
                actor: None,
            },
        }
    }

    /// 复制会话; 事务检查在副本上进行.
    #[must_use]
    pub fn fork(&self) -> Self {
        self.clone()
    }

    /// 检查一段输入; 作用域与数据类型声明跨调用保留.
    ///
    /// 检查在会话副本上进行, 成功后才提交;
    /// 失败不留半程绑定或 Data 声明.
    /// 若要求运行成功后才提交, 调用方应在调用前自行 fork.
    ///
    /// # Errors
    ///
    /// 返回首个类型错误; 会话保持原状.
    pub fn check(&mut self, program: &crate::ast::Program) -> Result<(), KslError> {
        self.check_typed(program, None).map(|_| ())
    }

    /// 检查一段输入并返回 typed AST.
    ///
    /// 检查在副本上进行, 成功后才提交; 失败时会话保持原状.
    ///
    /// # Errors
    ///
    /// 返回首个类型错误; 会话保持原状.
    pub fn check_typed(
        &mut self,
        program: &crate::ast::Program,
        main: Option<&Expr>,
    ) -> Result<ksl_typed::CheckedProgram, KslError> {
        let mut trial = self.fork();
        let checked = trial.check_typed_in_place(program, main)?;
        *self = trial;
        Ok(checked)
    }

    /// 在会话上就地检查; 失败时可能保留部分收集结果.
    ///
    /// 需要事务语义时先 [`Self::fork`], 再在副本上调用本方法;
    /// 成功后由调用方决定是否提交整个会话.
    ///
    /// # Errors
    ///
    /// 返回首个类型错误.
    pub fn check_typed_in_place(
        &mut self,
        program: &crate::ast::Program,
        main: Option<&Expr>,
    ) -> Result<ksl_typed::CheckedProgram, KslError> {
        let result = (|| {
            TypeDeclCollector::new(&mut self.checker.context.type_env)
                .collect(program, |name| name)?;
            self.checker.check_program_typed(program, main, true)
        })();
        result
            .map_err(|error| error.with_type_names(self.checker.context.type_env.type_names()))
    }

    /// 模块检查会话: 复用已收集的 Data 表与导入值类型.
    ///
    /// `module_dir` 是模块文件所在目录的绝对路径, 供编译期常量展开.
    #[must_use]
    pub fn for_module(
        type_env: TypeEnv,
        values: HashMap<Name, Type>,
        module_dir: String,
    ) -> Self {
        let env = module_env(values.clone());
        Self {
            checker: Checker {
                context: TypeContext::new(type_env, values),
                env,
                module_dir: Some(module_dir),
                infer: InferCtx::default(),
                pending_keys: Vec::new(),
                actor: None,
            },
        }
    }

    /// 从模块上下文恢复交互会话.
    #[must_use]
    pub fn from_module_context(context: ModuleContext) -> Self {
        let env = module_env(context.values.clone());
        Self {
            checker: Checker {
                context,
                env,
                module_dir: None,
                infer: InferCtx::default(),
                pending_keys: Vec::new(),
                actor: None,
            },
        }
    }

    /// 合并模块上下文: 数据类型与类型名绑定并入, 值类型覆盖同名绑定.
    ///
    /// 导入的类型名不得与已有类型重名, 也不得指向不同的真实类型.
    /// 同一可见名指向同一真实类型时允许重复导入.
    ///
    /// # Errors
    ///
    /// 可见名与已有类型冲突时报错, 位置取导入项.
    pub fn merge_module_context(&mut self, context: ModuleContext) -> Result<(), KslError> {
        if let Some((name, binding)) = self.checker.context.type_env.merge(&context.type_env) {
            return Err(KslError::duplicate_import(&name, binding.span));
        }
        for (name, ty) in context.values {
            self.checker.env.insert(name, ty);
        }
        Ok(())
    }

    /// 类型名绑定表.
    #[must_use]
    pub fn type_bindings(&self) -> &TypeBindings {
        self.checker.context.type_env.bindings()
    }

    /// 检查期的类型环境.
    #[must_use]
    pub fn type_env(&self) -> &TypeEnv {
        &self.checker.context.type_env
    }

    /// 当前会话的全局绑定; REPL 命令展示用.
    #[must_use]
    pub fn bindings(&self) -> Vec<(Name, Type)> {
        self.checker.env.global_bindings()
    }

    /// 在已收集 Data 表的前提下就地检查模块, 返回 typed 程序.
    ///
    /// 调用方应为本次检查使用新会话; 检查失败时丢弃该会话.
    ///
    /// # Errors
    ///
    /// 类型检查失败时报错.
    pub fn check_loaded_typed_in_place(
        &mut self,
        program: &crate::ast::Program,
        main: Option<&Expr>,
    ) -> Result<ksl_typed::CheckedProgram, KslError> {
        let result = self.checker.check_program_typed(program, main, false);
        result
            .map_err(|error| error.with_type_names(self.checker.context.type_env.type_names()))
    }

    /// 登记 `@Link` 值并返回其解析后的类型.
    ///
    /// # Errors
    ///
    /// 类型表达式非法时报错; 保留名由模块链接阶段检查.
    pub fn declare_link_value(&mut self, name: Name, ty: &TypeExpr) -> Result<Type, KslError> {
        self.checker.context.add_link_value(name.clone());
        let ty = match self.checker.type_from_expr(ty) {
            Ok(ty) => ty,
            Err(error) => {
                return Err(error.with_type_names(self.checker.context.type_env.type_names()));
            },
        };
        self.checker.env.insert(name.clone(), ty.clone());
        self.checker.context.values.insert(name, ty.clone());
        Ok(ty)
    }

    /// 读取当前模块顶层绑定的类型.
    #[must_use]
    pub fn value_type(&self, name: &Name) -> Option<Type> {
        self.checker.env.lookup(name).cloned()
    }
}

fn module_env(values: HashMap<Name, Type>) -> Env {
    let mut env = Env::default();
    for (name, ty) in values {
        env.insert(name, ty);
    }
    env
}
