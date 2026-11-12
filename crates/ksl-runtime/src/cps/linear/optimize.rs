//! 线性 IR 优化: 融合纯内置调用与单前驱续延块.
//!
//! 调用纯内置函数时, 结果续延块通常只有一个前驱. 把调用改写为指令并拼接
//! 续延块, 可以省去一次基本块切换与参数搬运; 槽位按被拼接块的偏移重编号.

use super::model::{Block, BlockId, Function, Instr, Operand, SlotRole, Terminator, VarRef};
use crate::builtin;

/// 可融合的终止指令种类.
#[derive(Clone, Copy)]
enum Fused {
    Builtin,
    Prim,
}

/// 反复融合纯内置调用与同步原语, 直到没有可融合的续延块.
pub(super) fn fuse_pure_calls(function: &mut Function) {
    let entry = function.entry;
    let mut blocks: Vec<Option<Block>> = std::mem::take(&mut function.blocks)
        .into_iter()
        .map(Some)
        .collect();
    while let Some((caller, kind)) = find_candidate(&blocks, entry) {
        fuse(&mut blocks, caller, kind);
    }
    compact(function, blocks);
}

/// 找一个可融合的终止指令: 被调为纯内置函数或同步原语,
/// 续延块只有该指令一个前驱.
fn find_candidate(blocks: &[Option<Block>], entry: BlockId) -> Option<(usize, Fused)> {
    let preds = predecessors(blocks);
    for (index, block) in blocks.iter().enumerate() {
        let Some(block) = block else {
            continue;
        };
        let (target, live, kind) = match &block.term {
            Terminator::Call {
                callee: Operand::Builtin(id),
                target,
                live,
                ..
            } if is_pure(*id) => (*target, live, Fused::Builtin),
            Terminator::Prim {
                op, target, live, ..
            } if op.is_synchronous() => (*target, live, Fused::Prim),
            _ => continue,
        };
        let target_index = target.index() as usize;
        if target_index == index || target == entry {
            continue;
        }
        let Some(target_block) = blocks.get(target_index).and_then(Option::as_ref) else {
            continue;
        };
        if target_block.param_count != live.len() + 1 {
            continue;
        }
        if preds[target_index].len() != 1 || preds[target_index][0] != index {
            continue;
        }
        return Some((index, kind));
    }
    None
}

fn is_pure(id: builtin::BuiltinId) -> bool {
    builtin::is_pure(id)
}

/// 把终止指令改写为块内指令, 并把续延块的指令与终止指令拼接到调用者.
fn fuse(blocks: &mut [Option<Block>], caller: usize, kind: Fused) {
    let target = match &blocks[caller].as_ref().expect("调用者块不存在").term {
        Terminator::Call { target, .. } | Terminator::Prim { target, .. } => *target,
        _ => unreachable!("候选块不是可融合指令"),
    };
    let mut target_block = blocks[target.index() as usize].take().expect("续延块已被融合");
    let caller_block = blocks[caller].as_mut().expect("调用者块不存在");
    let term = std::mem::replace(&mut caller_block.term, Terminator::Halt);
    let offset = u32::try_from(caller_block.slots.len()).expect("槽位数量超出 u32");
    let param_count = target_block.param_count;
    let (instr, live) = match (kind, term) {
        (
            Fused::Builtin,
            Terminator::Call {
                callee,
                args,
                live,
                span,
                ..
            },
        ) => {
            let Operand::Builtin(id) = callee else {
                unreachable!("候选调用的被调不是内置函数");
            };
            (
                Instr::Builtin {
                    slot: offset,
                    callee: id,
                    args,
                    span,
                },
                live,
            )
        },
        (
            Fused::Prim,
            Terminator::Prim {
                op,
                args,
                arg_types,
                ty,
                live,
                span,
                ..
            },
        ) => (
            Instr::Prim {
                slot: offset,
                op,
                args,
                arg_types,
                ty,
                span,
                dead: Vec::new(),
            },
            live,
        ),
        _ => unreachable!("候选终止指令与融合种类不符"),
    };
    caller_block.instrs.push(instr);
    // 边上的 live 实参都是局部变量, 直接替换进拼接块, 无需参数拷贝.
    let mut subst: Vec<Option<Operand>> = vec![None; target_block.slots.len()];
    subst[0] = Some(Operand::Var(VarRef::Local(offset)));
    for (index, value) in live.into_iter().enumerate() {
        subst[index + 1] = Some(value);
    }
    for instr in &mut target_block.instrs {
        rewrite_instr(instr, &subst, offset, param_count);
    }
    rewrite_term(&mut target_block.term, &subst, offset);
    caller_block.instrs.append(&mut target_block.instrs);
    caller_block.slots.extend(target_block.slots);
    caller_block.term = target_block.term;
}

