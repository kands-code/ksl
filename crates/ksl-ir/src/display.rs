//! 两套 IR 共用的文本显示片段.

use std::fmt;

use crate::cps::DataDecl;

/// 输出 Data 声明; 非空时追加一个空行.
pub(crate) fn write_data(f: &mut fmt::Formatter<'_>, data: &[DataDecl]) -> fmt::Result {
    for decl in data {
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
    if !data.is_empty() {
        writeln!(f)?;
    }
    Ok(())
}
