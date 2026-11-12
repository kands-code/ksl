//! 把 CPS 树线性化为基本块, 并完成闭包转换与槽位分配.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::free;
use super::model::{
    Block, BlockId, FStrPart, Function, Instr, MatchArm, Module, Operand, RecBinding, Target,
    Terminator, VarId, VarRef,
};
use crate::cps::ir::{
    Binder, Binding, Cont, ContId, Cps, FStrPartIr, Function as IrFunction, Module as IrModule,
    PatternIr, Value, VarKey,
};
use crate::symbol::Symbol;

/// 把 CPS 树模块线性化.
#[must_use]
pub fn linearize(module: &IrModule) -> Module {
    let free = free::analyze(module);
    let functions = module
        .functions
        .iter()
        .map(|function| linearize_function(function, &free))
        .collect();
    let main = linearize_function(&module.main, &free);
    Module {
        data: module.data.clone(),
        functions,
        main,
    }
}

fn linearize_function(function: &IrFunction, free: &HashMap<Symbol, Vec<Symbol>>) -> Function {
    let mut builder = Builder::new(free);
    let mut scope = Scope::new();
    let entry = builder.new_block(&function.params, &mut scope);
    builder.linearize(&function.body, entry, &scope);
    let free_vars = free.get(&function.name).cloned().unwrap_or_default();
    builder.finish(function, free_vars, entry)
}

/// 当前可见绑定; 解析前只记录函数内变量身份.
#[derive(Clone, Copy)]
struct Visible {
    var: VarId,
}

/// 当前可见绑定.
///
/// 每个 `@Let` 都会派生新作用域; 用持久化映射让 `clone` 保持 O(1),
/// 避免长语句序列在降级期出现 O(n²) 的哈希表复制.
type Scope = imbl::HashMap<VarKey, Visible>;

struct RawBlock {
    id: BlockId,
    slots: Vec<Binder>,
    /// 与槽位一一对应的变量身份; 用于活跃变量分析.
    slot_vars: Vec<VarId>,
    /// 解析前由边结构直接提供的参数个数.
    predefined: usize,
    param_count: usize,
    instrs: Vec<Instr>,
    term: Option<Terminator>,
}

/// 待线性化的 CPS 节点与目标基本块.
///
/// 分支与续延体作为独立任务处理; 单链任务在循环内一直推进到终止指令,
/// 长语句序列与长续延链不会加深调用栈.
struct Task<'ir> {
    cps: &'ir Cps,
    block: BlockId,
    scope: Scope,
}

struct Builder<'a> {
    free: &'a HashMap<Symbol, Vec<Symbol>>,
    blocks: Vec<RawBlock>,
    next_var: u32,
    var_binder: HashMap<VarId, Binder>,
    cont_blocks: HashMap<ContId, BlockId>,
}

impl<'a> Builder<'a> {
    fn new(free: &'a HashMap<Symbol, Vec<Symbol>>) -> Self {
        Self {
            free,
            blocks: Vec::new(),
            next_var: 0,
            var_binder: HashMap::new(),
            cont_blocks: HashMap::new(),
        }
    }

    fn fresh_var(&mut self, binder: &Binder) -> VarId {
        let id = VarId::from_index(self.next_var);
        self.next_var += 1;
        self.var_binder.insert(id, binder.clone());
        id
    }

    fn new_block(&mut self, params: &[Binder], scope: &mut Scope) -> BlockId {
        let id =
            BlockId::from_index(u32::try_from(self.blocks.len()).expect("基本块数量超出 u32"));
        let mut slot_vars = Vec::with_capacity(params.len());
        for binder in params {
            let var = self.fresh_var(binder);
            slot_vars.push(var);
            if let Some(key) = Option::<VarKey>::from(binder.target) {
                scope.insert(key, Visible { var });
            }
        }
        self.blocks.push(RawBlock {
            id,
            slots: params.to_vec(),
            slot_vars,
            predefined: params.len(),
            param_count: params.len(),
            instrs: Vec::new(),
            term: None,
        });
        id
    }

    fn emit(&mut self, block: BlockId, instr: Instr) {
        self.blocks[block.index() as usize].instrs.push(instr);
    }

