//! 把 CPS 树线性化为基本块, 并完成闭包转换与槽位分配.

use std::collections::{HashMap, HashSet};

use ksl_base::source::Span;
use ksl_base::symbol::Symbol;
use ksl_frontend::ast::FailKind;

use super::free;
use super::model::{
    Block, BlockId, FStrPart, Function, Instr, MatchArm, Module, Operand, RecBinding, Target,
    Terminator, VarId, VarRef,
};
use crate::cps::ir::{
    Binder, Binding, Cont, ContId, Cps, FStrPartIr, Function as IrFunction, MatchArmIr,
    Module as IrModule, PatternIr, PrimOp, Value, VarKey,
};

mod analysis;

use analysis::{
    append_edge_args, block_successors, block_uses, liveness, rewrite_instr, rewrite_term,
    shift_slots, validate_edges,
};

/// 把 CPS 树模块线性化.
#[must_use]
pub fn linearize(module: &IrModule) -> Module {
    let free = free::analyze(module);
    let functions = module
        .functions
        .iter()
        .map(|function| {
            let mut function = linearize_function(function, &free);
            super::optimize::fuse_pure_calls(&mut function);
            function
        })
        .collect();
    let mut main = linearize_function(&module.main, &free);
    super::optimize::fuse_pure_calls(&mut main);
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

/// 变量键到可见绑定的映射.
///
/// 每个 `@Let` 都会派生新作用域.
/// 用持久化映射让 `clone` 保持 O(1), 避免长语句序列在降级期出现 O(n²) 的哈希表复制.
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
/// 分支与续延体作为独立任务处理.
/// 单链任务在循环内一直推进到终止指令, 长语句序列与长续延链不会加深调用栈.
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
    /// 待处理节点放在显式栈上.
    /// 单链在循环内推进, 分支与续延体作为新任务入栈.
    /// 块编号按任务处理顺序分配, 与调用栈深度无关.
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
                    self.finish_jump(block, &scope, *cont, value);
                    return;
                },
                Cps::TailCall { callee, args, span } => {
                    self.finish_tail_call(block, &scope, callee, args, *span);
                    return;
                },
                Cps::Call {
                    callee,
                    args,
                    cont,
                    span,
                } => {
                    self.finish_call(block, &scope, pending, callee, args, cont, *span);
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
                    self.finish_prim(
                        block,
                        &scope,
                        pending,
                        *op,
                        args,
                        arg_types,
                        ty,
                        cont,
                        *span,
                    );
                    return;
                },
                Cps::Receive {
                    index,
                    timeout,
                    cont,
                    span,
                } => {
                    self.finish_receive(block, &scope, pending, *index, timeout, cont, *span);
                    return;
                },
                Cps::If { cond, then, els } => {
                    self.finish_if(block, &scope, pending, cond, then, els);
                    return;
                },
                Cps::Match { value, arms } => {
                    self.finish_match(block, &scope, pending, value, arms);
                    return;
                },
                Cps::Continue { args } => {
                    self.finish_continue(block, &scope, args);
                    return;
                },
                Cps::Exit(value) => {
                    self.finish_exit(block, &scope, value);
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
                    self.finish_fail(block, &scope, *kind, message, *span);
                    return;
                },
                _ => {
                    let (next, next_scope) = self.advance(current, block, &scope, pending);
                    current = next;
                    scope = next_scope;
                },
            }
        }
    }

    /// 推进非终止节点; 返回后继节点与更新后的作用域.
    fn advance<'ir>(
        &mut self,
        cps: &'ir Cps,
        block: BlockId,
        scope: &Scope,
        pending: &mut Vec<Task<'ir>>,
    ) -> (&'ir Cps, Scope) {
        match cps {
            Cps::Let {
                target,
                value,
                body,
            } => {
                let mut next = scope.clone();
                let slot = self.alloc_local(block, target, &mut next);
                let value = self.operand(value, block, scope);
                self.emit(block, Instr::Let { slot, value });
                (body.as_ref(), next)
            },
            Cps::GlobalLet {
                name,
                ty,
                value,
                body,
            } => {
                let value = self.operand(value, block, scope);
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
                (body.as_ref(), next)
            },
            Cps::LetRec { bindings, body } => {
                let mut next = scope.clone();
                let rec = self.rec_bindings(bindings, block, &mut next);
                self.emit(block, Instr::LetRec { bindings: rec });
                (body.as_ref(), next)
            },
            Cps::GlobalLetRec { bindings, body } => {
                let mut next = scope.clone();
                let rec = self.rec_bindings(bindings, block, &mut next);
                self.emit(block, Instr::GlobalLetRec { bindings: rec });
                (body.as_ref(), next)
            },
            Cps::LetCont { cont, body } => {
                let entry = self.cont_block(cont, scope, pending);
                self.cont_blocks.insert(cont.id, entry);
                (body.as_ref(), scope.clone())
            },
            _ => unreachable!("终止节点不在单链推进中处理"),
        }
    }

    /// 结束基本块: 把值交给续延; `RETURN` 表示返回.
    fn finish_jump(&mut self, block: BlockId, scope: &Scope, cont: ContId, value: &Value) {
        if cont == ContId::RETURN {
            let value = self.operand(value, block, scope);
            self.finish_block(block, Terminator::Return { value });
            return;
        }
        let target = self
            .cont_blocks
            .get(&cont)
            .copied()
            .unwrap_or_else(|| panic!("续延 {cont} 尚未绑定"));
        let value = self.operand(value, block, scope);
        self.finish_block(
            block,
            Terminator::Jump {
                target,
                args: vec![value],
            },
        );
    }

    fn finish_tail_call(
        &mut self,
        block: BlockId,
        scope: &Scope,
        callee: &Value,
        args: &[Value],
        span: Span,
    ) {
        let callee = self.operand(callee, block, scope);
        let args = self.operands(args, block, scope);
        self.finish_block(block, Terminator::TailCall { callee, args, span });
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_call<'ir>(
        &mut self,
        block: BlockId,
        scope: &Scope,
        pending: &mut Vec<Task<'ir>>,
        callee: &Value,
        args: &[Value],
        cont: &'ir Cont,
        span: Span,
    ) {
        let target = self.cont_block(cont, scope, pending);
        let callee = self.operand(callee, block, scope);
        let args = self.operands(args, block, scope);
        self.finish_block(
            block,
            Terminator::Call {
                callee,
                args,
                target,
                live: Vec::new(),
                span,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_prim<'ir>(
        &mut self,
        block: BlockId,
        scope: &Scope,
        pending: &mut Vec<Task<'ir>>,
        op: PrimOp,
        args: &[Value],
        arg_types: &[ksl_frontend::check::Type],
        ty: &ksl_frontend::check::Type,
        cont: &'ir Cont,
        span: Span,
    ) {
        let target = self.cont_block(cont, scope, pending);
        let args = self.operands(args, block, scope);
        self.finish_block(
            block,
            Terminator::Prim {
                op,
                args,
                arg_types: arg_types.to_vec(),
                ty: ty.clone(),
                target,
                live: Vec::new(),
                span,
            },
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_receive<'ir>(
        &mut self,
        block: BlockId,
        scope: &Scope,
        pending: &mut Vec<Task<'ir>>,
        index: usize,
        timeout: &Value,
        cont: &'ir Cont,
        span: Span,
    ) {
        let target = self.cont_block(cont, scope, pending);
        let timeout = self.operand(timeout, block, scope);
        self.finish_block(
            block,
            Terminator::Receive {
                index,
                timeout,
                target,
                live: Vec::new(),
                span,
            },
        );
    }

    fn finish_if<'ir>(
        &mut self,
        block: BlockId,
        scope: &Scope,
        pending: &mut Vec<Task<'ir>>,
        cond: &Value,
        then: &'ir Cps,
        els: &'ir Cps,
    ) {
        let cond = self.operand(cond, block, scope);
        let then_block = self.new_block(&[], &mut scope.clone());
        let els_block = self.new_block(&[], &mut scope.clone());
        // 分支按源码顺序处理; 栈后进先出, 故先压 else.
        pending.push(Task {
            cps: els,
            block: els_block,
            scope: scope.clone(),
        });
        pending.push(Task {
            cps: then,
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
    }

    fn finish_match<'ir>(
        &mut self,
        block: BlockId,
        scope: &Scope,
        pending: &mut Vec<Task<'ir>>,
        value: &Value,
        arms: &'ir [MatchArmIr],
    ) {
        let value = self.operand(value, block, scope);
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
    }

    fn finish_continue(&mut self, block: BlockId, scope: &Scope, args: &[Value]) {
        let args = self.operands(args, block, scope);
        self.finish_block(block, Terminator::Continue { args });
    }

    fn finish_exit(&mut self, block: BlockId, scope: &Scope, value: &Value) {
        let value = self.operand(value, block, scope);
        self.finish_block(block, Terminator::Exit { value });
    }

    fn finish_fail(
        &mut self,
        block: BlockId,
        scope: &Scope,
        kind: FailKind,
        message: &Value,
        span: Span,
    ) {
        let message = self.operand(message, block, scope);
        self.finish_block(
            block,
            Terminator::Fail {
                kind,
                message,
                span,
            },
        );
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
            Value::Str(value) => Operand::Str(value.clone()),
            Value::Bool(value) => Operand::Bool(*value),
            Value::Nil => Operand::Nil,
            Value::Builtin(id) => Operand::Builtin(*id),
            Value::Var(name) => Self::var_ref(VarKey::Named(*name), scope),
            Value::Temp(id) => Self::var_ref(VarKey::Temp(*id), scope),
            Value::Func(name) => {
                let captures = self.captures(*name, scope);
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

    fn var_ref(key: VarKey, scope: &Scope) -> Operand {
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

    fn captures(&self, function: Symbol, scope: &Scope) -> Vec<Operand> {
        let names = self.free.get(&function).cloned().unwrap_or_default();
        names
            .iter()
            .map(|name| Self::var_ref(VarKey::Named(*name), scope))
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
                ty: ksl_frontend::check::Type::Nil,
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
                captures: self.captures(*function, scope),
            })
            .collect()
    }

    fn finish(self, function: &IrFunction, free_vars: Vec<Symbol>, entry: BlockId) -> Function {
        let Builder {
            blocks, var_binder, ..
        } = self;
        let mut raw = blocks;
        let terms: Vec<Terminator> = raw
            .iter_mut()
            .map(|block| block.term.take().expect("基本块缺少终止指令"))
            .collect();
        let (extra_by_block, local_maps) = insert_edge_params(&mut raw, &terms, &var_binder);
        let free_index = free_indices(&free_vars);
        let predefined: Vec<usize> = raw.iter().map(|block| block.predefined).collect();
        let blocks = rewrite_blocks(raw, terms, &extra_by_block, &local_maps, &free_index);
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

/// 按活跃变量为每个块插入额外参数, 并返回边参数与变量到槽位的映射.
fn insert_edge_params(
    raw: &mut [RawBlock],
    terms: &[Terminator],
    var_binder: &HashMap<VarId, Binder>,
) -> (Vec<Vec<VarId>>, Vec<HashMap<VarId, u32>>) {
    let uses: Vec<HashSet<VarId>> = raw
        .iter()
        .zip(terms)
        .map(|(block, term)| block_uses(&block.instrs, term))
        .collect();
    let defs: Vec<HashSet<VarId>> = raw
        .iter()
        .map(|block| block.slot_vars.iter().copied().collect())
        .collect();
    let succs = block_successors(terms);
    let live_in = liveness(&uses, &defs, &succs);

    let mut extra_by_block = Vec::with_capacity(raw.len());
    let mut local_maps = Vec::with_capacity(raw.len());
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
                extra.iter().map(|var| var_binder[var].clone()).collect();
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
    (extra_by_block, local_maps)
}

/// 自由变量名到闭包捕获下标的映射.
fn free_indices(free_vars: &[Symbol]) -> HashMap<Symbol, u32> {
    free_vars
        .iter()
        .enumerate()
        .map(|(index, name)| (*name, u32::try_from(index).expect("自由变量数量超出 u32")))
        .collect()
}

/// 解析槽位与变量引用, 补上边参数, 组装最终基本块.
fn rewrite_blocks(
    raw: Vec<RawBlock>,
    terms: Vec<Terminator>,
    extra_by_block: &[Vec<VarId>],
    local_maps: &[HashMap<VarId, u32>],
    free_index: &HashMap<Symbol, u32>,
) -> Vec<Block> {
    let mut blocks = Vec::with_capacity(raw.len());
    for (index, (mut block, mut term)) in raw.into_iter().zip(terms).enumerate() {
        let local = &local_maps[index];
        for instr in &mut block.instrs {
            rewrite_instr(instr, local, free_index);
        }
        rewrite_term(&mut term, local, free_index);
        append_edge_args(&mut term, index, extra_by_block, local_maps);
        blocks.push(Block {
            id: block.id,
            slots: block.slots,
            param_count: block.param_count,
            instrs: block.instrs,
            term,
        });
    }
    blocks
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
                ty: ksl_frontend::check::Type::Nil,
            })
        })
        .collect()
}
