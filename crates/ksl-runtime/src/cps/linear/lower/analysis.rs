//! 线性 IR 的数据流分析, 槽位重写与边参数校验.

use std::collections::{HashMap, HashSet};

use ksl_base::symbol::Symbol;

use crate::cps::linear::model::{
    Block, BlockId, Instr, Operand, Target, Terminator, VarId, VarRef,
};

pub(super) fn validate_edges(blocks: &[Block], predefined: &[usize]) {
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

pub(super) fn shift_slots(instrs: &mut [Instr], added: u32) {
    for instr in instrs {
        instr.for_each_slot_mut(&mut |slot, _| *slot += added);
    }
}

pub(super) fn block_uses(instrs: &[Instr], term: &Terminator) -> HashSet<VarId> {
    let mut uses = HashSet::new();
    for instr in instrs {
        instr.for_each_operand(&mut |operand| operand_uses(operand, &mut uses));
    }
    term.for_each_operand(&mut |operand| operand_uses(operand, &mut uses));
    uses
}

fn operand_uses(operand: &Operand, uses: &mut HashSet<VarId>) {
    if let Operand::Var(VarRef::Id(var)) = operand {
        uses.insert(*var);
    }
    operand.for_each_child(&mut |child| operand_uses(child, uses));
}

pub(super) fn block_successors(terms: &[Terminator]) -> Vec<Vec<usize>> {
    terms
        .iter()
        .map(|term| {
            let mut succ = Vec::new();
            term.for_each_target(&mut |target: BlockId| succ.push(target.index() as usize));
            succ
        })
        .collect()
}

pub(super) fn liveness(
    uses: &[HashSet<VarId>],
    defs: &[HashSet<VarId>],
    succs: &[Vec<usize>],
) -> Vec<HashSet<VarId>> {
    let mut live_in: Vec<HashSet<VarId>> = vec![HashSet::new(); uses.len()];
    loop {
        let mut changed = false;
        for index in (0..uses.len()).rev() {
            // 每个变量在函数内只有一处定义, 因此块内定义直接从活跃集中扣除.
            // 结果只含块内定义之前就被读取的变量.
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

pub(super) fn rewrite_instr(
    instr: &mut Instr,
    local: &HashMap<VarId, u32>,
    free: &HashMap<Symbol, u32>,
) {
    instr.for_each_operand_mut(&mut |operand| rewrite_operand(operand, local, free));
}

pub(super) fn rewrite_term(
    term: &mut Terminator,
    local: &HashMap<VarId, u32>,
    free: &HashMap<Symbol, u32>,
) {
    term.for_each_operand_mut(&mut |operand| rewrite_operand(operand, local, free));
}

fn rewrite_operand(
    operand: &mut Operand,
    local: &HashMap<VarId, u32>,
    free: &HashMap<Symbol, u32>,
) {
    operand.walk_mut(&mut |operand| match operand {
        Operand::Var(VarRef::Id(var)) => {
            let slot = local.get(var).unwrap_or_else(|| panic!("变量 {var} 没有槽位"));
            *operand = Operand::Var(VarRef::Local(*slot));
            false
        },
        Operand::Var(VarRef::Free(name)) => {
            let index = free
                .get(name)
                .unwrap_or_else(|| panic!("名字 {name} 不在自由变量表中"));
            *operand = Operand::Var(VarRef::Capture(*index));
            false
        },
        _ => true,
    });
}

pub(super) fn append_edge_args(
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