    fn finish_block(&mut self, block: BlockId, term: Terminator) {
        let raw = &mut self.blocks[block.index() as usize];
        debug_assert!(raw.term.is_none(), "基本块被重复终止");
        raw.term = Some(term);
    }

    /// 分配一个局部槽位; `scope` 更新后, 后续正文可见该名字.
    fn alloc_local(&mut self, block: BlockId, binder: &Binder, scope: &mut Scope) -> u32 {
        let var = self.fresh_var(binder);
        let raw = &mut self.blocks[block.index() as usize];
        let slot = u32::try_from(raw.slots.len()).expect("槽位数量超出 u32");
        raw.slots.push(binder.clone());
        raw.slot_vars.push(var);
        if let Some(key) = Option::<VarKey>::from(binder.target) {
            scope.insert(key, Visible { var });
        }
        slot
    }

    /// 线性化一棵 CPS 树.
    ///
    /// 待处理节点放在显式栈上: 单链在循环内推进, 分支与续延体作为新任务
    /// 入栈. 块编号按任务处理顺序分配, 不再依赖调用栈深度.
    fn linearize(&mut self, cps: &Cps, block: BlockId, scope: &Scope) {
        let mut pending = vec![Task {
            cps,
            block,
            scope: scope.clone(),
        }];
        while let Some(task) = pending.pop() {
            self.walk(task.cps, task.block, task.scope, &mut pending);
        }
    }