/// 删除被融合的块, 重新编号并改写所有块引用.
fn compact(function: &mut Function, blocks: Vec<Option<Block>>) {
    let mut remap: Vec<Option<BlockId>> = vec![None; blocks.len()];
    let mut kept = Vec::new();
    for (old, block) in blocks.into_iter().enumerate() {
        let Some(mut block) = block else {
            continue;
        };
        let new = BlockId::from_index(u32::try_from(kept.len()).expect("基本块数量超出 u32"));
        remap[old] = Some(new);
        block.id = new;
        kept.push(block);
    }
    annotate_dead_slots(&mut kept);
    for block in &mut kept {
        remap_term(&mut block.term, &remap);
    }
    let entry = remap[function.entry.index() as usize].expect("入口块不应被融合");
    function.blocks = kept;
    function.entry = entry;
}

/// 标记每条同步原语指令执行后可释放的槽位.
///
/// 原语实参已求值到独立缓冲, 之后不再读取的槽位可以先清空;
/// 容器实参因此在原地修改时保持唯一引用.
fn annotate_dead_slots(blocks: &mut [Block]) {
    for block in blocks {
        let mut last_use = vec![None; block.slots.len()];
        for (index, instr) in block.instrs.iter().enumerate() {
            instr.for_each_operand(&mut |operand| {
                visit_operand(operand, &mut |slot| record_use(&mut last_use, slot, index));
            });
        }
        let term_index = block.instrs.len();
        block.term.for_each_operand(&mut |operand| {
            visit_operand(operand, &mut |slot| record_use(&mut last_use, slot, term_index));
        });
        let prims: Vec<usize> = block
            .instrs
            .iter()
            .enumerate()
            .filter_map(|(index, instr)| matches!(instr, Instr::Prim { .. }).then_some(index))
            .collect();
        // 每个槽位只在最后使用之后的第一个原语处清理一次.
        let mut dead_at = vec![Vec::new(); block.instrs.len()];
        for (slot, used) in last_use.iter().enumerate() {
            let Some(last) = *used else {
                continue;
            };
            if let Some(&index) = prims.get(prims.partition_point(|prim| *prim < last)) {
                dead_at[index].push(u32::try_from(slot).expect("槽位数量超出 u32"));
            }
        }
        for (instr, dead) in block.instrs.iter_mut().zip(dead_at) {
            if let Instr::Prim { dead: slots, .. } = instr {
                *slots = dead;
            }
        }
    }
}

fn record_use(last_use: &mut [Option<usize>], slot: u32, index: usize) {
    if let Some(entry) = last_use.get_mut(slot as usize) {
        *entry = Some(index);
    }
}

fn visit_operand(operand: &Operand, visit: &mut impl FnMut(u32)) {
    if let Operand::Var(VarRef::Local(slot)) = operand {
        visit(*slot);
    }
    operand.for_each_child(&mut |child| visit_operand(child, visit));
}

fn predecessors(blocks: &[Option<Block>]) -> Vec<Vec<usize>> {
    let mut preds = vec![Vec::new(); blocks.len()];
    for (index, block) in blocks.iter().enumerate() {
        let Some(block) = block else {
            continue;
        };
        block
            .term
            .for_each_target(&mut |target: BlockId| preds[target.index() as usize].push(index));
    }
    preds
}

/// 改写拼接块中的指令: 参数槽位按替换表处理, 其余槽位整体后移.
fn rewrite_instr(
    instr: &mut Instr,
    subst: &[Option<Operand>],
    offset: u32,
    param_count: usize,
) {
    instr.for_each_slot_mut(&mut |slot, role| {
        if role == SlotRole::Assigned {
            debug_assert!(*slot as usize >= param_count, "参数槽位不应被重新赋值");
        }
        *slot += offset;
    });
    instr.for_each_operand_mut(&mut |operand| rewrite_operand(operand, subst, offset));
}

fn rewrite_term(term: &mut Terminator, subst: &[Option<Operand>], offset: u32) {
    term.for_each_operand_mut(&mut |operand| rewrite_operand(operand, subst, offset));
}

fn rewrite_operand(operand: &mut Operand, subst: &[Option<Operand>], offset: u32) {
    operand.walk_mut(&mut |operand| {
        if let Operand::Var(VarRef::Local(slot)) = operand {
            if let Some(Some(replacement)) = subst.get(*slot as usize) {
                *operand = replacement.clone();
                return false;
            }
            *slot += offset;
        }
        true
    });
}

fn remap_term(term: &mut Terminator, remap: &[Option<BlockId>]) {
    term.for_each_target_mut(&mut |target: &mut BlockId| {
        *target = remap[target.index() as usize].expect("目标块已被融合");
    });
}
