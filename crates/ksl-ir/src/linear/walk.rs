//! 线性执行数据的遍历辅助.

use super::model::{BlockId, FStrPart, GlobalTarget, Instr, Operand, Terminator};

/// 遍历操作数的直接子项; `$iter` 选择只读或可变迭代.
macro_rules! walk_operand_children {
    ($operand:expr, $visit:expr, $iter:ident) => {
        match $operand {
            Operand::Func(func) => {
                for capture in func.captures.$iter() {
                    $visit(capture);
                }
            },
            Operand::List(list) => {
                for item in list.items.$iter() {
                    $visit(item);
                }
            },
            Operand::Pair { first, second } => {
                $visit(first);
                $visit(second);
            },
            Operand::Data(data) => {
                for field in data.fields.$iter() {
                    $visit(field);
                }
            },
            Operand::FStr(parts) => {
                for part in parts.$iter() {
                    if let FStrPart::Interp { value, .. } = part {
                        $visit(value);
                    }
                }
            },
            Operand::I64(_)
            | Operand::I32(_)
            | Operand::U8(_)
            | Operand::U32(_)
            | Operand::U64(_)
            | Operand::F32(_)
            | Operand::F64(_)
            | Operand::Char(_)
            | Operand::Str(_)
            | Operand::Bool(_)
            | Operand::Nil
            | Operand::Builtin(_)
            | Operand::Var(_) => {},
        }
    };
}

/// 遍历指令引用的操作数; `$iter` 选择只读或可变迭代.
macro_rules! walk_instr_operands {
    ($instr:expr, $visit:expr, $iter:ident) => {
        match $instr {
            Instr::Let { value, .. } | Instr::GlobalLet { value, .. } => $visit(value),
            Instr::Builtin { args, .. } | Instr::Prim { args, .. } => {
                for arg in args.$iter() {
                    $visit(arg);
                }
            },
            Instr::LetRec { bindings } | Instr::GlobalLetRec { bindings } => {
                for binding in bindings.$iter() {
                    for capture in binding.captures.$iter() {
                        $visit(capture);
                    }
                }
            },
        }
    };
}

/// 遍历终止指令引用的操作数; `$iter` 选择只读或可变迭代.
macro_rules! walk_term_operands {
    ($term:expr, $visit:expr, $iter:ident) => {
        match $term {
            Terminator::Return { value } | Terminator::Exit { value } => $visit(value),
            Terminator::Fail { message, .. } => $visit(message),
            Terminator::Jump { args, .. } | Terminator::Continue { args } => {
                for arg in args.$iter() {
                    $visit(arg);
                }
            },
            Terminator::TailCall { callee, args, .. } => {
                $visit(callee);
                for arg in args.$iter() {
                    $visit(arg);
                }
            },
            Terminator::Call {
                callee, args, live, ..
            } => {
                $visit(callee);
                for arg in args.$iter().chain(live.$iter()) {
                    $visit(arg);
                }
            },
            Terminator::Prim { args, live, .. } => {
                for arg in args.$iter().chain(live.$iter()) {
                    $visit(arg);
                }
            },
            Terminator::Receive { timeout, live, .. } => {
                $visit(timeout);
                for arg in live.$iter() {
                    $visit(arg);
                }
            },
            Terminator::If { cond, then, els } => {
                $visit(cond);
                for arg in then.args.$iter().chain(els.args.$iter()) {
                    $visit(arg);
                }
            },
            Terminator::Match { value, arms } => {
                $visit(value);
                for arm in arms.$iter() {
                    for arg in arm.live.$iter() {
                        $visit(arg);
                    }
                }
            },
            Terminator::Halt => {},
        }
    };
}

/// 遍历操作数的直接子项.
pub(super) trait OperandWalk {
    fn for_each_child(&self, visit: &mut impl FnMut(&Operand));
    fn for_each_child_mut(&mut self, visit: &mut impl FnMut(&mut Operand));
    fn walk_mut(&mut self, visit: &mut impl FnMut(&mut Operand) -> bool);
}

impl OperandWalk for Operand {
    fn for_each_child(&self, visit: &mut impl FnMut(&Operand)) {
        walk_operand_children!(self, visit, iter);
    }

    fn for_each_child_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        walk_operand_children!(self, visit, iter_mut);
    }

    fn walk_mut(&mut self, visit: &mut impl FnMut(&mut Operand) -> bool) {
        walk_operand_mut(self, visit);
    }
}

fn walk_operand_mut(operand: &mut Operand, visit: &mut impl FnMut(&mut Operand) -> bool) {
    if visit(operand) {
        operand.for_each_child_mut(&mut |child| walk_operand_mut(child, visit));
    }
}

/// 遍历指令引用的操作数与槽位.
pub(super) trait InstrWalk {
    fn for_each_operand(&self, visit: &mut impl FnMut(&Operand));
    fn for_each_operand_mut(&mut self, visit: &mut impl FnMut(&mut Operand));
    fn for_each_slot_mut(&mut self, visit: &mut impl FnMut(&mut u32));
}

impl InstrWalk for Instr {
    fn for_each_operand(&self, visit: &mut impl FnMut(&Operand)) {
        walk_instr_operands!(self, visit, iter);
    }

    fn for_each_operand_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        walk_instr_operands!(self, visit, iter_mut);
    }

    fn for_each_slot_mut(&mut self, visit: &mut impl FnMut(&mut u32)) {
        match self {
            Self::Let { slot, .. } | Self::Builtin { slot, .. } => visit(slot),
            Self::Prim { slot, dead, .. } => {
                visit(slot);
                for slot in dead {
                    visit(slot);
                }
            },
            Self::GlobalLet { target, .. } => {
                if let GlobalTarget::Named { slot, .. } = target {
                    visit(slot);
                }
            },
            Self::LetRec { bindings } | Self::GlobalLetRec { bindings } => {
                for binding in bindings {
                    visit(&mut binding.slot);
                }
            },
        }
    }
}

/// 遍历终止指令引用的操作数与目标块.
pub(super) trait TermWalk {
    fn for_each_operand(&self, visit: &mut impl FnMut(&Operand));
    fn for_each_operand_mut(&mut self, visit: &mut impl FnMut(&mut Operand));
    fn for_each_target(&self, visit: &mut impl FnMut(BlockId));
}

impl TermWalk for Terminator {
    fn for_each_operand(&self, visit: &mut impl FnMut(&Operand)) {
        walk_term_operands!(self, visit, iter);
    }

    fn for_each_operand_mut(&mut self, visit: &mut impl FnMut(&mut Operand)) {
        walk_term_operands!(self, visit, iter_mut);
    }

    fn for_each_target(&self, visit: &mut impl FnMut(BlockId)) {
        match self {
            Self::Jump { target, .. }
            | Self::Call { target, .. }
            | Self::Prim { target, .. }
            | Self::Receive { target, .. } => visit(*target),
            Self::If { then, els, .. } => {
                visit(then.block);
                visit(els.block);
            },
            Self::Match { arms, .. } => {
                for arm in arms {
                    visit(arm.target);
                }
            },
            Self::Return { .. }
            | Self::TailCall { .. }
            | Self::Continue { .. }
            | Self::Exit { .. }
            | Self::Halt
            | Self::Fail { .. } => {},
        }
    }
}
