//! CPS IR 的文本显示.

use std::fmt;

use ksl_catalog::builtin;

use super::model::{
    Cont, Cps, EvalKind, FStrPartIr, Function, FunctionKind, MatchArmIr, Module, Value,
};

impl fmt::Display for Module {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        crate::display::write_data(f, &self.data)?;
        for function in &self.functions {
            writeln!(f, "{function}")?;
            writeln!(f)?;
        }
        if let Some(entry) = &self.entry {
            writeln!(f, "{entry}")?;
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
        writeln!(
            f,
            "{keyword} {}({}) : {}:",
            self.name,
            params.join(", "),
            self.ret
        )?;
        self.body.fmt_indented(f, 1)
    }
}

/// 显式栈条目; 按先序输出.
enum Entry<'a> {
    Node(&'a Cps, usize),
    Text(&'a str, usize),
    MatchArm(&'a MatchArmIr, usize),
}

impl Cps {
    fn fmt_indented(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        let mut pending = vec![Entry::Node(self, indent)];
        while let Some(entry) = pending.pop() {
            match entry {
                Entry::Node(cps, indent) => Self::fmt_node(cps, f, indent, &mut pending)?,
                Entry::Text(text, indent) => {
                    writeln!(f, "{}{text}", "  ".repeat(indent))?;
                },
                Entry::MatchArm(arm, indent) => {
                    writeln!(f, "{}  {} ->", "  ".repeat(indent), arm.pattern)?;
                },
            }
        }
        Ok(())
    }

    fn fmt_node<'a>(
        cps: &'a Cps,
        f: &mut fmt::Formatter<'_>,
        indent: usize,
        pending: &mut Vec<Entry<'a>>,
    ) -> fmt::Result {
        match cps {
            Self::Jump { .. }
            | Self::TailCall { .. }
            | Self::Continue { .. }
            | Self::Exit(_)
            | Self::Halt
            | Self::Fail { .. } => Self::fmt_simple(cps, f, indent),
            Self::Eval { .. } => Self::fmt_eval(cps, f, indent, pending),
            _ => Self::fmt_control(cps, f, indent, pending),
        }
    }

    fn fmt_simple(cps: &Cps, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        let pad = "  ".repeat(indent);
        match cps {
            Self::Jump { cont, value } => writeln!(f, "{pad}jump {cont}({value})"),
            Self::TailCall { callee, args, .. } => {
                writeln!(f, "{pad}tailcall {callee}({})", join_values(args))
            },
            Self::Continue { args } => writeln!(f, "{pad}continue({})", join_values(args)),
            Self::Exit(value) => writeln!(f, "{pad}exit({value})"),
            Self::Halt => writeln!(f, "{pad}halt"),
            Self::Fail { kind, message, .. } => {
                writeln!(f, "{pad}fail {kind}({message})")
            },
            _ => unreachable!("非简单节点"),
        }
    }

    fn fmt_eval<'a>(
        cps: &'a Cps,
        f: &mut fmt::Formatter<'_>,
        indent: usize,
        pending: &mut Vec<Entry<'a>>,
    ) -> fmt::Result {
        let pad = "  ".repeat(indent);
        let Self::Eval {
            op,
            args,
            target,
            body,
            ..
        } = cps
        else {
            unreachable!("非同步求值节点");
        };
        match op {
            EvalKind::Builtin(id) => {
                writeln!(
                    f,
                    "{pad}let {target} = @{}({})",
                    builtin::name(*id),
                    join_values(args)
                )?;
            },
            EvalKind::Prim { op, ty } => {
                writeln!(
                    f,
                    "{pad}let {target} = prim @{op}({}) : {ty}",
                    join_values(args)
                )?;
            },
        }
        // Eval 的正文与节点同缩进, 直接放入待处理栈.
        pending.push(Entry::Node(body, indent));
        Ok(())
    }

    fn fmt_control<'a>(
        cps: &'a Cps,
        f: &mut fmt::Formatter<'_>,
        indent: usize,
        pending: &mut Vec<Entry<'a>>,
    ) -> fmt::Result {
        let pad = "  ".repeat(indent);
        match cps {
            Self::Call {
                callee, args, cont, ..
            } => {
                writeln!(
                    f,
                    "{pad}call {callee}({}) then {}:",
                    join_values(args),
                    cont.header()
                )?;
                pending.push(Entry::Node(&cont.body, indent + 1));
            },
            Self::Prim {
                op, args, ty, cont, ..
            } => {
                writeln!(
                    f,
                    "{pad}prim @{op}({}) : {ty} then {}:",
                    join_values(args),
                    cont.header()
                )?;
                pending.push(Entry::Node(&cont.body, indent + 1));
            },
            Self::Receive {
                index,
                timeout,
                cont,
                ..
            } => {
                writeln!(f, "{pad}receive {index}, {timeout} then {}:", cont.header())?;
                pending.push(Entry::Node(&cont.body, indent + 1));
            },
            Self::Let {
                target,
                value,
                body,
            } => {
                writeln!(f, "{pad}let {target} = {value}")?;
                pending.push(Entry::Node(body, indent));
            },
            Self::GlobalLet {
                name,
                ty,
                value,
                body,
            } => {
                match name {
                    Some(name) => {
                        writeln!(f, "{pad}global let {name} : {ty} = {value}")?;
                    },
                    None => {
                        writeln!(f, "{pad}global let _ : {ty} = {value}")?;
                    },
                }
                pending.push(Entry::Node(body, indent));
            },
            Self::LetRec { bindings, body } => {
                for (name, function) in bindings {
                    writeln!(f, "{pad}def {name} = fn {function}")?;
                }
                pending.push(Entry::Node(body, indent));
            },
            Self::GlobalLetRec { bindings, body } => {
                for (name, function) in bindings {
                    writeln!(f, "{pad}global def {name} = fn {function}")?;
                }
                pending.push(Entry::Node(body, indent));
            },
            Self::LetCont { cont, body } => {
                writeln!(f, "{pad}letcont {}:", cont.header())?;
                // 先输出续延体, 再输出 `in:` 与正文.
                pending.push(Entry::Node(body, indent + 1));
                pending.push(Entry::Text("in:", indent));
                pending.push(Entry::Node(&cont.body, indent + 1));
            },
            Self::If { cond, then, els } => {
                writeln!(f, "{pad}if {cond}:")?;
                pending.push(Entry::Node(els, indent + 1));
                pending.push(Entry::Text("else:", indent));
                pending.push(Entry::Node(then, indent + 1));
            },
            Self::Match { value, arms } => {
                writeln!(f, "{pad}match {value}:")?;
                for arm in arms.iter().rev() {
                    pending.push(Entry::Node(&arm.body, indent + 2));
                    pending.push(Entry::MatchArm(arm, indent));
                }
            },
            _ => unreachable!("非控制节点"),
        }
        Ok(())
    }
}

