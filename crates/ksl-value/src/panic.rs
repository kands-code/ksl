//! panic 载荷的文本化.

use std::any::Any;

/// 取 panic 载荷的说明文本.
#[must_use]
pub fn message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "未知原因".to_string()
    }
}
