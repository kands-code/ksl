//! Dict 与 Set 原语.

use std::sync::Arc;

use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::ops::{DictOpKind, SetOpKind};
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_value::value::{Key, List, Value, list, option_none, option_some};

use super::{Control, DictFilterLoop, Kont, Machine, SetFilterLoop};

impl Machine {
    pub(super) fn dict_literal(args: &mut Vec<Value>) -> Result<Value, Error> {
        let mut map = imbl::OrdMap::new();
        for arg in args.drain(..) {
            let (key, value) = pair_entry(&arg)?;
            map.insert(key, value);
        }
        Ok(Value::Dict(Arc::new(map)))
    }

    pub(super) fn set_literal(args: &mut Vec<Value>) -> Result<Value, Error> {
        let mut set = imbl::OrdSet::new();
        for arg in args.drain(..) {
            set.insert(key_of(&arg)?);
        }
        Ok(Value::Set(Arc::new(set)))
    }

    pub(super) fn dict_prim(
        op: DictOpKind,
        args: &mut Vec<Value>,
        ty: &Type,
    ) -> Result<Value, Error> {
        let value =
            match op {
                DictOpKind::Get | DictOpKind::Update => {
                    return Err(Error::internal("DictGet/DictUpdate 应在降级期展开"));
                },
                DictOpKind::Filter => {
                    return Err(Error::internal("DictFilter 由异步通道处理"));
                },
                DictOpKind::Lookup => {
                    let map = dict_arg(args.as_slice(), 0)?;
                    let key = key_arg(args.as_slice(), 1)?;
                    map.get(&key).cloned().map_or_else(option_none, option_some)
                },
                DictOpKind::Put => {
                    // 实参持有字典的唯一引用时原地插入, 共享时才复制路径.
                    let mut map = take_dict(args, 0)?;
                    let key = key_arg(args.as_slice(), 1)?;
                    let value = take_value(args, 2)
                        .ok_or_else(|| Error::internal("DictPut 缺少值参数"))?;
                    let _ = Arc::make_mut(&mut map).insert(key, value);
                    Value::Dict(map)
                },
                DictOpKind::Remove => {
                    let mut map = take_dict(args, 0)?;
                    let key = key_arg(args.as_slice(), 1)?;
                    let _ = Arc::make_mut(&mut map).remove(&key);
                    Value::Dict(map)
                },
                DictOpKind::Has => {
                    let map = dict_arg(args.as_slice(), 0)?;
                    let key = key_arg(args.as_slice(), 1)?;
                    Value::Bool(map.contains_key(&key))
                },
                DictOpKind::Length => {
                    let map = dict_arg(args.as_slice(), 0)?;
                    Value::U64(
                        u64::try_from(map.len())
                            .map_err(|_| Error::internal("Dict 长度超出 u64 上限"))?,
                    )
                },
                DictOpKind::IsEmpty => {
                    let map = dict_arg(args.as_slice(), 0)?;
                    Value::Bool(map.is_empty())
                },
                DictOpKind::Entries => {
                    let map = dict_arg(args.as_slice(), 0)?;
                    list(map.iter().map(|(key, value)| {
                        Value::Pair(Arc::new((key.to_value(), value.clone())))
                    }))
                },
                DictOpKind::Keys => {
                    let map = dict_arg(args.as_slice(), 0)?;
                    let mut keys = List::empty_for(ty);
                    for key in map.keys() {
                        keys.push_back(key.to_value());
                    }
                    Value::List(Arc::new(keys))
                },
                DictOpKind::Values => {
                    let map = dict_arg(args.as_slice(), 0)?;
                    let mut values = List::empty_for(ty);
                    for value in map.values() {
                        values.push_back(value.clone());
                    }
                    Value::List(Arc::new(values))
                },
                DictOpKind::Merge => {
                    let first = take_dict(args, 0)?;
                    let second = take_dict(args, 1)?;
                    // 冲突时以第二个字典的值为准.
                    let merged = dict_owned(second).union(dict_owned(first));
                    Value::Dict(Arc::new(merged))
                },
                DictOpKind::FromEntries => {
                    let entries = list_arg(args.as_slice(), 0)?;
                    let mut map = imbl::OrdMap::new();
                    for entry in entries.to_values() {
                        let (key, value) = pair_entry(&entry)?;
                        map.insert(key, value);
                    }
                    Value::Dict(Arc::new(map))
                },
            };
        Ok(value)
    }