    /// 推进一条 CPS 单链; 遇到分支或续延时把后续节点压栈.
    #[allow(clippy::too_many_lines)]
    fn walk<'ir>(
        &mut self,
        cps: &'ir Cps,
        block: BlockId,
        mut scope: Scope,
        pending: &mut Vec<Task<'ir>>,
    ) {
        let mut current = cps;
        loop {
            match current {
                Cps::Jump { cont, value } => {
                    if *cont == ContId::RETURN {
                        let value = self.operand(value, block, &scope);
                        self.finish_block(block, Terminator::Return { value });
                        return;
                    }
                    let target = self
                        .cont_blocks
                        .get(cont)
                        .copied()
                        .unwrap_or_else(|| panic!("续延 {cont} 尚未绑定"));
                    let value = self.operand(value, block, &scope);
                    self.finish_block(
                        block,
                        Terminator::Jump {
                            target,
                            args: vec![value],
                        },
                    );
                    return;
                },
                Cps::TailCall { callee, args, span } => {
                    let callee = self.operand(callee, block, &scope);
                    let args = self.operands(args, block, &scope);
                    self.finish_block(
                        block,
                        Terminator::TailCall {
                            callee,
                            args,
                            span: *span,
                        },
                    );
                    return;
                },
                Cps::Call {
                    callee,
                    args,
                    cont,
                    span,
                } => {
                    let target = self.cont_block(cont, &scope, pending);
                    let callee = self.operand(callee, block, &scope);
                    let args = self.operands(args, block, &scope);
                    self.finish_block(
                        block,
                        Terminator::Call {
                            callee,
                            args,
                            target,
                            live: Vec::new(),
                            span: *span,
                        },
                    );
                    return;
                },
                Cps::Prim {
                    op,
                    args,
                    arg_types,
                    ty,
                    cont,
                    span,
                } => {
                    let target = self.cont_block(cont, &scope, pending);
                    let args = self.operands(args, block, &scope);
                    self.finish_block(
                        block,
                        Terminator::Prim {
                            op: *op,
                            args,
                            arg_types: arg_types.clone(),
                            ty: ty.clone(),
                            target,
                            live: Vec::new(),
                            span: *span,
                        },
                    );
                    return;
                },
                Cps::Receive {
                    index,
                    timeout,
                    cont,
                    span,
                } => {
                    let target = self.cont_block(cont, &scope, pending);
                    let timeout = self.operand(timeout, block, &scope);
                    self.finish_block(
                        block,
                        Terminator::Receive {
                            index: *index,
                            timeout,
                            target,
                            live: Vec::new(),
                            span: *span,
                        },
                    );
                    return;
                },
                Cps::Let {
                    target,
                    value,
                    body,
                } => {
                    let mut next = scope.clone();
                    let slot = self.alloc_local(block, target, &mut next);
                    let value = self.operand(value, block, &scope);
                    self.emit(block, Instr::Let { slot, value });
                    scope = next;
                    current = body.as_ref();
                },
                Cps::GlobalLet {
                    name,
                    ty,
                    value,
                    body,
                } => {
                    let value = self.operand(value, block, &scope);
                    let mut next = scope.clone();
                    let slot = name.map(|name| {
                        let binder = Binder {
                            target: Binding::Named(name),
                            ty: ty.clone(),
                        };
                        self.alloc_local(block, &binder, &mut next)
                    });
                    self.emit(
                        block,
                        Instr::GlobalLet {
                            slot,
                            name: *name,
                            ty: ty.clone(),
                            value,
                        },
                    );
                    scope = next;
                    current = body.as_ref();
                },
                Cps::LetRec { bindings, body } => {
                    let mut next = scope.clone();
                    let rec = self.rec_bindings(bindings, block, &mut next);
                    self.emit(block, Instr::LetRec { bindings: rec });
                    scope = next;
                    current = body.as_ref();
                },
                Cps::GlobalLetRec { bindings, body } => {
                    let mut next = scope.clone();
                    let rec = self.rec_bindings(bindings, block, &mut next);
                    self.emit(block, Instr::GlobalLetRec { bindings: rec });
                    scope = next;
                    current = body.as_ref();
                },
                Cps::LetCont { cont, body } => {
                    let entry = self.cont_block(cont, &scope, pending);
                    self.cont_blocks.insert(cont.id, entry);
                    current = body.as_ref();
                },
                Cps::If { cond, then, els } => {
                    let cond = self.operand(cond, block, &scope);
                    let then_block = self.new_block(&[], &mut scope.clone());
                    let els_block = self.new_block(&[], &mut scope.clone());
                    // 分支按源码顺序处理; 栈后进先出, 故先压 else.
                    pending.push(Task {
                        cps: els.as_ref(),
                        block: els_block,
                        scope: scope.clone(),
                    });
                    pending.push(Task {
                        cps: then.as_ref(),
                        block: then_block,
                        scope: scope.clone(),
                    });
                    self.finish_block(
                        block,
                        Terminator::If {
                            cond,
                            then: Target {
                                block: then_block,
                                args: Vec::new(),
                            },
                            els: Target {
                                block: els_block,
                                args: Vec::new(),
                            },
                        },
                    );
                    return;
                },
                Cps::Match { value, arms } => {
                    let value = self.operand(value, block, &scope);
                    let mut lowered = Vec::with_capacity(arms.len());
                    let mut tasks = Vec::with_capacity(arms.len());
                    for arm in arms {
                        let params = pattern_params(&arm.pattern);
                        let mut inner = scope.clone();
                        let target = self.new_block(&params, &mut inner);
                        lowered.push(MatchArm {
                            pattern: arm.pattern.clone(),
                            target,
                            live: Vec::new(),
                        });
                        tasks.push(Task {
                            cps: arm.body.as_ref(),
                            block: target,
                            scope: inner,
                        });
                    }
                    // 分支按源码顺序处理; 栈后进先出, 故倒序压入.
                    pending.extend(tasks.into_iter().rev());
                    self.finish_block(
                        block,
                        Terminator::Match {
                            value,
                            arms: lowered,
                        },
                    );
                    return;
                },
                Cps::Continue { args } => {
                    let args = self.operands(args, block, &scope);
                    self.finish_block(block, Terminator::Continue { args });
                    return;
                },
                Cps::Exit(value) => {
                    let value = self.operand(value, block, &scope);
                    self.finish_block(block, Terminator::Exit { value });
                    return;
                },
                Cps::Halt => {
                    self.finish_block(block, Terminator::Halt);
                    return;
                },
                Cps::Fail {
                    kind,
                    message,
                    span,
                } => {
                    let message = self.operand(message, block, &scope);
                    self.finish_block(
                        block,
                        Terminator::Fail {
                            kind: *kind,
                            message,
                            span: *span,
                        },
                    );
                    return;
                },
            }
        }
    }

    /// 为新续延分配入口块, 并把续延体排入待处理任务.
    fn cont_block<'ir>(
        &mut self,
        cont: &'ir Cont,
        scope: &Scope,
        pending: &mut Vec<Task<'ir>>,
    ) -> BlockId {
        let mut inner = scope.clone();
        let entry = self.new_block(&cont.params, &mut inner);
        pending.push(Task {
            cps: cont.body.as_ref(),
            block: entry,
            scope: inner,
        });
        entry
    }

    fn operands(&mut self, values: &[Value], block: BlockId, scope: &Scope) -> Vec<Operand> {
        values
            .iter()
            .map(|value| self.operand(value, block, scope))
            .collect()
    }

    fn operand(&mut self, value: &Value, block: BlockId, scope: &Scope) -> Operand {
        match value {
            Value::I64(value) => Operand::I64(*value),
            Value::U8(value) => Operand::U8(*value),
            Value::U64(value) => Operand::U64(*value),
            Value::F64(value) => Operand::F64(*value),
            Value::Char(value) => Operand::Char(*value),
            Value::Str(value) => Operand::Str(Arc::clone(value)),
            Value::Bool(value) => Operand::Bool(*value),
            Value::Nil => Operand::Nil,
            Value::Builtin(id) => Operand::Builtin(*id),
            Value::Var(name) => Self::var_ref(VarKey::Named(*name), block, scope),
            Value::Temp(id) => Self::var_ref(VarKey::Temp(*id), block, scope),
            Value::Func(name) => {
                let captures = self.captures(*name, block, scope);
                Operand::Func {
                    name: *name,
                    captures,
                }
            },
            Value::List { elem, items } => Operand::List {
                elem: elem.clone(),
                items: self.operands(items, block, scope),
            },
            Value::Pair { ty, first, second } => Operand::Pair {
                ty: ty.clone(),
                first: Box::new(self.operand(first, block, scope)),
                second: Box::new(self.operand(second, block, scope)),
            },
            Value::Data {
                ty,
                variant,
                fields,
            } => Operand::Data {
                ty: *ty,
                variant: *variant,
                fields: self.operands(fields, block, scope),
            },
            Value::FStr(parts) => Operand::FStr(
                parts
                    .iter()
                    .map(|part| match part {
                        FStrPartIr::Text(text) => FStrPart::Text(text.clone()),
                        FStrPartIr::Interp { value, spec } => FStrPart::Interp {
                            value: self.operand(value, block, scope),
                            spec: spec.clone(),
                        },
                    })
                    .collect(),
            ),
        }
    }

    fn var_ref(key: VarKey, _block: BlockId, scope: &Scope) -> Operand {
        if let Some(visible) = scope.get(&key) {
            return Operand::Var(VarRef::Id(visible.var));
        }
        match key {
            VarKey::Named(name) => match crate::builtin::id(name) {
                Some(id) => Operand::Builtin(id),
                None => Operand::Var(VarRef::Free(name)),
            },
            VarKey::Temp(id) => panic!("未绑定的临时值: {id}"),
        }
    }

    fn captures(&self, function: Symbol, block: BlockId, scope: &Scope) -> Vec<Operand> {
        let names = self.free.get(&function).cloned().unwrap_or_default();
        names
            .iter()
            .map(|name| Self::var_ref(VarKey::Named(*name), block, scope))
            .collect()
    }

    fn rec_bindings(
        &mut self,
        bindings: &[(Symbol, Symbol)],
        block: BlockId,
        scope: &mut Scope,
    ) -> Vec<RecBinding> {
        let mut slots = Vec::with_capacity(bindings.len());
        for (name, _) in bindings {
            let binder = Binder {
                target: Binding::Named(*name),
                ty: crate::check::Type::Nil,
            };
            let slot = self.alloc_local(block, &binder, scope);
            slots.push(slot);
        }
        bindings
            .iter()
            .zip(slots)
            .map(|((name, function), slot)| RecBinding {
                name: *name,
                slot,
                function: *function,
                captures: self.captures(*function, block, scope),
            })
            .collect()
    }

    fn finish(self, function: &IrFunction, free_vars: Vec<Symbol>, entry: BlockId) -> Function {
        let mut raw: Vec<RawBlock> = self.blocks;
        let terms: Vec<Terminator> = raw
            .iter_mut()
            .map(|block| block.term.take().expect("基本块缺少终止指令"))
            .collect();
        let uses: Vec<HashSet<VarId>> = raw
            .iter()
            .zip(&terms)
            .map(|(block, term)| block_uses(&block.instrs, term))
            .collect();
        let defs: Vec<HashSet<VarId>> = raw
            .iter()
            .map(|block| block.slot_vars.iter().copied().collect())
            .collect();
        let succs = block_successors(&terms);
        let live_in = liveness(&uses, &defs, &succs);

        let mut extra_by_block: Vec<Vec<VarId>> = Vec::with_capacity(raw.len());
        let mut local_maps: Vec<HashMap<VarId, u32>> = Vec::with_capacity(raw.len());
        for (index, block) in raw.iter_mut().enumerate() {
            let mut extra: Vec<VarId> = live_in[index]
                .iter()
                .copied()
                .filter(|var| !defs[index].contains(var))
                .collect();
            extra.sort_unstable();
            let added = u32::try_from(extra.len()).expect("参数数量超出 u32");
            if added > 0 {
                shift_slots(&mut block.instrs, added);
                let insert_at = block.param_count;
                let binders: Vec<Binder> =
                    extra.iter().map(|var| self.var_binder[var].clone()).collect();
                block.slots.splice(insert_at..insert_at, binders);
                block
                    .slot_vars
                    .splice(insert_at..insert_at, extra.iter().copied());
                block.param_count += extra.len();
            }
            let mut map = HashMap::with_capacity(block.slot_vars.len());
            for (slot, var) in block.slot_vars.iter().enumerate() {
                map.insert(*var, u32::try_from(slot).expect("槽位数量超出 u32"));
            }
            local_maps.push(map);
            extra_by_block.push(extra);
        }

        let free_index: HashMap<Symbol, u32> = free_vars
            .iter()
            .enumerate()
            .map(|(index, name)| (*name, u32::try_from(index).expect("自由变量数量超出 u32")))
            .collect();

        let predefined: Vec<usize> = raw.iter().map(|block| block.predefined).collect();
        let mut blocks = Vec::with_capacity(raw.len());
        for (index, (mut block, mut term)) in raw.into_iter().zip(terms).enumerate() {
            let local = &local_maps[index];
            for instr in &mut block.instrs {
                rewrite_instr(instr, local, &free_index);
            }
            rewrite_term(&mut term, local, &free_index);
            append_edge_args(&mut term, index, &extra_by_block, &local_maps);
            blocks.push(Block {
                id: block.id,
                slots: block.slots,
                param_count: block.param_count,
                instrs: block.instrs,
                term,
            });
        }
        validate_edges(&blocks, &predefined);

        Function {
            name: function.name,
            kind: function.kind,
            params: function.params.clone(),
            free_vars,
            ret: function.ret.clone(),
            blocks,
            entry,
            display_name: function.display_name,
        }
    }
}

