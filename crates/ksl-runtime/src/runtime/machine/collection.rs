//! Dict 与 Set 原语.

use std::sync::Arc;

use ksl_base::source::Span;
use ksl_frontend::ast::{DictOpKind, SetOpKind};
use ksl_frontend::check::Type;
use ksl_frontend::error::RuntimeError as Error;

use super::list::empty_for_type;
use super::{Control, DictFilterLoop, Kont, Machine, SetFilterLoop};
use crate::value::{Key, List, Value, list, option_none, option_some};

impl Machine {
    pub(super) fn start_dict_literal(
        &mut self,
        args: Vec<Value>,
        _span: Span,
    ) -> Result<(), Error> {
        let mut map = imbl::OrdMap::new();
        for arg in args {
            let (key, value) = pair_entry(&arg)?;
            map.insert(key, value);
        }
        self.control = Control::Return(Value::Dict(Arc::new(map)));
        Ok(())
    }

    pub(super) fn start_set_literal(
        &mut self,
        args: Vec<Value>,
        _span: Span,
    ) -> Result<(), Error> {
        let mut set = imbl::OrdSet::new();
        for arg in args {
            set.insert(key_of(&arg)?);
        }
        self.control = Control::Return(Value::Set(Arc::new(set)));
        Ok(())
    }

    pub(super) fn start_dict_op(
        &mut self,
        op: DictOpKind,
        mut args: Vec<Value>,
        ty: &Type,
        span: Span,
    ) -> Result<(), Error> {
        let value =
            match op {
                DictOpKind::Get | DictOpKind::Update => {
                    return Err(Error::internal("DictGet/DictUpdate 应在降级期展开"));
                },
                DictOpKind::Filter => {
                    return self.start_dict_filter(&args, span);
                },
                DictOpKind::Lookup => {
                    let map = dict_arg(&args, 0)?;
                    let key = key_arg(&args, 1)?;
                    map.get(&key).cloned().map_or_else(option_none, option_some)
                },
                DictOpKind::Put => {
                    // 实参持有字典的唯一引用时原地插入, 共享时才复制路径.
                    let mut map = take_dict(&mut args, 0)?;
                    let key = key_arg(&args, 1)?;
                    let value = take_value(&mut args, 2)
                        .ok_or_else(|| Error::internal("DictPut 缺少值参数"))?;
                    let _ = Arc::make_mut(&mut map).insert(key, value);
                    Value::Dict(map)
                },
                DictOpKind::Remove => {
                    let mut map = take_dict(&mut args, 0)?;
                    let key = key_arg(&args, 1)?;
                    let _ = Arc::make_mut(&mut map).remove(&key);
                    Value::Dict(map)
                },
                DictOpKind::Has => {
                    let map = dict_arg(&args, 0)?;
                    let key = key_arg(&args, 1)?;
                    Value::Bool(map.contains_key(&key))
                },
                DictOpKind::Length => {
                    let map = dict_arg(&args, 0)?;
                    Value::U64(
                        u64::try_from(map.len())
                            .map_err(|_| Error::internal("字典长度超出 u64 上限"))?,
                    )
                },
                DictOpKind::IsEmpty => {
                    let map = dict_arg(&args, 0)?;
                    Value::Bool(map.is_empty())
                },
                DictOpKind::Entries => {
                    let map = dict_arg(&args, 0)?;
                    list(map.iter().map(|(key, value)| {
                        Value::Pair(Arc::new((key.to_value(), value.clone())))
                    }))
                },
                DictOpKind::Keys => {
                    let map = dict_arg(&args, 0)?;
                    let mut keys = empty_for_type(ty);
                    for key in map.keys() {
                        keys.push_back(key.to_value());
                    }
                    Value::List(Arc::new(keys))
                },
                DictOpKind::Values => {
                    let map = dict_arg(&args, 0)?;
                    let mut values = empty_for_type(ty);
                    for value in map.values() {
                        values.push_back(value.clone());
                    }
                    Value::List(Arc::new(values))
                },
                DictOpKind::Merge => {
                    let first = take_dict(&mut args, 0)?;
                    let second = take_dict(&mut args, 1)?;
                    // 冲突时以第二个字典的值为准.
                    let merged = dict_owned(second).union(dict_owned(first));
                    Value::Dict(Arc::new(merged))
                },
                DictOpKind::FromEntries => {
                    let entries = list_arg(&args, 0)?;
                    let mut map = imbl::OrdMap::new();
                    for entry in entries.to_values() {
                        let (key, value) = pair_entry(&entry)?;
                        map.insert(key, value);
                    }
                    Value::Dict(Arc::new(map))
                },
            };
        let _ = span;
        self.control = Control::Return(value);
        Ok(())
    }

