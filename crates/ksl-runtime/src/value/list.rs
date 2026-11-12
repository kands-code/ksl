//! 列表存储与操作.

use super::Value;

/// 列表元素与 `Value` 的双向映射.
trait ListElem: Clone {
    fn to_value(&self) -> Value;

    fn from_value(value: Value) -> Result<Self, Value>;
}

impl ListElem for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }

    fn from_value(value: Value) -> Result<Self, Value> {
        Ok(value)
    }
}

macro_rules! scalar_list_elem {
    ($($ty:ty => $variant:ident),+ $(,)?) => {
        $(impl ListElem for $ty {
            fn to_value(&self) -> Value {
                Value::$variant(*self)
            }

            fn from_value(value: Value) -> Result<Self, Value> {
                match value {
                    Value::$variant(value) => Ok(value),
                    other => Err(other),
                }
            }
        })+
    };
}

scalar_list_elem! {
    i64 => I64,
    u8 => U8,
    u64 => U64,
    f64 => F64,
    char => Char,
    bool => Bool,
}

/// 声明列表变体与构造器.
macro_rules! list_variants {
    ($($variant:ident($elem:ty, $ctor:ident, $empty:ident)),+ $(,)?) => {
        /// 列表存储: 标量元素使用紧凑表示, 其余类型回退到 `Value`.
        #[derive(Debug, Clone)]
        pub enum List {
            $($variant(imbl::Vector<$elem>),)+
            Values(imbl::Vector<Value>),
        }

        impl List {
            $(
                #[must_use]
                pub(crate) fn $ctor(items: impl IntoIterator<Item = $elem>) -> Self {
                    Self::$variant(items.into_iter().collect())
                }

                #[must_use]
                pub(crate) fn $empty() -> Self {
                    Self::$variant(imbl::Vector::new())
                }
            )+

            #[must_use]
            pub(crate) fn from_values(items: impl IntoIterator<Item = Value>) -> Self {
                Self::Values(items.into_iter().collect())
            }

            #[must_use]
            pub(crate) fn empty_values() -> Self {
                Self::Values(imbl::Vector::new())
            }
        }
    };
}

list_variants! {
    I64(i64, i64s, empty_i64),
    U8(u8, u8s, empty_u8),
    U64(u64, u64s, empty_u64),
    F64(f64, f64s, empty_f64),
    Char(char, chars, empty_char),
    Bool(bool, bools, empty_bool),
}

/// 以只读方式分派到具体向量.
macro_rules! list_with {
    ($list:expr, | $items:ident | $body:expr) => {
        match $list {
            List::I64($items) => $body,
            List::U8($items) => $body,
            List::U64($items) => $body,
            List::F64($items) => $body,
            List::Char($items) => $body,
            List::Bool($items) => $body,
            List::Values($items) => $body,
        }
    };
}

/// 以只读方式分派, 并重建为同一变体.
macro_rules! list_map {
    ($list:expr, | $items:ident | $body:expr) => {
        match $list {
            List::I64($items) => List::I64($body),
            List::U8($items) => List::U8($body),
            List::U64($items) => List::U64($body),
            List::F64($items) => List::F64($body),
            List::Char($items) => List::Char($body),
            List::Bool($items) => List::Bool($body),
            List::Values($items) => List::Values($body),
        }
    };
}

/// 按同一变体配对分派; 形态不同时回退为 `Values`.
macro_rules! list_pair {
    ($first:expr, $second:expr, | $left:ident, $right:ident | $body:expr) => {
        match ($first, $second) {
            (List::I64($left), List::I64($right)) => List::I64($body),
            (List::U8($left), List::U8($right)) => List::U8($body),
            (List::U64($left), List::U64($right)) => List::U64($body),
            (List::F64($left), List::F64($right)) => List::F64($body),
            (List::Char($left), List::Char($right)) => List::Char($body),
            (List::Bool($left), List::Bool($right)) => List::Bool($body),
            (List::Values($left), List::Values($right)) => List::Values($body),
            (first, second) => List::Values(first.to_values() + second.to_values()),
        }
    };
}

/// 向同形态向量追加; 值不属于该形态时原样返回.
fn push_value<T: ListElem>(
    items: &mut imbl::Vector<T>,
    value: Value,
    front: bool,
) -> Result<(), Value> {
    match T::from_value(value) {
        Ok(value) => {
            if front {
                items.push_front(value);
            } else {
                items.push_back(value);
            }
            Ok(())
        },
        Err(value) => Err(value),
    }
}