fn validate_edges(blocks: &[Block], predefined: &[usize]) {
    for block in blocks {
        match &block.term {
            Terminator::Jump { target, args } => {
                debug_assert_eq!(args.len(), block_param_count(blocks, *target));
            },
            Terminator::Call { target, live, .. }
            | Terminator::Prim { target, live, .. }
            | Terminator::Receive { target, live, .. } => {
                debug_assert_eq!(live.len() + 1, block_param_count(blocks, *target));
            },
            Terminator::If { then, els, .. } => {
                debug_assert_eq!(then.args.len(), block_param_count(blocks, then.block));
                debug_assert_eq!(els.args.len(), block_param_count(blocks, els.block));
            },
            Terminator::Match { arms, .. } => {
                for arm in arms {
                    debug_assert_eq!(
                        arm.live.len() + predefined[arm.target.index() as usize],
                        block_param_count(blocks, arm.target)
                    );
                }
            },
            Terminator::Return { .. }
            | Terminator::TailCall { .. }
            | Terminator::Continue { .. }
            | Terminator::Exit { .. }
            | Terminator::Halt
            | Terminator::Fail { .. } => {},
        }
    }
}

fn block_param_count(blocks: &[Block], block: BlockId) -> usize {
    blocks[block.index() as usize].param_count
}