    pub(super) fn set_prim(
        op: SetOpKind,
        args: &mut Vec<Value>,
        ty: &Type,
    ) -> Result<Value, Error> {
        let value = match op {
            SetOpKind::Insert => {
                let mut set = take_set(args, 0)?;
                let key = key_arg(args.as_slice(), 1)?;
                let _ = Arc::make_mut(&mut set).insert(key);
                Value::Set(set)
            },
            SetOpKind::Remove => {
                let mut set = take_set(args, 0)?;
                let key = key_arg(args.as_slice(), 1)?;
                let _ = Arc::make_mut(&mut set).remove(&key);
                Value::Set(set)
            },
            SetOpKind::Has => {
                let set = set_arg(args.as_slice(), 0)?;
                let key = key_arg(args.as_slice(), 1)?;
                Value::Bool(set.contains(&key))
            },
            SetOpKind::Length => {
                let set = set_arg(args.as_slice(), 0)?;
                Value::U64(
                    u64::try_from(set.len())
                        .map_err(|_| Error::internal("Set 长度超出 u64 上限"))?,
                )
            },
            SetOpKind::IsEmpty => {
                let set = set_arg(args.as_slice(), 0)?;
                Value::Bool(set.is_empty())
            },
            SetOpKind::Items => {
                let set = set_arg(args.as_slice(), 0)?;
                let mut items = List::empty_for(ty);
                for key in set.iter() {
                    items.push_back(key.to_value());
                }
                Value::List(Arc::new(items))
            },
            SetOpKind::Filter => {
                return Err(Error::internal("SetFilter 由异步通道处理"));
            },
            SetOpKind::Union | SetOpKind::Intersect | SetOpKind::Difference => {
                let first = take_set(args, 0)?;
                let second = take_set(args, 1)?;
                let (first, second) = (set_owned(first), set_owned(second));
                let result = match op {
                    SetOpKind::Union => first.union(second),
                    SetOpKind::Intersect => first.intersection(second),
                    SetOpKind::Difference => first.relative_complement(second),
                    _ => unreachable!("集合运算已限定"),
                };
                Value::Set(Arc::new(result))
            },
            SetOpKind::FromList => {
                let list = list_arg(args.as_slice(), 0)?;
                let mut set = imbl::OrdSet::new();
                for value in list.to_values() {
                    set.insert(key_of(&value)?);
                }
                Value::Set(Arc::new(set))
            },
        };
        Ok(value)
    }

    pub(super) fn start_dict_filter(
        &mut self,
        args: &[Value],
        span: Span,
    ) -> Result<(), Error> {
        let map = dict_arg(args, 0)?;
        let predicate = args
            .get(1)
            .cloned()
            .ok_or_else(|| Error::internal("DictFilter 缺少谓词"))?;
        let entries = map
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        self.advance_dict_filter(DictFilterLoop {
            entries,
            index: 0,
            predicate,
            kept: imbl::OrdMap::new(),
            span,
        })
    }

    fn advance_dict_filter(&mut self, frame: DictFilterLoop) -> Result<(), Error> {
        let Some((key, value)) = frame.entries.get(frame.index).cloned() else {
            self.control = Control::Return(Value::Dict(Arc::new(frame.kept)));
            return Ok(());
        };
        let mut next = frame;
        next.index += 1;
        let predicate = next.predicate.clone();
        let span = next.span;
        let mut args = self.take_values();
        args.push(key.to_value());
        args.push(value);
        self.kont.push(Kont::DictFilter(Box::new(next)));
        self.apply_value(predicate, args, Some(span))
    }

    pub(super) fn dict_filter_return(
        &mut self,
        mut frame: DictFilterLoop,
        value: &Value,
    ) -> Result<(), Error> {
        let Value::Bool(keep) = value else {
            return Err(Error::internal("DictFilter 的谓词未返回 bool"));
        };
        if *keep {
            let index = frame.index - 1;
            let (key, value) = frame.entries[index].clone();
            frame.kept.insert(key, value);
        }
        self.advance_dict_filter(frame)
    }