/// 反转向量, 保持元素类型.
fn reversed<T: Clone>(items: &imbl::Vector<T>) -> imbl::Vector<T> {
    items.iter().rev().cloned().collect()
}

/// 替换同形态向量中的一个元素; 值不属于该形态时原样返回.
fn set_value<T: ListElem>(
    items: &mut imbl::Vector<T>,
    index: usize,
    value: Value,
) -> Result<(), Value> {
    match T::from_value(value) {
        Ok(value) => {
            items.set(index, value);
            Ok(())
        },
        Err(value) => Err(value),
    }
}

impl List {
    #[must_use]
    pub(crate) fn same_shape(&self) -> Self {
        list_map!(self, |_items| imbl::Vector::new())
    }

    #[must_use]
    pub(crate) fn len(&self) -> usize {
        list_with!(self, |items| items.len())
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub(crate) fn get_value(&self, index: usize) -> Option<Value> {
        list_with!(self, |items| { items.get(index).map(ListElem::to_value) })
    }

    #[must_use]
    pub(crate) fn front_value(&self) -> Option<Value> {
        list_with!(self, |items| items.front().map(ListElem::to_value))
    }

    #[must_use]
    pub(crate) fn skip(&self, count: usize) -> Self {
        let count = count.min(self.len());
        list_map!(self, |items| items.skip(count))
    }

    #[must_use]
    pub(crate) fn take(&self, count: usize) -> Self {
        let count = count.min(self.len());
        list_map!(self, |items| items.take(count))
    }

    #[must_use]
    pub(crate) fn slice(&self, start: usize, count: usize) -> Self {
        self.skip(start).take(count)
    }

    #[must_use]
    pub(crate) fn reverse(&self) -> Self {
        list_map!(self, |items| reversed(items))
    }

    pub(crate) fn push_back(&mut self, value: Value) {
        match list_with!(self, |items| push_value(items, value, false)) {
            Ok(()) => {},
            Err(value) => {
                self.convert_to_values();
                self.push_back(value);
            },
        }
    }

    pub(crate) fn push_front(&mut self, value: Value) {
        match list_with!(self, |items| push_value(items, value, true)) {
            Ok(()) => {},
            Err(value) => {
                self.convert_to_values();
                self.push_front(value);
            },
        }
    }

    pub(crate) fn set(&mut self, index: usize, value: Value) {
        match list_with!(self, |items| set_value(items, index, value)) {
            Ok(()) => {},
            Err(value) => {
                self.convert_to_values();
                self.set(index, value);
            },
        }
    }

    #[must_use]
    pub(crate) fn concat(self, other: Self) -> Self {
        // 右操作数较小时逐元素追加, 避免为少量元素重平衡整棵 RRB 树.
        const SMALL_CONCAT: usize = 128;
        if other.len() <= SMALL_CONCAT {
            return self.append(other);
        }
        list_pair!(self, other, |left, right| left + right)
    }

    /// 逐元素追加到同形态列表; 混合形态回退为 `Value` 列表.
    fn append(self, other: Self) -> Self {
        list_pair!(self, other, |left, right| {
            let mut left = left;
            left.extend(right);
            left
        })
    }

    #[must_use]
    pub(crate) fn to_values(&self) -> imbl::Vector<Value> {
        match self {
            // 持久化向量可直接共享, 避免逐元素复制.
            Self::Values(items) => items.clone(),
            other => list_with!(other, |items| {
                items.iter().map(ListElem::to_value).collect()
            }),
        }
    }

    fn convert_to_values(&mut self) {
        *self = Self::Values(self.to_values());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_operations_keep_scalar_shape() {
        let list = List::i64s([1, 2, 3]);
        assert!(matches!(list.reverse(), List::I64(_)));
        assert!(matches!(list.skip(1), List::I64(_)));
        assert!(matches!(list.take(2), List::I64(_)));
        assert!(matches!(list.same_shape(), List::I64(_)));
        assert!(matches!(list.get_value(0), Some(Value::I64(1))));
        assert!(matches!(list.front_value(), Some(Value::I64(1))));

        let mut mixed = List::i64s([1]);
        mixed.push_back(Value::Nil);
        assert!(matches!(mixed, List::Values(_)));
        assert_eq!(mixed.len(), 2);
    }
}