fn pattern_params(pattern: &PatternIr) -> Vec<Binder> {
    let PatternIr::Variant { fields, .. } = pattern else {
        return Vec::new();
    };
    fields
        .iter()
        .map(|field| {
            field.clone().unwrap_or(Binder {
                target: Binding::Discard,
                ty: crate::check::Type::Nil,
            })
        })
        .collect()
}

fn shift_slots(instrs: &mut [Instr], added: u32) {
    for instr in instrs {
        match instr {
            Instr::Let { slot, .. } => *slot += added,
            Instr::GlobalLet { slot, .. } => {
                if let Some(slot) = slot {
                    *slot += added;
                }
            },
            Instr::LetRec { bindings } | Instr::GlobalLetRec { bindings } => {
                for binding in bindings {
                    binding.slot += added;
                }
            },
        }
    }
}

fn block_uses(instrs: &[Instr], term: &Terminator) -> HashSet<VarId> {
    let mut uses = HashSet::new();
    for instr in instrs {
        match instr {
            Instr::Let { value, .. } | Instr::GlobalLet { value, .. } => {
                operand_uses(value, &mut uses);
            },
            Instr::LetRec { bindings } | Instr::GlobalLetRec { bindings } => {
                for binding in bindings {
                    for capture in &binding.captures {
                        operand_uses(capture, &mut uses);
                    }
                }
            },
        }
    }
    term_uses(term, &mut uses);
    uses
}