impl Cont {
    fn header(&self) -> String {
        let params: Vec<String> = self.params.iter().map(ToString::to_string).collect();
        format!("{}({})", self.id, params.join(", "))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::I64(value) => write!(f, "{value}i64"),
            Self::I32(value) => write!(f, "{value}i32"),
            Self::U8(value) => write!(f, "{value}u8"),
            Self::U32(value) => write!(f, "{value}u32"),
            Self::U64(value) => write!(f, "{value}u64"),
            Self::F32(value) => write!(f, "{value:?}f32"),
            Self::F64(value) => write!(f, "{value:?}f64"),
            Self::Char(value) => write!(f, "{value:?}"),
            Self::Str(value) => write!(f, "{value:?}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Nil => write!(f, "nil"),
            Self::Var(name) => write!(f, "{name}"),
            Self::Temp(id) => write!(f, "{id}"),
            Self::Func(name) => write!(f, "fn {name}"),
            Self::Builtin(id) => write!(f, "{}", builtin::name(*id)),
            Self::List(list) => {
                write!(f, "List[{}]{{{}}}", list.elem, join_values(&list.items))
            },
            Self::Pair { first, second } => write!(f, "({first}, {second})"),
            Self::Data(data) => {
                write!(f, "{}/{}[{}]", data.ty, data.variant, join_values(&data.fields))
            },
            Self::FStr(parts) => {
                write!(f, "f\"")?;
                for part in parts {
                    match part {
                        FStrPartIr::Text(text) => write!(f, "{}", escape_fstr(text))?,
                        FStrPartIr::Interp { value, spec } => {
                            if let Some(spec) = spec {
                                write!(f, "{{{value}:{spec}}}")?;
                            } else {
                                write!(f, "{{{value}}}")?;
                            }
                        },
                    }
                }
                write!(f, "\"")
            },
        }
    }
}

fn join_values(values: &[Value]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn escape_fstr(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('{', "{{")
        .replace('}', "}}")
}
