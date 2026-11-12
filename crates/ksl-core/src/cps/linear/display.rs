//! 线性 CPS IR 的文本显示.

use std::fmt;

use super::model::{
    Block, FStrPart, Function, Instr, MatchArm, Module, Operand, RecBinding, Terminator, VarRef,
};
use crate::cps::ir::{Binding, FunctionKind};

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for decl in &self.data {
            writeln!(f, "data {} {{", decl.name)?;
            for variant in &decl.variants {
                if variant.fields.is_empty() {
                    writeln!(f, "  {}[]", variant.name)?;
                } else {
                    let fields: Vec<String> =
                        variant.fields.iter().map(ToString::to_string).collect();
                    writeln!(f, "  {}[{}]", variant.name, fields.join(", "))?;
                }
            }
            writeln!(f, "}}")?;
        }
        if !self.data.is_empty() {
            writeln!(f)?;
        }
        for function in &self.functions {
            writeln!(f, "{function}")?;
            writeln!(f)?;
        }
        write!(f, "{}", self.main)
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut params: Vec<String> = self.params.iter().map(ToString::to_string).collect();
        params.push("$k".to_string());
        let keyword = match self.kind {
            FunctionKind::Fn => "fn",
            FunctionKind::Handler => "handler",
        };
        write!(
            f,
            "{keyword} {}({}) : {}",
            self.name,
            params.join(", "),
            self.ret
        )?;
        if !self.free_vars.is_empty() {
            let free: Vec<String> = self.free_vars.iter().map(ToString::to_string).collect();
            write!(f, " free [{}]", free.join(", "))?;
        }
        writeln!(f, ":")?;
        for block in &self.blocks {
            write_block(f, block, self)?;
        }
        Ok(())
    }
}

fn write_block(f: &mut fmt::Formatter<'_>, block: &Block, function: &Function) -> fmt::Result {
    let params: Vec<String> = block.slots[..block.param_count]
        .iter()
        .map(|binder| binder.target.to_string())
        .collect();
    if params.is_empty() {
        writeln!(f, "  {}:", block.id)?;
    } else {
        writeln!(f, "  {}({}):", block.id, params.join(", "))?;
    }
    for instr in &block.instrs {
        write!(f, "    ")?;
        write_instr(f, instr, block, function)?;
        writeln!(f)?;
    }
    write!(f, "    ")?;
    write_term(f, &block.term, block, function)?;
    writeln!(f)
}

fn write_instr(
    f: &mut fmt::Formatter<'_>,
    instr: &Instr,
    block: &Block,
    function: &Function,
) -> fmt::Result {
    match instr {
        Instr::Let { slot, value } => {
            write!(f, "let {} = ", slot_name(block, *slot))?;
            write_operand(f, value, block, function)
        },
        Instr::GlobalLet {
            slot,
            name,
            ty,
            value,
        } => {
            let target = match (name, slot) {
                (Some(name), Some(slot)) => format!("{name}#{slot}"),
                (Some(name), None) => name.to_string(),
                (None, _) => "_".to_string(),
            };
            write!(f, "global let {target} : {ty} = ")?;
            write_operand(f, value, block, function)
        },
        Instr::LetRec { bindings } => write_rec(f, bindings, block, function, false),
        Instr::GlobalLetRec { bindings } => write_rec(f, bindings, block, function, true),
    }
}

fn write_rec(
    f: &mut fmt::Formatter<'_>,
    bindings: &[RecBinding],
    block: &Block,
    function: &Function,
    global: bool,
) -> fmt::Result {
    let keyword = if global { "global def" } else { "def" };
    let items: Vec<String> = bindings
        .iter()
        .map(|binding| {
            let mut text = format!("{keyword} {} = fn {}", binding.name, binding.function);
            if !binding.captures.is_empty() {
                let captures: Vec<String> = binding
                    .captures
                    .iter()
                    .map(|operand| operand_text(operand, block, function))
                    .collect();
                text.push('[');
                text.push_str(&captures.join(", "));
                text.push(']');
            }
            text
        })
        .collect();
    write!(f, "{}", items.join(", "))
}

fn write_term(
    f: &mut fmt::Formatter<'_>,
    term: &Terminator,
    block: &Block,
    function: &Function,
) -> fmt::Result {
    match term {
        Terminator::Return { value } => {
            write!(f, "return ")?;
            write_operand(f, value, block, function)
        },
        Terminator::Jump { target, args } => {
            write!(f, "jump {target}(")?;
            write_operands(f, args, block, function)?;
            write!(f, ")")
        },
        Terminator::TailCall { callee, args, .. } => {
            write!(f, "tailcall ")?;
            write_operand(f, callee, block, function)?;
            write!(f, "(")?;
            write_operands(f, args, block, function)?;
            write!(f, ")")
        },
        Terminator::Call {
            callee,
            args,
            target,
            live,
            ..
        } => {
            write!(f, "call ")?;
            write_operand(f, callee, block, function)?;
            write!(f, "(")?;
            write_operands(f, args, block, function)?;
            write!(f, ") -> {target}")?;
            write_live(f, live, block, function)
        },
        Terminator::Prim {
            op,
            args,
            ty,
            target,
            live,
            ..
        } => {
            write!(f, "prim @{op}(")?;
            write_operands(f, args, block, function)?;
            write!(f, ") : {ty} -> {target}")?;
            write_live(f, live, block, function)
        },
        Terminator::Receive {
            index,
            timeout,
            target,
            live,
            ..
        } => {
            write!(f, "receive {index}, ")?;
            write_operand(f, timeout, block, function)?;
            write!(f, " -> {target}")?;
            write_live(f, live, block, function)
        },
        Terminator::If { cond, then, els } => {
            write!(f, "if ")?;
            write_operand(f, cond, block, function)?;
            write!(f, " -> ")?;
            write_target(f, then.block, &then.args, block, function)?;
            write!(f, ", ")?;
            write_target(f, els.block, &els.args, block, function)
        },
        Terminator::Match { value, arms } => {
            write!(f, "match ")?;
            write_operand(f, value, block, function)?;
            write!(f, ":")?;
            for arm in arms {
                write!(f, " ")?;
                write_arm(f, arm, block, function)?;
            }
            Ok(())
        },
        Terminator::Continue { args } => {
            write!(f, "continue(")?;
            write_operands(f, args, block, function)?;
            write!(f, ")")
        },
        Terminator::Exit { value } => {
            write!(f, "exit(")?;
            write_operand(f, value, block, function)?;
            write!(f, ")")
        },
        Terminator::Halt => write!(f, "halt"),
        Terminator::Fail { kind, message, .. } => {
            write!(f, "fail {kind}(")?;
            write_operand(f, message, block, function)?;
            write!(f, ")")
        },
    }
}