    pub(super) fn start_set_filter(&mut self, args: &[Value], span: Span) -> Result<(), Error> {
        let set = set_arg(args, 0)?;
        let predicate = args
            .get(1)
            .cloned()
            .ok_or_else(|| Error::internal("SetFilter 缺少谓词"))?;
        let items = set.iter().cloned().collect();
        self.advance_set_filter(SetFilterLoop {
            items,
            index: 0,
            predicate,
            kept: imbl::OrdSet::new(),
            span,
        })
    }

    fn advance_set_filter(&mut self, frame: SetFilterLoop) -> Result<(), Error> {
        let Some(key) = frame.items.get(frame.index).cloned() else {
            self.control = Control::Return(Value::Set(Arc::new(frame.kept)));
            return Ok(());
        };
        let mut next = frame;
        next.index += 1;
        let predicate = next.predicate.clone();
        let span = next.span;
        let mut args = self.take_values();
        args.push(key.to_value());
        self.kont.push(Kont::SetFilter(Box::new(next)));
        self.apply_value(predicate, args, Some(span))
    }

    pub(super) fn set_filter_return(
        &mut self,
        mut frame: SetFilterLoop,
        value: &Value,
    ) -> Result<(), Error> {
        let Value::Bool(keep) = value else {
            return Err(Error::internal("SetFilter 的谓词未返回 bool"));
        };
        if *keep {
            let index = frame.index - 1;
            frame.kept.insert(frame.items[index].clone());
        }
        self.advance_set_filter(frame)
    }
}

fn pair_entry(value: &Value) -> Result<(Key, Value), Error> {
    let Value::Pair(pair) = value else {
        return Err(Error::internal("Dict 的条目不是配对"));
    };
    Ok((key_of(&pair.0)?, pair.1.clone()))
}

fn key_of(value: &Value) -> Result<Key, Error> {
    Key::from_value(value).ok_or_else(|| Error::internal("不支持的 Dict/Set 键类型"))
}

fn key_arg(args: &[Value], index: usize) -> Result<Key, Error> {
    let value = args
        .get(index)
        .ok_or_else(|| Error::internal("缺少 Dict/Set 键参数"))?;
    key_of(value)
}

fn dict_arg(args: &[Value], index: usize) -> Result<Arc<imbl::OrdMap<Key, Value>>, Error> {
    match args.get(index) {
        Some(Value::Dict(dict)) => Ok(Arc::clone(dict)),
        _ => Err(Error::internal("Dict 操作的实参不是 Dict")),
    }
}

fn set_arg(args: &[Value], index: usize) -> Result<Arc<imbl::OrdSet<Key>>, Error> {
    match args.get(index) {
        Some(Value::Set(set)) => Ok(Arc::clone(set)),
        _ => Err(Error::internal("Set 操作的实参不是 Set")),
    }
}

/// 取走指定位置的实参, 原位置换成 `nil`.
fn take_value(args: &mut [Value], index: usize) -> Option<Value> {
    args.get_mut(index)
        .map(|value| std::mem::replace(value, Value::Nil))
}

/// 取走字典实参; 实参持有唯一引用时可直接原地修改.
fn take_dict(args: &mut [Value], index: usize) -> Result<Arc<imbl::OrdMap<Key, Value>>, Error> {
    match take_value(args, index) {
        Some(Value::Dict(dict)) => Ok(dict),
        _ => Err(Error::internal("Dict 操作的实参不是 Dict")),
    }
}

/// 取走集合实参; 实参持有唯一引用时可直接原地修改.
fn take_set(args: &mut [Value], index: usize) -> Result<Arc<imbl::OrdSet<Key>>, Error> {
    match take_value(args, index) {
        Some(Value::Set(set)) => Ok(set),
        _ => Err(Error::internal("Set 操作的实参不是 Set")),
    }
}

/// 取出字典存储: 引用计数为一时直接取走, 否则复制一份.
fn dict_owned(map: Arc<imbl::OrdMap<Key, Value>>) -> imbl::OrdMap<Key, Value> {
    Arc::unwrap_or_clone(map)
}

/// 取出集合存储: 引用计数为一时直接取走, 否则复制一份.
fn set_owned(set: Arc<imbl::OrdSet<Key>>) -> imbl::OrdSet<Key> {
    Arc::unwrap_or_clone(set)
}

fn list_arg(args: &[Value], index: usize) -> Result<List, Error> {
    match args.get(index) {
        Some(Value::List(list)) => Ok((**list).clone()),
        _ => Err(Error::internal("Set 转换的实参不是 List")),
    }
}
