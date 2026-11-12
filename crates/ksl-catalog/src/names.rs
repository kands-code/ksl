//! 编辑器与文档使用的名字分组.
//!
//! 分组只影响高亮与展示, 不参与解析与检查.
//! 核心, Actor 与命名注册归特殊形式; 容器构造器与容器操作归容器操作.
//! 解析与检查仍按形式表与操作表处理.

use crate::forms::special_form_names;
use crate::ops::{REGISTRY_OPS, operation_names};

/// 高亮展示分组.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightGroup {
    /// 核心, Actor 与命名注册特殊形式.
    SpecialForm,
    /// 容器构造器与容器操作.
    ContainerOperation,
}

/// 按文档分组查找名字; 非特殊形式或操作返回 `None`.
#[must_use]
pub fn highlight_group(name: &str) -> Option<HighlightGroup> {
    // 容器构造器在形式表中, 但文档归入容器.
    if matches!(name, "Dict" | "Set") {
        return Some(HighlightGroup::ContainerOperation);
    }
    // 命名注册在操作表中, 但文档归入特殊形式.
    if REGISTRY_OPS.iter().any(|spec| spec.name == name) {
        return Some(HighlightGroup::SpecialForm);
    }
    if special_form_names().any(|form| form == name) {
        return Some(HighlightGroup::SpecialForm);
    }
    if operation_names().any(|operation| operation == name) {
        return Some(HighlightGroup::ContainerOperation);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{HighlightGroup, highlight_group};

    /// 分组与文档章节一致.
    #[test]
    fn groups_follow_docs() {
        for name in ["Let", "Match", "Send", "Register", "WhereIs", "Unregister"] {
            assert_eq!(
                highlight_group(name),
                Some(HighlightGroup::SpecialForm),
                "{name}"
            );
        }
        for name in ["Dict", "Set", "ListMap", "DictGet", "IsSome"] {
            assert_eq!(
                highlight_group(name),
                Some(HighlightGroup::ContainerOperation),
                "{name}"
            );
        }
        for name in ["StringJoin", "i64", "foo"] {
            assert_eq!(highlight_group(name), None, "{name}");
        }
    }
}
