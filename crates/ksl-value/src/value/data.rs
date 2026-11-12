//! Data 值及内置构造器.

use std::ops::Deref;
use std::slice;
use std::sync::Arc;

use ksl_base::name::{Name, keywords};

use super::{List, Value};

/// Data 字段存储.
///
/// 0 到 2 个字段内联, 更多字段存堆上的切片.
#[derive(Debug, Default)]
pub enum DataFields {
    #[default]
    Zero,
    One([Value; 1]),
    Two([Value; 2]),
    Many(Box<[Value]>),
}

impl DataFields {
    fn drain_into(self, work: &mut DropWork) {
        match self {
            Self::Zero => {},
            Self::One([value]) => work.push(value),
            Self::Two([first, second]) => {
                work.push(first);
                work.push(second);
            },
            Self::Many(values) => work.extend(values),
        }
    }
}

/// 迭代释放用的栈; 少量值内联, 更多值进入溢出栈.
struct DropWork {
    inline: [Option<Value>; 4],
    len: usize,
    spill: Vec<Value>,
}

impl DropWork {
    fn new() -> Self {
        Self {
            inline: [None, None, None, None],
            len: 0,
            spill: Vec::new(),
        }
    }

    fn push(&mut self, value: Value) {
        if self.len < self.inline.len() {
            self.inline[self.len] = Some(value);
            self.len += 1;
        } else {
            self.spill.push(value);
        }
    }

    fn extend(&mut self, values: impl IntoIterator<Item = Value>) {
        for value in values {
            self.push(value);
        }
    }

    fn pop(&mut self) -> Option<Value> {
        if let Some(value) = self.spill.pop() {
            return Some(value);
        }
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        self.inline[self.len].take()
    }
}

impl Deref for DataFields {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Zero => &[],
            Self::One(values) => values,
            Self::Two(values) => values,
            Self::Many(values) => values,
        }
    }
}

impl<'a> IntoIterator for &'a DataFields {
    type IntoIter = slice::Iter<'a, Value>;
    type Item = &'a Value;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl From<[Value; 1]> for DataFields {
    fn from(values: [Value; 1]) -> Self {
        Self::One(values)
    }
}

impl From<[Value; 2]> for DataFields {
    fn from(values: [Value; 2]) -> Self {
        Self::Two(values)
    }
}

impl From<Vec<Value>> for DataFields {
    fn from(values: Vec<Value>) -> Self {
        let mut values = values.into_iter();
        let Some(first) = values.next() else {
            return Self::Zero;
        };
        let Some(second) = values.next() else {
            return Self::One([first]);
        };
        if values.len() == 0 {
            return Self::Two([first, second]);
        }
        let mut rest = Vec::with_capacity(2 + values.len());
        rest.push(first);
        rest.push(second);
        rest.extend(values);
        Self::Many(rest.into_boxed_slice())
    }
}

/// Data 值: 类型名, 变体名与字段.
#[derive(Debug)]
pub struct DataValue {
    pub ty: Name,
    pub variant: Name,
    pub fields: DataFields,
}

impl Drop for DataValue {
    /// 迭代释放字段, 不递归析构 Data 链.
    ///
    /// 只展开直接包含 `Value` 的容器; 标量与 `Set` 不含 `Value`.
    /// 闭包捕获不在此处展开, 由常规释放处理.
    fn drop(&mut self) {
        let mut work = DropWork::new();
        std::mem::take(&mut self.fields).drain_into(&mut work);
        while let Some(value) = work.pop() {
            match value {
                Value::Data(arc) => {
                    if let Ok(mut data) = Arc::try_unwrap(arc) {
                        std::mem::take(&mut data.fields).drain_into(&mut work);
                    }
                },
                Value::Pair(arc) => {
                    if let Ok((first, second)) = Arc::try_unwrap(arc) {
                        work.push(first);
                        work.push(second);
                    }
                },
                Value::List(arc) => {
                    if let Ok(List::Values(items)) = Arc::try_unwrap(arc) {
                        work.extend(items);
                    }
                },
                Value::Dict(arc) => {
                    if let Ok(dict) = Arc::try_unwrap(arc) {
                        work.extend(dict.into_iter().map(|(_, value)| value));
                    }
                },
                // 其余变体不直接包含可迭代的 `Value`; 正常释放.
                Value::I64(_)
                | Value::I32(_)
                | Value::U8(_)
                | Value::U32(_)
                | Value::U64(_)
                | Value::F32(_)
                | Value::F64(_)
                | Value::Char(_)
                | Value::Bool(_)
                | Value::Nil
                | Value::Str(_)
                | Value::Set(_)
                | Value::Closure(_)
                | Value::Builtin(_)
                | Value::Pid(_)
                | Value::ForeignFn(_)
                | Value::NativeOpaque(_) => {},
            }
        }
    }
}

/// 构造内置 Data 值; 字段接受数组或 `Vec`.
#[must_use]
pub fn data_value(ty: Name, variant: Name, fields: impl Into<DataFields>) -> Value {
    Value::Data(Arc::new(DataValue {
        ty,
        variant,
        fields: fields.into(),
    }))
}

/// 构造无载荷的内置 Data 值.
#[must_use]
pub fn data_value0(ty: Name, variant: Name) -> Value {
    data_value(ty, variant, DataFields::Zero)
}

#[must_use]
pub fn option_some(value: Value) -> Value {
    let names = keywords();
    data_value(names.option_.clone(), names.some_.clone(), [value])
}

#[must_use]
pub fn option_none() -> Value {
    let names = keywords();
    data_value0(names.option_.clone(), names.none_.clone())
}

#[must_use]
pub fn result_ok(value: Value) -> Value {
    let names = keywords();
    data_value(names.result_.clone(), names.ok_.clone(), [value])
}

#[must_use]
pub fn result_err(error: Value) -> Value {
    let names = keywords();
    data_value(names.result_.clone(), names.err_.clone(), [error])
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ksl_base::name::Name;

    use super::*;

    #[test]
    fn deep_data_drop_is_iterative() {
        let ty = Name::new("Node");
        let leaf = Name::new("Leaf");
        let branch = Name::new("Branch");
        let mut value = data_value0(ty.clone(), leaf.clone());
        for _ in 0..100_000 {
            value = data_value(ty.clone(), branch.clone(), [value]);
        }
        drop(value);
    }

    #[test]
    fn deep_data_through_list_is_iterative() {
        let ty = Name::new("Node");
        let leaf = Name::new("Leaf");
        let branch = Name::new("Branch");
        let mut value = data_value0(ty.clone(), leaf.clone());
        for _ in 0..50_000 {
            let nested = Value::List(Arc::new(List::from_values([value])));
            value = data_value(ty.clone(), branch.clone(), [nested]);
        }
        drop(value);
    }
}