fn term_uses(term: &Terminator, uses: &mut HashSet<VarId>) {
    match term {
        Terminator::Return { value } | Terminator::Exit { value } => operand_uses(value, uses),
        Terminator::Fail { message, .. } => operand_uses(message, uses),
        Terminator::Jump { args, .. } | Terminator::Continue { args } => {
            for arg in args {
                operand_uses(arg, uses);
            }
        },
        Terminator::TailCall { callee, args, .. } => {
            operand_uses(callee, uses);
            for arg in args {
                operand_uses(arg, uses);
            }
        },
        Terminator::Call {
            callee, args, live, ..
        } => {
            operand_uses(callee, uses);
            for arg in args.iter().chain(live) {
                operand_uses(arg, uses);
            }
        },
        Terminator::Prim { args, live, .. } => {
            for arg in args.iter().chain(live) {
                operand_uses(arg, uses);
            }
        },
        Terminator::Receive { timeout, live, .. } => {
            operand_uses(timeout, uses);
            for arg in live {
                operand_uses(arg, uses);
            }
        },
        Terminator::If { cond, then, els } => {
            operand_uses(cond, uses);
            for arg in then.args.iter().chain(&els.args) {
                operand_uses(arg, uses);
            }
        },
        Terminator::Match { value, arms } => {
            operand_uses(value, uses);
            for arm in arms {
                for arg in &arm.live {
                    operand_uses(arg, uses);
                }
            }
        },
        Terminator::Halt => {},
    }
}

fn operand_uses(operand: &Operand, uses: &mut HashSet<VarId>) {
    match operand {
        Operand::Var(VarRef::Id(var)) => {
            uses.insert(*var);
        },
        Operand::Var(_)
        | Operand::I64(_)
        | Operand::U8(_)
        | Operand::U64(_)
        | Operand::F64(_)
        | Operand::Char(_)
        | Operand::Str(_)
        | Operand::Bool(_)
        | Operand::Nil
        | Operand::Builtin(_) => {},
        Operand::Func { captures, .. } => {
            for capture in captures {
                operand_uses(capture, uses);
            }
        },
        Operand::List { items, .. } => {
            for item in items {
                operand_uses(item, uses);
            }
        },
        Operand::Pair { first, second, .. } => {
            operand_uses(first, uses);
            operand_uses(second, uses);
        },
        Operand::Data { fields, .. } => {
            for field in fields {
                operand_uses(field, uses);
            }
        },
        Operand::FStr(parts) => {
            for part in parts {
                if let FStrPart::Interp { value, .. } = part {
                    operand_uses(value, uses);
                }
            }
        },
    }
}

