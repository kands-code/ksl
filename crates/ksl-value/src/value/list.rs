//! 列表存储与操作.

use ksl_base::types::Type;

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
    i32 => I32,
    u8 => U8,
    u32 => U32,
    u64 => U64,
    f32 => F32,
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
                pub fn $ctor(items: impl IntoIterator<Item = $elem>) -> Self {
                    Self::$variant(items.into_iter().collect())
                }

                #[must_use]
                pub fn $empty() -> Self {
                    Self::$variant(imbl::Vector::new())
                }
            )+

            #[must_use]
            pub fn from_values(items: impl IntoIterator<Item = Value>) -> Self {
                Self::Values(items.into_iter().collect())
            }

            #[must_use]
            pub fn empty_values() -> Self {
                Self::Values(imbl::Vector::new())
            }
        }
    };
}

list_variants! {
    I64(i64, i64s, empty_i64),
    I32(i32, i32s, empty_i32),
    U8(u8, u8s, empty_u8),
    U32(u32, u32s, empty_u32),
    U64(u64, u64s, empty_u64),
    F32(f32, f32s, empty_f32),
    F64(f64, f64s, empty_f64),
    Char(char, chars, empty_char),
    Bool(bool, bools, empty_bool),
}

/// 以只读方式分派到具体向量.
macro_rules! list_with {
    ($list:expr, | $items:ident | $body:expr) => {
        match $list {
            List::I64($items) => $body,
            List::I32($items) => $body,
            List::U8($items) => $body,
            List::U32($items) => $body,
            List::U64($items) => $body,
            List::F32($items) => $body,
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
            List::I32($items) => List::I32($body),
            List::U8($items) => List::U8($body),
            List::U32($items) => List::U32($body),
            List::U64($items) => List::U64($body),
            List::F32($items) => List::F32($body),
            List::F64($items) => List::F64($body),
            List::Char($items) => List::Char($body),
            List::Bool($items) => List::Bool($body),
            List::Values($items) => List::Values($body),
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
    /// 按列表类型选择空存储; 非 List 类型回退到 `Values`.
    #[must_use]
    pub fn empty_for(ty: &Type) -> Self {
        let Some(inner) = ty.as_list() else {
            return Self::empty_values();
        };
        match inner {
            Type::I64 => Self::empty_i64(),
            Type::I32 => Self::empty_i32(),
            Type::U8 => Self::empty_u8(),
            Type::U32 => Self::empty_u32(),
            Type::U64 => Self::empty_u64(),
            Type::F32 => Self::empty_f32(),
            Type::F64 => Self::empty_f64(),
            Type::Char => Self::empty_char(),
            Type::Bool => Self::empty_bool(),
            _ => Self::empty_values(),
        }
    }

    /// 按静态元素类型构造列表; 元素与类型不符时返回 None.
    ///
    /// 标量元素使用紧凑表示, 其余类型回退到 `Values`.
    #[must_use]
    pub fn from_values_typed(elem: &Type, values: Vec<Value>) -> Option<Self> {
        match elem {
            Type::I64 => values
                .into_iter()
                .map(|value| value.as_i64())
                .collect::<Option<Vec<_>>>()
                .map(Self::i64s),
            Type::I32 => values
                .into_iter()
                .map(|value| value.as_i32())
                .collect::<Option<Vec<_>>>()
                .map(Self::i32s),
            Type::U8 => values
                .into_iter()
                .map(|value| value.as_u8())
                .collect::<Option<Vec<_>>>()
                .map(Self::u8s),
            Type::U32 => values
                .into_iter()
                .map(|value| value.as_u32())
                .collect::<Option<Vec<_>>>()
                .map(Self::u32s),
            Type::U64 => values
                .into_iter()
                .map(|value| value.as_u64())
                .collect::<Option<Vec<_>>>()
                .map(Self::u64s),
            Type::F32 => values
                .into_iter()
                .map(|value| value.as_f32())
                .collect::<Option<Vec<_>>>()
                .map(Self::f32s),
            Type::F64 => values
                .into_iter()
                .map(|value| value.as_f64())
                .collect::<Option<Vec<_>>>()
                .map(Self::f64s),
            Type::Char => values
                .into_iter()
                .map(|value| value.as_char())
                .collect::<Option<Vec<_>>>()
                .map(Self::chars),
            Type::Bool => values
                .into_iter()
                .map(|value| value.as_bool())
                .collect::<Option<Vec<_>>>()
                .map(Self::bools),
            _ => Some(Self::from_values(values)),
        }
    }

    #[must_use]
    pub fn same_shape(&self) -> Self {
        list_map!(self, |_items| imbl::Vector::new())
    }

    #[must_use]
    pub fn len(&self) -> usize {
        list_with!(self, |items| items.len())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[must_use]
    pub fn get_value(&self, index: usize) -> Option<Value> {
        list_with!(self, |items| { items.get(index).map(ListElem::to_value) })
    }

    #[must_use]
    pub fn front_value(&self) -> Option<Value> {
        list_with!(self, |items| items.front().map(ListElem::to_value))
    }

    #[must_use]
    pub fn skip(&self, count: usize) -> Self {
        let count = count.min(self.len());
        list_map!(self, |items| items.skip(count))
    }

    #[must_use]
    pub fn take(&self, count: usize) -> Self {
        let count = count.min(self.len());
        list_map!(self, |items| items.take(count))
    }

    #[must_use]
    pub fn slice(&self, start: usize, count: usize) -> Self {
        self.skip(start).take(count)
    }

    #[must_use]
    pub fn reverse(&self) -> Self {
        list_map!(self, |items| reversed(items))
    }

    pub fn push_back(&mut self, value: Value) {
        match list_with!(self, |items| push_value(items, value, false)) {
            Ok(()) => {},
            Err(value) => {
                self.convert_to_values();
                self.push_back(value);
            },
        }
    }

    pub fn push_front(&mut self, value: Value) {
        match list_with!(self, |items| push_value(items, value, true)) {
            Ok(()) => {},
            Err(value) => {
                self.convert_to_values();
                self.push_front(value);
            },
        }
    }

    pub fn set(&mut self, index: usize, value: Value) {
        match list_with!(self, |items| set_value(items, index, value)) {
            Ok(()) => {},
            Err(value) => {
                self.convert_to_values();
                self.set(index, value);
            },
        }
    }

    #[must_use]
    pub fn to_values(&self) -> imbl::Vector<Value> {
        match self {
            // 持久化向量直接共享.
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
