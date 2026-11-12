//! 检查会话与模块上下文入口.

use std::collections::HashMap;

use ksl_base::symbol::{Symbol, resolve};

use super::Checker;
use super::context::{ModuleContext, TypeContext};
use super::data::{DataTable, collect_data};
use super::display::TypeTable;
use super::env::Env;
use super::infer::InferCtx;
use super::types::{HandlerType, Type};
use crate::ast::{Expr, ExprKind, Special, TypeExpr};
use crate::error::{ErrorKind, KslError};
use crate::rewrite::{self, TypeNames};

/// 检查整个程序: 顶层语句序列加可选的尾表达式.
///
/// 语句值处置规则只对带分号的语句生效.
/// 尾表达式的值即程序值, 类型不受限制, 无需绑定.
///
/// # Errors
///
/// 返回首个类型错误.
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
                infer_types: indexmap::IndexMap::new(),
                infer: InferCtx::default(),
                pending_keys: Vec::new(),
                actor: None,
            },
        }
    }

    /// 检查一段输入; 作用域与数据类型声明跨调用保留.
    ///
    /// 检查在会话副本上进行, 成功后才提交.
    /// 因此失败不会留下半程绑定或半程 Data 声明.
    /// 若要求运行成功后才提交, 调用方应在调用前自行克隆会话.
    ///
    /// # Errors
    ///
    /// 返回首个类型错误; 会话保持原状.
    pub fn check(&mut self, program: &crate::ast::Program) -> Result<(), KslError> {
        self.check_with_tail(program).map(|_tail| ())
    }

    /// 检查一段输入并返回尾表达式类型.
    ///
    /// REPL 用尾表达式类型决定结果是否可打印. 类型检查仍在会话副本上进行,
    /// 成功后才提交.
    ///
    /// # Errors
    ///
    /// 返回首个类型错误; 会话保持原状.
    pub fn check_with_tail(
        &mut self,
        program: &crate::ast::Program,
    ) -> Result<Option<Type>, KslError> {
        let mut trial = self.clone();
        collect_data(
            program,
            &mut trial.checker.context.data,
            &trial.checker.context.opaque_types,
        )?;
        trial.reject_conflicting_types(program)?;
        let result = trial.checker.check_program_with_tail(program)?;
        *self = trial;
        Ok(result)
    }

    /// 模块检查会话: 复用已收集的 Data 表与导入值类型.
    #[must_use]
    pub fn for_module(data: DataTable, values: HashMap<Symbol, Type>) -> Self {
        let context = TypeContext::new(data, values);
        let env = module_env(context.values.clone());
        Self {
            checker: Checker {
                context,
                env,
                infer_types: indexmap::IndexMap::new(),
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
                infer_types: indexmap::IndexMap::new(),
                infer: InferCtx::default(),
                pending_keys: Vec::new(),
                actor: None,
            },
        }
    }

    /// 合并模块上下文: 数据类型与不透明类型名并入, 值类型覆盖同名绑定, 类型别名冲突时报错.
    ///
    /// 导入的类型名不得与本地声明的类型重名, 也不得指向不同的真实类型.
    /// 同一可见名指向同一真实类型时允许重复导入.
    ///
    /// # Errors
    ///
    /// 可见名与已有类型冲突时报错, 位置取导入项.
    pub fn merge_module_context(&mut self, context: ModuleContext) -> Result<(), KslError> {
        self.checker.context.data.merge(&context.data);
        for (name, ty) in context.values {
            self.checker.env.insert(name, ty);
        }
        for (name, alias) in context.type_names {
            self.check_type_name(name, alias)?;
        }
        self.checker.context.types.extend(context.types);
        self.checker.context.opaque_types.extend(context.opaque_types);
        Ok(())
    }

    /// 登记一个可见类型名; 与已有类型冲突时报错.
    fn check_type_name(
        &mut self,
        name: Symbol,
        alias: rewrite::TypeName,
    ) -> Result<(), KslError> {
        // 本地声明的类型占用可见名, 导入不可顶替.
        if self.checker.context.data.contains_key(&name) {
            return Err(KslError::with_subject(
                ErrorKind::DuplicateImport {
                    name: resolve(name).to_string(),
                },
                alias.span,
                resolve(name).to_string(),
            ));
        }
        // 同一可见名只可指向一个真实类型.
        if let Some(existing) = self.checker.context.type_names.get(&name)
            && existing.full != alias.full
        {
            return Err(KslError::with_subject(
                ErrorKind::DuplicateImport {
                    name: resolve(name).to_string(),
                },
                alias.span,
                resolve(name).to_string(),
            ));
        }
        self.checker.context.type_names.insert(name, alias);
        Ok(())
    }

    /// 类型别名表.
    #[must_use]
    pub fn type_names(&self) -> &TypeNames {
        &self.checker.context.type_names
    }

    /// 最近一次成功检查生成的类型表.
    #[must_use]
    pub fn types(&self) -> &TypeTable {
        &self.checker.context.types
    }

    /// 当前会话的全局绑定; REPL 命令展示用.
    #[must_use]
    pub fn bindings(&self) -> Vec<(Symbol, Type)> {
        self.checker.env.global_bindings()
    }

    /// 替换整张类型表. 调用方负责先合并一次输入内各语句段的类型表.
    pub fn replace_types(&mut self, types: TypeTable) {
        self.checker.context.types = types;
    }

    /// 在已收集 Data 表的前提下检查一个模块.
    ///
    /// # Errors
    ///
    /// 类型检查失败时返回首个错误.
    pub fn check_loaded(&mut self, program: &crate::ast::Program) -> Result<(), KslError> {
        self.checker.check_program(program)
    }

    /// 检查模块程序与入口声明, 返回入口蓝图类型.
    ///
    /// # Errors
    ///
    /// 类型检查失败时返回首个错误.
    pub fn check_loaded_with_main(
        &mut self,
        program: &crate::ast::Program,
        main: Option<&Expr>,
    ) -> Result<Option<HandlerType>, KslError> {
        self.checker.check_program_with_entry(program, main)
    }

    /// 检查一段带入口声明的程序; 先收集 Data 声明, 成功后才提交会话.
    ///
    /// 供脚本测试等单文件程序使用; 模块加载已有独立的数据收集流程.
    ///
    /// # Errors
    ///
    /// 类型检查失败时返回首个错误; 会话保持原状.
    pub fn check_with_entry(
        &mut self,
        program: &crate::ast::Program,
        main: Option<&Expr>,
    ) -> Result<Option<HandlerType>, KslError> {
        let mut trial = self.clone();
        collect_data(
            program,
            &mut trial.checker.context.data,
            &trial.checker.context.opaque_types,
        )?;
        trial.reject_conflicting_types(program)?;
        let entry = trial.checker.check_program_with_entry(program, main)?;
        *self = trial;
        Ok(entry)
    }

    /// 登记不透明类型的完全名, 供类型表达式解析.
    pub fn add_opaque_type(&mut self, full: Symbol) {
        self.checker.context.opaque_types.insert(full);
    }

    /// 登记 `@Link` 类型并允许其完全名参与类型解析.
    pub fn add_link_type(&mut self, local: Symbol, full: Symbol) {
        self.checker.context.add_link_type(local, full);
    }

    /// 登记 `@Link` 值并返回其解析后的类型.
    ///
    /// # Errors
    ///
    /// 类型表达式非法时报错; 保留名由模块链接阶段检查.
    pub fn declare_link_value(
        &mut self,
        name: Symbol,
        ty: &TypeExpr,
    ) -> Result<Type, KslError> {
        self.checker.context.add_link_value(name);
        let ty = self.checker.type_from_expr(ty)?;
        self.checker.env.insert(name, ty.clone());
        self.checker.context.values.insert(name, ty.clone());
        Ok(ty)
    }

    /// 读取当前模块顶层绑定的类型.
    #[must_use]
    pub fn value_type(&self, name: Symbol) -> Option<Type> {
        self.checker.env.lookup(name).cloned()
    }

    /// 本地类型声明与导入的类型可见名冲突时报错.
    ///
    /// 类型声明不可覆盖, 故不按遮蔽处理; 位置取本地声明处.
    fn reject_conflicting_types(&self, program: &crate::ast::Program) -> Result<(), KslError> {
        for stmt in program.exprs() {
            let ExprKind::Special(Special::Data { name, .. }) = &stmt.node else {
                continue;
            };
            if self.checker.context.type_names.contains_key(&name.node)
                || self.checker.context.link_types.contains(&name.node)
            {
                let kind = if self.checker.context.link_types.contains(&name.node) {
                    ErrorKind::LinkNameConflict {
                        name: resolve(name.node).to_string(),
                    }
                } else {
                    ErrorKind::DuplicateData
                };
                return Err(KslError::with_subject(
                    kind,
                    name.span,
                    resolve(name.node).to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// 按会话的类型别名改写程序中的类型引用.
///
/// 导入的类型在会话中以本地名可见.
/// 改写后程序直接引用真实类型名, 类型身份因此与模块内一致.
pub fn apply_type_names(session: &mut Session, program: &mut crate::ast::Program) {
    rewrite::rewrite_type_refs(program, session.type_names());
}

fn module_env(values: HashMap<Symbol, Type>) -> Env {
    let mut env = Env::default();
    for (name, ty) in values {
        env.insert(name, ty);
    }
    env
}