fn block_successors(terms: &[Terminator]) -> Vec<Vec<usize>> {
    terms
        .iter()
        .map(|term| {
            let mut succ = Vec::new();
            match term {
                Terminator::Jump { target, .. }
                | Terminator::Call { target, .. }
                | Terminator::Prim { target, .. }
                | Terminator::Receive { target, .. } => {
                    succ.push(target.index() as usize);
                },
                Terminator::If { then, els, .. } => {
                    succ.push(then.block.index() as usize);
                    succ.push(els.block.index() as usize);
                },
                Terminator::Match { arms, .. } => {
                    for arm in arms {
                        succ.push(arm.target.index() as usize);
                    }
                },
                Terminator::Return { .. }
                | Terminator::TailCall { .. }
                | Terminator::Continue { .. }
                | Terminator::Exit { .. }
                | Terminator::Halt
                | Terminator::Fail { .. } => {},
            }
            succ
        })
        .collect()
}

fn liveness(
    uses: &[HashSet<VarId>],
    defs: &[HashSet<VarId>],
    succs: &[Vec<usize>],
) -> Vec<HashSet<VarId>> {
    let mut live_in: Vec<HashSet<VarId>> = vec![HashSet::new(); uses.len()];
    loop {
        let mut changed = false;
        for index in (0..uses.len()).rev() {
            // 每个变量在函数内只有一处定义, 因此块内定义直接从活跃集
            // 中扣除; 这里处理的是 upward-exposed use.
            let mut next: HashSet<VarId> = uses[index]
                .iter()
                .copied()
                .filter(|var| !defs[index].contains(var))
                .collect();
            for succ in &succs[index] {
                for var in &live_in[*succ] {
                    if !defs[index].contains(var) {
                        next.insert(*var);
                    }
                }
            }
            if next != live_in[index] {
                live_in[index] = next;
                changed = true;
            }
        }
        if !changed {
            return live_in;
        }
    }
}

fn rewrite_instr(instr: &mut Instr, local: &HashMap<VarId, u32>, free: &HashMap<Symbol, u32>) {
    match instr {
        Instr::Let { value, .. } | Instr::GlobalLet { value, .. } => {
            rewrite_operand(value, local, free);
        },
        Instr::LetRec { bindings } | Instr::GlobalLetRec { bindings } => {
            for binding in bindings {
                for capture in &mut binding.captures {
                    rewrite_operand(capture, local, free);
                }
            }
        },
    }
}

fn rewrite_term(
    term: &mut Terminator,
    local: &HashMap<VarId, u32>,
    free: &HashMap<Symbol, u32>,
) {
    match term {
        Terminator::Return { value } | Terminator::Exit { value } => {
            rewrite_operand(value, local, free);
        },
        Terminator::Fail { message, .. } => {
            rewrite_operand(message, local, free);
        },
        Terminator::Jump { args, .. } | Terminator::Continue { args } => {
            for arg in args {
                rewrite_operand(arg, local, free);
            }
        },
        Terminator::TailCall { callee, args, .. } => {
            rewrite_operand(callee, local, free);
            for arg in args {
                rewrite_operand(arg, local, free);
            }
        },
        Terminator::Call {
            callee, args, live, ..
        } => {
            rewrite_operand(callee, local, free);
            for arg in args.iter_mut().chain(live) {
                rewrite_operand(arg, local, free);
            }
        },
        Terminator::Prim { args, live, .. } => {
            for arg in args.iter_mut().chain(live) {
                rewrite_operand(arg, local, free);
            }
        },
        Terminator::Receive { timeout, live, .. } => {
            rewrite_operand(timeout, local, free);
            for arg in live {
                rewrite_operand(arg, local, free);
            }
        },
        Terminator::If { cond, then, els } => {
            rewrite_operand(cond, local, free);
            for arg in then.args.iter_mut().chain(&mut els.args) {
                rewrite_operand(arg, local, free);
            }
        },
        Terminator::Match { value, arms } => {
            rewrite_operand(value, local, free);
            for arm in arms {
                for arg in &mut arm.live {
                    rewrite_operand(arg, local, free);
                }
            }
        },
        Terminator::Halt => {},
    }
}

