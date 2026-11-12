//! 函数自由变量分析.
//!
//! 闭包只捕获函数体真正引用的外部名字. 分析在 CPS 树上完成, 结果按首次
//! 出现顺序给出, 捕获向量与 `Capture` 下标都以此为准.

use std::collections::{HashMap, HashSet};

use crate::cps::ir::{
    Binding, Cps, FStrPartIr, Function as IrFunction, Module as IrModule, PatternIr, Value,
};
use crate::symbol::Symbol;

/// 每个函数的自由变量名, 按首次出现顺序.
///
/// 函数值会把被引用函数的自由变量带到引用点; 引用链可能成环, 因此从空表
/// 出发迭代到不动点, 直到不再有新名字加入.
pub(super) fn analyze(module: &IrModule) -> HashMap<Symbol, Vec<Symbol>> {
    let functions: Vec<&IrFunction> = module
        .functions
        .iter()
        .chain(std::iter::once(&module.main))
        .collect();
    let mut table: HashMap<Symbol, Vec<Symbol>> = functions
        .iter()
        .map(|function| (function.name, Vec::new()))
        .collect();
    loop {
        let next: HashMap<Symbol, Vec<Symbol>> = functions
            .iter()
            .map(|function| (function.name, function_free_vars(function, &table)))
            .collect();
        if next == table {
            return table;
        }
        table = next;
    }
}

fn function_free_vars(
    function: &IrFunction,
    functions: &HashMap<Symbol, Vec<Symbol>>,
) -> Vec<Symbol> {
    let mut bound = imbl::HashSet::new();
    for param in &function.params {
        if let Binding::Named(name) = param.target {
            bound.insert(name);
        }
    }
    let mut collector = Collector {
        functions,
        free: Vec::new(),
        seen: HashSet::new(),
    };
    collector.cps(&function.body, &bound);
    collector.free
}

struct Collector<'a> {
    functions: &'a HashMap<Symbol, Vec<Symbol>>,
    free: Vec<Symbol>,
    seen: HashSet<Symbol>,
}

