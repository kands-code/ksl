//! 从 typed AST 降级为 CPS IR.
//!
//! 覆盖字面量, 名字, 列表, 配对, 纯内置调用, `@Let`, `@Block`, `@If`,
//! `@Fn`, `@Def` 与 `@Fix`; 用户函数按名字调用.
//! 降级产物交给线性化与抽象机执行.

use std::collections::HashSet;
use std::sync::Arc;

use ksl_base::fail::FailKind;
use ksl_base::name::{Name, keywords};
use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::builtin::{BuiltinId, is_pure};
use ksl_catalog::ops::{DictOpKind, OptionOpKind};
use ksl_typed::{
    Callee, CheckedBinder, CheckedDataDecl, CheckedDef, CheckedExpr, CheckedMatchArm,
    CheckedPattern, CheckedProgram, ExprKind, FStrPart, Pattern,
};

use crate::cps::{
    Binder, Binding, Cont, ContId, Cps, DataDecl, EvalKind, FStrPartIr, Function, FunctionKind,
    MatchArmIr, Module, OptionPrim, PatternIr, PrimOp, TempId, Value, VariantDecl,
};

/// 把 typed AST 降级为 CPS IR.
#[must_use]
pub fn to_cps(program: &CheckedProgram) -> Module {
    let mut lowerer = Lowerer::default();
    let body = lowerer.sequence(
        &program.stmts,
        program.tail.as_ref(),
        Target::Cont(ContId::RETURN),
        true,
    );
    let ret = program.tail.as_ref().map_or(Type::Nil, |tail| tail.ty.clone());
    let main = Function {
        name: Name::new("$main"),
        kind: FunctionKind::Fn,
        params: Vec::new(),
        ret,
        body: Arc::new(body),
        display_name: None,
    };
    let entry = program.main.as_ref().map(|main| {
        let ExprKind::Main { params, ret, body } = &main.kind else {
            unreachable!("typed program 的入口应是 @Main");
        };
        let name = lowerer.fresh_synthetic_function("$entry");
        lowerer.build_function(
            name,
            params,
            ret.clone(),
            body,
            FunctionKind::Handler,
            Some(Name::new("Main")),
        )
    });
    Module {
        data: lowerer.data,
        functions: lowerer.functions,
        main,
        entry,
    }
}

/// 降级目标: 已有续延, 或绑定结果后继续.
#[derive(Clone)]
enum Target {
    Cont(ContId),
    Bind(Binder, Arc<Cps>),
}

#[derive(Default)]
struct Lowerer {
    next_temp: u32,
    next_cont: u32,
    next_function: u32,
    used_functions: HashSet<Name>,
    functions: Vec<Function>,
    data: Vec<DataDecl>,
}

impl Lowerer {
    pub(super) fn fresh_temp(&mut self) -> TempId {
        let id = TempId::new(self.next_temp);
        self.next_temp += 1;
        id
    }

    pub(super) fn fresh_cont(&mut self) -> ContId {
        // 0 保留给函数返回续延 `$k`.
        self.next_cont += 1;
        ContId(self.next_cont)
    }

    pub(super) fn fresh_function(&mut self, base: Name) -> Name {
        if self.used_functions.insert(base.clone()) {
            return base;
        }
        loop {
            let candidate = Name::new(format!("{base}#{}", self.next_function));
            self.next_function += 1;
            if self.used_functions.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    /// 合成匿名函数; 前缀后接单调序号, 与源码名不会冲突.
    pub(super) fn fresh_synthetic_function(&mut self, prefix: &str) -> Name {
        let base = Name::new(format!("{prefix}{}", self.next_function));
        self.next_function += 1;
        self.fresh_function(base)
    }

    pub(super) fn sequence(
        &mut self,
        stmts: &[CheckedExpr],
        tail: Option<&CheckedExpr>,
        target: Target,
        top_level: bool,
    ) -> Cps {
        let mut body = match tail {
            Some(tail)
                if matches!(
                    &tail.kind,
                    ExprKind::Let { .. }
                        | ExprKind::Def(_)
                        | ExprKind::Fix(_)
                        | ExprKind::Data(_)
                        | ExprKind::Link
                ) =>
            {
                self.stmt(tail, goto(target, Value::Nil), top_level)
            },
            Some(tail) => self.lower_to(tail, target),
            None => goto(target, Value::Nil),
        };
        for stmt in stmts.iter().rev() {
            body = self.stmt(stmt, body, top_level);
        }
        body
    }
}

mod defs;
mod expr;
mod prim;
mod support;

use support::{goto, wrap_join};