    pub(super) fn start_set_op(
        &mut self,
        op: SetOpKind,
        mut args: Vec<Value>,
        ty: &Type,
        span: Span,
    ) -> Result<(), Error> {
        let value = match op {
            SetOpKind::Insert => {
                let mut set = take_set(&mut args, 0)?;
                let key = key_arg(&args, 1)?;
                let _ = Arc::make_mut(&mut set).insert(key);
                Value::Set(set)
            },
            SetOpKind::Remove => {
                let mut set = take_set(&mut args, 0)?;
                let key = key_arg(&args, 1)?;
                let _ = Arc::make_mut(&mut set).remove(&key);
                Value::Set(set)
            },
            SetOpKind::Has => {
                let set = set_arg(&args, 0)?;
                let key = key_arg(&args, 1)?;
                Value::Bool(set.contains(&key))
            },
            SetOpKind::Length => {
                let set = set_arg(&args, 0)?;
                Value::U64(
                    u64::try_from(set.len())
                        .map_err(|_| Error::internal("集合长度超出 u64 上限"))?,
                )
            },
            SetOpKind::IsEmpty => {
                let set = set_arg(&args, 0)?;
                Value::Bool(set.is_empty())
            },
            SetOpKind::Items => {
                let set = set_arg(&args, 0)?;
                let mut items = empty_for_type(ty);
                for key in set.iter() {
                    items.push_back(key.to_value());
                }
                Value::List(Arc::new(items))
            },
            SetOpKind::Filter => return self.start_set_filter(&args, span),
            SetOpKind::Union | SetOpKind::Intersect | SetOpKind::Difference => {
                let first = take_set(&mut args, 0)?;
                let second = take_set(&mut args, 1)?;
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
                let list = list_arg(&args, 0)?;
                let mut set = imbl::OrdSet::new();
                for value in list.to_values() {
                    set.insert(key_of(&value)?);
                }
                Value::Set(Arc::new(set))
            },
        };
        self.control = Control::Return(value);
        Ok(())
    }

    fn start_dict_filter(&mut self, args: &[Value], span: Span) -> Result<(), Error> {
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
        self.kont.push(Kont::DictFilter(Box::new(next)));
        self.apply_value(predicate, vec![key.to_value(), value], Some(span))
    }

    pub(super) fn dict_filter_return(
        &mut self,
        mut frame: DictFilterLoop,
        value: &Value,
    ) -> Result<(), Error> {
        let Value::Bool(keep) = value else {
            return Err(Error::internal("DictFilter 的谓词未返回布尔值"));
        };
        if *keep {
            let index = frame.index - 1;
            let (key, value) = frame.entries[index].clone();
            frame.kept.insert(key, value);
        }
        self.advance_dict_filter(frame)
    }

    fn start_set_filter(&mut self, args: &[Value], span: Span) -> Result<(), Error> {
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
        self.kont.push(Kont::SetFilter(Box::new(next)));
        self.apply_value(predicate, vec![key.to_value()], Some(span))
    }

    pub(super) fn set_filter_return(
        &mut self,
        mut frame: SetFilterLoop,
        value: &Value,
    ) -> Result<(), Error> {
        let Value::Bool(keep) = value else {
            return Err(Error::internal("SetFilter 的谓词未返回布尔值"));
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
        _ => Err(Error::internal("Dict 操作的实参不是字典")),
    }
}

fn set_arg(args: &[Value], index: usize) -> Result<Arc<imbl::OrdSet<Key>>, Error> {
    match args.get(index) {
        Some(Value::Set(set)) => Ok(Arc::clone(set)),
        _ => Err(Error::internal("Set 操作的实参不是集合")),
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
        _ => Err(Error::internal("Dict 操作的实参不是字典")),
    }
}

/// 取走集合实参; 实参持有唯一引用时可直接原地修改.
fn take_set(args: &mut [Value], index: usize) -> Result<Arc<imbl::OrdSet<Key>>, Error> {
    match take_value(args, index) {
        Some(Value::Set(set)) => Ok(set),
        _ => Err(Error::internal("Set 操作的实参不是集合")),
    }
}

/// 取出字典存储: 引用计数为一时直接取走, 否则复制一份.
fn dict_owned(map: Arc<imbl::OrdMap<Key, Value>>) -> imbl::OrdMap<Key, Value> {
    Arc::try_unwrap(map).unwrap_or_else(|shared| (*shared).clone())
}

/// 取出集合存储: 引用计数为一时直接取走, 否则复制一份.
fn set_owned(set: Arc<imbl::OrdSet<Key>>) -> imbl::OrdSet<Key> {
    Arc::try_unwrap(set).unwrap_or_else(|shared| (*shared).clone())
}

fn list_arg(args: &[Value], index: usize) -> Result<List, Error> {
    match args.get(index) {
        Some(Value::List(list)) => Ok((**list).clone()),
        _ => Err(Error::internal("集合转换的实参不是列表")),
    }
}