impl Collector<'_> {
    /// 遍历 CPS 树并收集自由变量.
    ///
    /// 用显式栈代替递归: 只有一个后续的链在循环内直接下沉, 分支节点把
    /// 待处理分支压栈; 记录顺序仍与先序遍历一致. 长语句序列与长续延链
    /// 不再受调用栈深度限制. 作用域使用持久化集合, 逐条绑定只做结构共享,
    /// 不会因长序列产生 O(n²) 的集合复制.
    #[allow(clippy::too_many_lines)]
    fn cps(&mut self, cps: &Cps, bound: &imbl::HashSet<Symbol>) {
        let mut pending: Vec<(&Cps, imbl::HashSet<Symbol>)> = vec![(cps, bound.clone())];
        while let Some((root, root_bound)) = pending.pop() {
            let mut current = root;
            let mut scope = root_bound;
            loop {
                match current {
                    Cps::Jump { value, .. } | Cps::Exit(value) => {
                        self.value(value, &scope);
                        break;
                    },
                    Cps::TailCall { callee, args, .. } => {
                        self.value(callee, &scope);
                        for arg in args {
                            self.value(arg, &scope);
                        }
                        break;
                    },
                    Cps::Call {
                        callee, args, cont, ..
                    } => {
                        self.value(callee, &scope);
                        for arg in args {
                            self.value(arg, &scope);
                        }
                        for param in &cont.params {
                            if let Binding::Named(name) = param.target {
                                scope.insert(name);
                            }
                        }
                        current = cont.body.as_ref();
                    },
                    Cps::Prim { args, cont, .. } => {
                        for arg in args {
                            self.value(arg, &scope);
                        }
                        for param in &cont.params {
                            if let Binding::Named(name) = param.target {
                                scope.insert(name);
                            }
                        }
                        current = cont.body.as_ref();
                    },
                    Cps::Receive { timeout, cont, .. } => {
                        self.value(timeout, &scope);
                        for param in &cont.params {
                            if let Binding::Named(name) = param.target {
                                scope.insert(name);
                            }
                        }
                        current = cont.body.as_ref();
                    },
                    Cps::Let {
                        target,
                        value,
                        body,
                    } => {
                        self.value(value, &scope);
                        if let Binding::Named(name) = target.target {
                            scope.insert(name);
                        }
                        current = body.as_ref();
                    },
                    Cps::GlobalLet {
                        name, value, body, ..
                    } => {
                        self.value(value, &scope);
                        if let Some(name) = name {
                            scope.insert(*name);
                        }
                        current = body.as_ref();
                    },
                    Cps::LetRec { bindings, body } | Cps::GlobalLetRec { bindings, body } => {
                        for (name, _) in bindings {
                            scope.insert(*name);
                        }
                        // 线性化时先登记全部递归名, 再在同一个作用域内取闭包捕获;
                        // 被绑定函数的自由变量若不在该作用域内, 也必须由外层捕获.
                        for (_, function) in bindings {
                            self.referenced(*function, &scope);
                        }
                        current = body.as_ref();
                    },
                    Cps::LetCont { cont, body } => {
                        let mut inner = scope.clone();
                        for param in &cont.params {
                            if let Binding::Named(name) = param.target {
                                inner.insert(name);
                            }
                        }
                        // 续延体先于外层正文, 保持先序遍历顺序.
                        pending.push((body.as_ref(), scope));
                        scope = inner;
                        current = cont.body.as_ref();
                    },
                    Cps::If { cond, then, els } => {
                        self.value(cond, &scope);
                        pending.push((els.as_ref(), scope.clone()));
                        current = then.as_ref();
                    },
                    Cps::Match { value, arms } => {
                        self.value(value, &scope);
                        for arm in arms.iter().rev() {
                            let mut inner = scope.clone();
                            bind_pattern(&arm.pattern, &mut inner);
                            pending.push((arm.body.as_ref(), inner));
                        }
                        break;
                    },
                    Cps::Continue { args } => {
                        for arg in args {
                            self.value(arg, &scope);
                        }
                        break;
                    },
                    Cps::Halt => break,
                    Cps::Fail { message, .. } => {
                        self.value(message, &scope);
                        break;
                    },
                }
            }
        }
    }

    fn value(&mut self, value: &Value, bound: &imbl::HashSet<Symbol>) {
        match value {
            Value::Var(name) => self.add_free(*name, bound),
            Value::Temp(_)
            | Value::I64(_)
            | Value::U8(_)
            | Value::U64(_)
            | Value::F64(_)
            | Value::Char(_)
            | Value::Str(_)
            | Value::Bool(_)
            | Value::Nil
            | Value::Builtin(_) => {},
            Value::Func(function) => self.referenced(*function, bound),
            Value::List { items, .. } => {
                for item in items {
                    self.value(item, bound);
                }
            },
            Value::Pair { first, second, .. } => {
                self.value(first, bound);
                self.value(second, bound);
            },
            Value::Data { fields, .. } => {
                for field in fields {
                    self.value(field, bound);
                }
            },
            Value::FStr(parts) => {
                for part in parts {
                    if let FStrPartIr::Interp { value, .. } = part {
                        self.value(value, bound);
                    }
                }
            },
        }
    }

    /// 函数值在引用点展开被引用函数的自由变量; 已被此处绑定的名字无需外提.
    fn referenced(&mut self, function: Symbol, bound: &imbl::HashSet<Symbol>) {
        let names = self.functions.get(&function).cloned().unwrap_or_default();
        for name in names {
            self.add_free(name, bound);
        }
    }

    fn add_free(&mut self, name: Symbol, bound: &imbl::HashSet<Symbol>) {
        if !bound.contains(&name)
            && crate::builtin::id(name).is_none()
            && self.seen.insert(name)
        {
            self.free.push(name);
        }
    }
}

fn bind_pattern(pattern: &PatternIr, bound: &mut imbl::HashSet<Symbol>) {
    if let PatternIr::Variant { fields, .. } = pattern {
        for field in fields.iter().flatten() {
            if let Binding::Named(name) = field.target {
                bound.insert(name);
            }
        }
    }
}
