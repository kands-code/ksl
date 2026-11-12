//! 线性 IR 的槽位生命周期标记.
//!
//! 同步原语执行前清空其上不再读取的槽位, 使容器实参保持唯一.

use super::model::{Block, Instr, Operand};
use super::walk::{InstrWalk, OperandWalk, TermWalk};

/// 标记每条同步原语指令执行后可释放的槽位.
pub(super) fn annotate_dead_slots(blocks: &mut [Block]) {
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
    if let Operand::Var(super::model::VarRef::Local(slot)) = operand {
        visit(*slot);
    }
    operand.for_each_child(&mut |child| visit_operand(child, visit));
}