fn rewrite_operand(
    operand: &mut Operand,
    local: &HashMap<VarId, u32>,
    free: &HashMap<Symbol, u32>,
) {
    match operand {
        Operand::Var(VarRef::Id(var)) => {
            let slot = local.get(var).unwrap_or_else(|| panic!("变量 {var} 没有槽位"));
            *operand = Operand::Var(VarRef::Local(*slot));
        },
        Operand::Var(VarRef::Free(name)) => {
            let index = free
                .get(name)
                .unwrap_or_else(|| panic!("名字 {name} 不在自由变量表中"));
            *operand = Operand::Var(VarRef::Capture(*index));
        },
        Operand::Var(VarRef::Local(_) | VarRef::Capture(_))
        | Operand::I64(_)
        | Operand::U8(_)
        | Operand::U64(_)
        | Operand::F64(_)
        | Operand::Char(_)
        | Operand::Str(_)
        | Operand::Bool(_)
        | Operand::Nil
        | Operand::Builtin(_) => {},
        Operand::Func { captures, .. } => {
            for capture in captures {
                rewrite_operand(capture, local, free);
            }
        },
        Operand::List { items, .. } => {
            for item in items {
                rewrite_operand(item, local, free);
            }
        },
        Operand::Pair { first, second, .. } => {
            rewrite_operand(first, local, free);
            rewrite_operand(second, local, free);
        },
        Operand::Data { fields, .. } => {
            for field in fields {
                rewrite_operand(field, local, free);
            }
        },
        Operand::FStr(parts) => {
            for part in parts {
                if let FStrPart::Interp { value, .. } = part {
                    rewrite_operand(value, local, free);
                }
            }
        },
    }
}

fn append_edge_args(
    term: &mut Terminator,
    source: usize,
    extra_by_block: &[Vec<VarId>],
    local_maps: &[HashMap<VarId, u32>],
) {
    match term {
        Terminator::Jump { target, args } => {
            append_live(*target, args, source, extra_by_block, local_maps);
        },
        Terminator::Call { target, live, .. }
        | Terminator::Prim { target, live, .. }
        | Terminator::Receive { target, live, .. } => {
            append_live(*target, live, source, extra_by_block, local_maps);
        },
        Terminator::If { then, els, .. } => {
            append_target_args(then, source, extra_by_block, local_maps);
            append_target_args(els, source, extra_by_block, local_maps);
        },
        Terminator::Match { arms, .. } => {
            for arm in arms {
                let extra = &extra_by_block[arm.target.index() as usize];
                arm.live = extra
                    .iter()
                    .map(|var| live_operand(*var, source, local_maps))
                    .collect();
            }
        },
        Terminator::Return { .. }
        | Terminator::TailCall { .. }
        | Terminator::Continue { .. }
        | Terminator::Exit { .. }
        | Terminator::Halt
        | Terminator::Fail { .. } => {},
    }
}

fn append_target_args(
    target: &mut Target,
    source: usize,
    extra_by_block: &[Vec<VarId>],
    local_maps: &[HashMap<VarId, u32>],
) {
    let extra = &extra_by_block[target.block.index() as usize];
    target.args = extra
        .iter()
        .map(|var| live_operand(*var, source, local_maps))
        .collect();
}

fn append_live(
    target: BlockId,
    live: &mut Vec<Operand>,
    source: usize,
    extra_by_block: &[Vec<VarId>],
    local_maps: &[HashMap<VarId, u32>],
) {
    let extra = &extra_by_block[target.index() as usize];
    live.extend(extra.iter().map(|var| live_operand(*var, source, local_maps)));
}

fn live_operand(var: VarId, source: usize, local_maps: &[HashMap<VarId, u32>]) -> Operand {
    let slot = local_maps[source]
        .get(&var)
        .unwrap_or_else(|| panic!("跳转源缺少变量 {var} 的槽位"));
    Operand::Var(VarRef::Local(*slot))
}