fn write_arm(
    f: &mut fmt::Formatter<'_>,
    arm: &MatchArm,
    block: &Block,
    function: &Function,
) -> fmt::Result {
    write!(f, "{} -> ", arm.pattern)?;
    write_target(f, arm.target, &arm.live, block, function)
}

fn write_target(
    f: &mut fmt::Formatter<'_>,
    target: super::model::BlockId,
    args: &[Operand],
    block: &Block,
    function: &Function,
) -> fmt::Result {
    write!(f, "{target}(")?;
    write_operands(f, args, block, function)?;
    write!(f, ")")
}

fn write_live(
    f: &mut fmt::Formatter<'_>,
    live: &[Operand],
    block: &Block,
    function: &Function,
) -> fmt::Result {
    if live.is_empty() {
        return Ok(());
    }
    write!(f, " live(")?;
    write_operands(f, live, block, function)?;
    write!(f, ")")
}

fn write_operands(
    f: &mut fmt::Formatter<'_>,
    operands: &[Operand],
    block: &Block,
    function: &Function,
) -> fmt::Result {
    for (index, operand) in operands.iter().enumerate() {
        if index > 0 {
            write!(f, ", ")?;
        }
        write_operand(f, operand, block, function)?;
    }
    Ok(())
}

fn write_operand(
    f: &mut fmt::Formatter<'_>,
    operand: &Operand,
    block: &Block,
    function: &Function,
) -> fmt::Result {
    match operand {
        Operand::I64(value) => write!(f, "{value}"),
        Operand::U8(value) => write!(f, "{value}u"),
        Operand::U64(value) => write!(f, "{value}U"),
        Operand::F64(value) => write!(f, "{value}"),
        Operand::Char(value) => write!(f, "{value:?}"),
        Operand::Str(value) => write!(f, "{value:?}"),
        Operand::Bool(value) => write!(f, "{value}"),
        Operand::Nil => write!(f, "nil"),
        Operand::Builtin(id) => write!(f, "@{}", crate::builtin::name(*id)),
        Operand::Var(VarRef::Local(slot)) => write!(f, "{}", slot_name(block, *slot)),
        Operand::Var(VarRef::Capture(index)) => {
            let name = function
                .free_vars
                .get(*index as usize)
                .map_or_else(|| format!("^{index}"), ToString::to_string);
            write!(f, "^{name}")
        },
        Operand::Var(VarRef::Id(var)) => write!(f, "{var}"),
        Operand::Var(VarRef::Free(name)) => write!(f, "{name}"),
        Operand::Func { name, captures } => {
            write!(f, "fn {name}")?;
            if !captures.is_empty() {
                write!(f, "[")?;
                write_operands(f, captures, block, function)?;
                write!(f, "]")?;
            }
            Ok(())
        },
        Operand::List { items, .. } => {
            write!(f, "{{")?;
            write_operands(f, items, block, function)?;
            write!(f, "}}")
        },
        Operand::Pair { first, second, .. } => {
            write!(f, "(")?;
            write_operand(f, first, block, function)?;
            write!(f, ", ")?;
            write_operand(f, second, block, function)?;
            write!(f, ")")
        },
        Operand::Data {
            ty,
            variant,
            fields,
        } => {
            write!(f, "{ty}/{variant}[")?;
            write_operands(f, fields, block, function)?;
            write!(f, "]")
        },
        Operand::FStr(parts) => {
            write!(f, "f\"")?;
            for part in parts {
                match part {
                    FStrPart::Text(text) => write!(f, "{}", text.escape_debug())?,
                    FStrPart::Interp { value, spec } => {
                        write!(f, "{{")?;
                        write_operand(f, value, block, function)?;
                        write!(f, "}}")?;
                        if let Some(spec) = spec {
                            write!(f, ":{spec}")?;
                        }
                    },
                }
            }
            write!(f, "\"")
        },
    }
}

fn slot_name(block: &Block, slot: u32) -> String {
    match block.slots.get(slot as usize).map(|binder| binder.target) {
        Some(Binding::Named(name)) => format!("{name}#{slot}"),
        Some(Binding::Temp(id)) => format!("{id}#{slot}"),
        Some(Binding::Discard) | None => format!("_#{slot}"),
    }
}

fn operand_text(operand: &Operand, block: &Block, function: &Function) -> String {
    struct Wrapper<'a> {
        operand: &'a Operand,
        block: &'a Block,
        function: &'a Function,
    }
    impl fmt::Display for Wrapper<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write_operand(f, self.operand, self.block, self.function)
        }
    }
    Wrapper {
        operand,
        block,
        function,
    }
    .to_string()
}
