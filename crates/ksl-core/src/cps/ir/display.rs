//! CPS IR 的文本显示.

use std::fmt;

use super::model::{
    Cont, Cps, FStrPartIr, Function, FunctionKind, MatchArmIr, Module, PatternIr, PrimOp, Value,
};
use crate::builtin;

impl fmt::Display for PrimOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List(op) => f.write_str(op.name()),
            Self::Pair(op) => f.write_str(op.name()),
            Self::Option(op) => f.write_str(op.name()),
            Self::Dict(op) => f.write_str(op.name()),
            Self::Set(op) => f.write_str(op.name()),
            Self::DictLiteral => f.write_str("DictLiteral"),
            Self::SetLiteral => f.write_str("SetLiteral"),
            Self::Registry(op) => f.write_str(op.name()),
            Self::Spawn { detached: false } => f.write_str("Spawn"),
            Self::Spawn { detached: true } => f.write_str("SpawnDetached"),
            Self::Send => f.write_str("Send"),
            Self::Join => f.write_str("Join"),
            Self::Kill => f.write_str("Kill"),
            Self::Monitor => f.write_str("Monitor"),
            Self::Unmonitor => f.write_str("Unmonitor"),
            Self::PidSerial => f.write_str("PidSerial"),
            Self::SetTimer => f.write_str("SetTimer"),
            Self::CancelTimer => f.write_str("CancelTimer"),
            Self::Sleep => f.write_str("Sleep"),
        }
    }
}

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
        params.push(self.cont.to_string());
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

impl Cps {
    #[allow(clippy::too_many_lines)]
    fn fmt_indented(&self, f: &mut fmt::Formatter<'_>, indent: usize) -> fmt::Result {
        // 显式栈按先序输出, 避免格式化长语句序列时按 CPS 深度递归.
        enum Entry<'a> {
            Node(&'a Cps, usize),
            Text(&'a str, usize),
            MatchArm(&'a MatchArmIr, usize),
        }

        let mut pending = vec![Entry::Node(self, indent)];
        while let Some(entry) = pending.pop() {
            match entry {
                Entry::Node(cps, indent) => {
                    let pad = "  ".repeat(indent);
                    match cps {
                        Self::Jump { cont, value } => {
                            writeln!(f, "{pad}jump {cont}({value})")?;
                        },
                        Self::TailCall { callee, args, .. } => {
                            writeln!(f, "{pad}tailcall {callee}({})", join_values(args))?;
                        },
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
                            writeln!(
                                f,
                                "{pad}receive {index}, {timeout} then {}:",
                                cont.header()
                            )?;
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
                        Self::Continue { args } => {
                            writeln!(f, "{pad}continue({})", join_values(args))?;
                        },
                        Self::Exit(value) => {
                            writeln!(f, "{pad}exit({value})")?;
                        },
                        Self::Halt => {
                            writeln!(f, "{pad}halt")?;
                        },
                        Self::Fail { kind, message, .. } => {
                            writeln!(f, "{pad}fail {kind}({message})")?;
                        },
                    }
                },
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
            Self::I64(value) => write!(f, "{value}"),
            Self::U8(value) => write!(f, "{value}u"),
            Self::U64(value) => write!(f, "{value}U"),
            Self::F64(value) => write!(f, "{value:?}"),
            Self::Char(value) => write!(f, "{value:?}"),
            Self::Str(value) => write!(f, "{value:?}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Nil => write!(f, "nil"),
            Self::Var(name) => write!(f, "{name}"),
            Self::Temp(id) => write!(f, "{id}"),
            Self::Func(name) => write!(f, "fn {name}"),
            Self::Builtin(id) => write!(f, "{}", builtin::name(*id)),
            Self::List { elem, items } => {
                write!(f, "List[{elem}]{{{}}}", join_values(items))
            },
            Self::Pair { first, second, .. } => write!(f, "({first}, {second})"),
            Self::Data {
                ty,
                variant,
                fields,
            } => write!(f, "{ty}/{variant}[{}]", join_values(fields)),
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

impl fmt::Display for PatternIr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wildcard => write!(f, "_"),
            Self::Variant {
                qualifier,
                name,
                fields,
            } => {
                if let Some(qualifier) = qualifier {
                    write!(f, "{qualifier}/")?;
                }
                write!(f, "{name}[")?;
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        write!(f, ", ")?;
                    }
                    match field {
                        Some(binder) => write!(f, "{binder}")?,
                        None => write!(f, "_")?,
                    }
                }
                write!(f, "]")
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
