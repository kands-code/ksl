//! 编辑器与文档使用的名字分组.
//!
//! 分组只影响高亮与展示, 不参与解析与检查.
//! 核心与 Actor 归特殊形式; 容器构造器与容器操作归容器操作.
//! 解析与检查仍按形式表与操作表处理.

use crate::forms::FormKind;
use crate::ops::operation_names;

/// 高亮展示分组.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightGroup {
    /// 核心, Actor 特殊形式.
    SpecialForm,
    /// 容器构造器与容器操作.
    ContainerOperation,
}

/// 按文档分组查找名字; 非特殊形式或操作返回 `None`.
#[must_use]
pub fn highlight_group(name: &str) -> Option<HighlightGroup> {
    if let Some(kind) = FormKind::from_name(name) {
        // 容器构造器在形式表中, 但文档归入容器.
        return Some(match kind {
            FormKind::Dict | FormKind::Set => HighlightGroup::ContainerOperation,
            _ => HighlightGroup::SpecialForm,
        });
    }
    if operation_names().any(|operation| operation == name) {
        return Some(HighlightGroup::ContainerOperation);
    }
    None
}
