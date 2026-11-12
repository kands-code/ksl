//! 语言表面名单导出.
//!
//! 供 tree-sitter 生成脚本与编辑器工具读取, 不解析 Rust 源文件.
//!
//! 用法: `dump <builtins|types|primitive-types|type-constructors|named-types|`
//! `special-forms|operations|op-families|reserved>`.

use std::process::ExitCode;

use ksl_catalog::{builtin, forms, ops, reserved};

fn main() -> ExitCode {
    let Some(kind) = std::env::args().nth(1) else {
        eprintln!(concat!(
            "用法: dump <builtins|types|primitive-types|type-constructors|named-types|",
            "special-forms|operations|op-families|reserved>"
        ));
        return ExitCode::FAILURE;
    };
    match kind.as_str() {
        "builtins" => {
            for spec in builtin::specs() {
                println!("{}", spec.name);
            }
        },
        "types" => {
            for name in &*reserved::BUILTIN_TYPE_NAMES {
                println!("{name}");
            }
        },
        "primitive-types" => {
            for name in reserved::PRIMITIVE_TYPE_NAMES {
                println!("{name}");
            }
        },
        "type-constructors" => {
            for name in reserved::TYPE_CONSTRUCTOR_NAMES {
                println!("{name}");
            }
            // Fn 是特殊形式, 同时构造函数类型.
            println!("Fn");
        },
        "named-types" => {
            for name in &*reserved::BUILTIN_DATA_TYPE_NAMES {
                println!("{name}");
            }
            for name in reserved::BUILTIN_OPAQUE_TYPE_NAMES {
                println!("{name}");
            }
        },
        "special-forms" => {
            for name in forms::special_form_names() {
                println!("{name}");
            }
        },
        "operations" => {
            for name in ops::operation_names() {
                println!("{name}");
            }
        },
        "op-families" => {
            for spec in ops::LIST_OPS {
                println!("LIST_OPS {}", spec.name);
            }
            for spec in ops::PAIR_OPS {
                println!("PAIR_OPS {}", spec.name);
            }
            for spec in ops::DICT_OPS {
                println!("DICT_OPS {}", spec.name);
            }
            for spec in ops::SET_OPS {
                println!("SET_OPS {}", spec.name);
            }
            for spec in ops::OPTION_OPS {
                println!("OPTION_OPS {}", spec.name);
            }
        },
        "reserved" => {
            for name in reserved::names() {
                println!("{name}");
            }
        },
        other => {
            eprintln!("未知名单: {other}");
            return ExitCode::FAILURE;
        },
    }
    ExitCode::SUCCESS
}
