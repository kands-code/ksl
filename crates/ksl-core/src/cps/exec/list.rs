//! 列表原语与高阶循环.

use std::sync::Arc;

use super::{Control, Kont, ListLoop, Machine, PrimArgs};
use crate::ast::{ListOpKind, PairOpKind};
use crate::check::Type;
use crate::error::{ErrorKind, RuntimeError as Error};
use crate::source::Span;
use crate::value::{List, Value, list_owned};

/// 按结果类型选择空列表表示.
pub(super) fn empty_for_type(ty: &Type) -> List {
    let Type::List(inner) = ty else {
        return List::empty_values();
    };
    match &**inner {
        Type::I64 => List::empty_i64(),
        Type::U8 => List::empty_u8(),
        Type::U64 => List::empty_u64(),
        Type::F64 => List::empty_f64(),
        Type::Char => List::empty_char(),
        Type::Bool => List::empty_bool(),
        _ => List::empty_values(),
    }
}

impl Machine {
    /// 配对访问: 取首分量或次分量.
    pub(super) fn start_pair_op(
        &mut self,
        op: PairOpKind,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(), Error> {
        let mut args = PrimArgs::new(args, span);
        let Value::Pair(pair) = args.next()? else {
            return Err(Error::internal("配对访问的实参不是配对"));
        };
        let value = match op {
            PairOpKind::Fst => pair.0.clone(),
            PairOpKind::Snd => pair.1.clone(),
        };
        self.control = Control::Return(value);
        Ok(())
    }

    pub(super) fn start_list_op(
        &mut self,
        op: ListOpKind,
        args: Vec<Value>,
        ty: &Type,
        span: Span,
    ) -> Result<(), Error> {
        use ListOpKind as K;
        if matches!(
            op,
            K::TakeWhile
                | K::DropWhile
                | K::Map
                | K::MapIndexed
                | K::Filter
                | K::Fold
                | K::PrefixFold
                | K::Any
                | K::All
                | K::ZipWith
        ) {
            return self.start_list_loop(op, args, ty, span);
        }
        let mut args = PrimArgs::new(args, span);
        let value = match op {
            K::Get | K::Head => Self::list_access(op, &mut args)?,
            K::Update
            | K::Prepend
            | K::Append
            | K::Concat
            | K::Tail
            | K::Take
            | K::Drop
            | K::Length
            | K::Riffle
            | K::Window
            | K::Zip
            | K::Reverse
            | K::Repeat
            | K::Flatten
            | K::Chunk => Self::list_build(op, &mut args, ty)?,
            K::TakeWhile
            | K::DropWhile
            | K::Map
            | K::MapIndexed
            | K::Filter
            | K::Fold
            | K::PrefixFold
            | K::Any
            | K::All
            | K::ZipWith => {
                unreachable!("高阶操作已提前处理");
            },
        };
        self.control = Control::Return(value);
        Ok(())
    }

    /// 不改变列表形态的访问: 索引, 头部与配对分量.
    fn list_access(op: ListOpKind, args: &mut PrimArgs) -> Result<Value, Error> {
        use ListOpKind as K;
        let span = args.span;
        Ok(match op {
            K::Get => {
                let list = args.list()?;
                let index = args.count()?;
                list.get_value(index)
                    .ok_or_else(|| Error::runtime(ErrorKind::IndexOutOfRange, Some(span)))?
            },
            K::Head => args
                .list()?
                .front_value()
                .ok_or_else(|| Error::runtime(ErrorKind::EmptyListHead, Some(span)))?,
            _ => unreachable!("list_access 只接收访问类操作"),
        })
    }

    /// 构造新列表的操作: 索引更新, 拼接, 截取与窗口.
    #[allow(clippy::too_many_lines)]
    fn list_build(op: ListOpKind, args: &mut PrimArgs, ty: &Type) -> Result<Value, Error> {
        use ListOpKind as K;
        let span = args.span;
        Ok(match op {
            K::Update => {
                let mut list = args.list()?;
                let index = args.count()?;
                let value = args.next()?;
                if index >= list.len() {
                    return Err(Error::runtime(ErrorKind::IndexOutOfRange, Some(span)));
                }
                Arc::make_mut(&mut list).set(index, value);
                Value::List(list)
            },
            K::Prepend | K::Append => {
                let mut list = args.list()?;
                let value = args.next()?;
                if op == K::Prepend {
                    Arc::make_mut(&mut list).push_front(value);
                } else {
                    Arc::make_mut(&mut list).push_back(value);
                }
                Value::List(list)
            },
            K::Concat => {
                let first = list_owned(args.list()?);
                let second = list_owned(args.list()?);
                Value::List(Arc::new(first.concat(second)))
            },
            K::Tail => Value::List(Arc::new(args.list()?.skip(1))),
            K::Reverse => Value::List(Arc::new(args.list()?.reverse())),
            K::Repeat => {
                let value = args.next()?;
                let count = args.count()?;
                let mut result = empty_for_type(ty);
                for _ in 0..count {
                    result.push_back(value.clone());
                }
                Value::List(Arc::new(result))
            },
            K::Flatten => {
                let list = args.list()?;
                let mut result = empty_for_type(ty);
                for inner in list.to_values() {
                    let Value::List(inner) = inner else {
                        return Err(Error::internal("Flatten 的元素不是列表"));
                    };
                    for item in inner.to_values() {
                        result.push_back(item);
                    }
                }
                Value::List(Arc::new(result))
            },
            K::Chunk => {
                let list = args.list()?;
                let size = args.count()?;
                if size == 0 {
                    return Err(Error::runtime(ErrorKind::InvalidChunkSize, Some(span)));
                }
                let mut chunks: imbl::Vector<Value> = imbl::Vector::new();
                let mut start = 0;
                while start < list.len() {
                    chunks.push_back(Value::List(Arc::new(list.slice(start, size))));
                    start += size;
                }
                Value::List(Arc::new(List::from_values(chunks)))
            },
            K::Take | K::Drop => {
                let list = args.list()?;
                let count = args.count()?;
                Value::List(Arc::new(if op == K::Take {
                    list.take(count)
                } else {
                    list.skip(count)
                }))
            },
            K::Length => {
                let list = args.list()?;
                Value::U64(
                    u64::try_from(list.len())
                        .map_err(|_| Error::internal("列表长度超出 u64 上限"))?,
                )
            },
            K::Riffle => {
                let list = list_owned(args.list()?);
                let elem = args.next()?;
                let every = args.count()?;
                if every == 0 {
                    return Err(Error::runtime(ErrorKind::InvalidInterval, Some(span)));
                }
                let mut riffled = list.same_shape();
                for (index, item) in list.to_values().into_iter().enumerate() {
                    if index > 0 && index % every == 0 {
                        riffled.push_back(elem.clone());
                    }
                    riffled.push_back(item);
                }
                Value::List(Arc::new(riffled))
            },
            K::Window => {
                let list = args.list()?;
                let size = args.count()?;
                if size == 0 {
                    return Err(Error::runtime(ErrorKind::InvalidWindowSize, Some(span)));
                }
                let count = list.len().saturating_add(1).saturating_sub(size);
                let windows: imbl::Vector<Value> = (0..count)
                    .map(|start| Value::List(Arc::new(list.slice(start, size))))
                    .collect();
                Value::List(Arc::new(List::from_values(windows)))
            },
            K::Zip => {
                let first = list_owned(args.list()?).to_values();
                let second = list_owned(args.list()?).to_values();
                let pairs: imbl::Vector<Value> = first
                    .into_iter()
                    .zip(second)
                    .map(|(a, b)| Value::Pair(Arc::new((a, b))))
                    .collect();
                Value::List(Arc::new(List::from_values(pairs)))
            },
            _ => unreachable!("list_build 只接收构造类操作"),
        })
    }

    fn start_list_loop(
        &mut self,
        op: ListOpKind,
        args: Vec<Value>,
        ty: &Type,
        span: Span,
    ) -> Result<(), Error> {
        let mut args = PrimArgs::new(args, span);
        let (list, second, function, acc) = if op == ListOpKind::ZipWith {
            let first = list_owned(args.list()?);
            let second = list_owned(args.list()?);
            let function = args.next()?;
            (first, Some(second), function, Value::Nil)
        } else {
            let list = list_owned(args.list()?);
            let function = args.next()?;
            let acc = if matches!(op, ListOpKind::Fold | ListOpKind::PrefixFold) {
                args.next()?
            } else {
                Value::Nil
            };
            (list, None, function, acc)
        };
        let mut kept = match op {
            ListOpKind::Map
            | ListOpKind::MapIndexed
            | ListOpKind::PrefixFold
            | ListOpKind::ZipWith => empty_for_type(ty),
            ListOpKind::Filter | ListOpKind::TakeWhile | ListOpKind::DropWhile => {
                list.same_shape()
            },
            _ => List::empty_values(),
        };
        if op == ListOpKind::PrefixFold {
            kept.push_back(acc.clone());
        }
        self.advance_list_loop(ListLoop {
            op,
            function,
            list,
            second,
            current: None,
            current2: None,
            index: 0,
            kept,
            acc,
            span,
        })
    }

    fn advance_list_loop(&mut self, mut frame: ListLoop) -> Result<(), Error> {
        let item = if frame.op == ListOpKind::ZipWith {
            let Some(first) = frame.list.front_value() else {
                return self.finish_list_loop(frame);
            };
            let Some(second) = frame.second.as_ref().and_then(crate::value::List::front_value)
            else {
                return self.finish_list_loop(frame);
            };
            frame.list = frame.list.skip(1);
            if let Some(list) = frame.second.as_mut() {
                *list = list.skip(1);
            }
            frame.current2 = Some(second.clone());
            (first, Some(second))
        } else {
            let Some(item) = frame.list.front_value() else {
                return self.finish_list_loop(frame);
            };
            frame.list = frame.list.skip(1);
            (item, None)
        };
        let args = match frame.op {
            ListOpKind::Fold | ListOpKind::PrefixFold => {
                vec![item.0.clone(), frame.acc.clone()]
            },
            ListOpKind::MapIndexed => {
                let index = frame.index;
                frame.index += 1;
                vec![Value::U64(index), item.0.clone()]
            },
            ListOpKind::ZipWith => {
                vec![item.0.clone(), item.1.expect("ZipWith 本轮应有第二元素")]
            },
            _ => vec![item.0.clone()],
        };
        frame.current = Some(item.0);
        let function = frame.function.clone();
        let span = frame.span;
        self.kont.push(Kont::ListLoop(Box::new(frame)));
        self.apply_value(function, args, Some(span))
    }

    fn finish_list_loop(&mut self, frame: ListLoop) -> Result<(), Error> {
        let value = match frame.op {
            ListOpKind::Map
            | ListOpKind::MapIndexed
            | ListOpKind::Filter
            | ListOpKind::TakeWhile
            | ListOpKind::DropWhile
            | ListOpKind::PrefixFold
            | ListOpKind::ZipWith => Value::List(Arc::new(frame.kept)),
            ListOpKind::Any => Value::Bool(false),
            ListOpKind::All => Value::Bool(true),
            ListOpKind::Fold => frame.acc,
            _ => return Err(Error::internal("非法的列表循环操作")),
        };
        self.control = Control::Return(value);
        Ok(())
    }

    pub(super) fn list_loop_return(
        &mut self,
        mut frame: ListLoop,
        value: Value,
    ) -> Result<(), Error> {
        let current = frame.current.take();
        match frame.op {
            ListOpKind::Map | ListOpKind::MapIndexed | ListOpKind::ZipWith => {
                frame.kept.push_back(value);
                self.advance_list_loop(frame)
            },
            ListOpKind::Filter => {
                let Value::Bool(keep) = value else {
                    return Err(Error::internal("过滤函数未返回布尔值"));
                };
                if keep {
                    frame.kept.push_back(
                        current.ok_or_else(|| Error::internal("过滤循环缺少当前元素"))?,
                    );
                }
                self.advance_list_loop(frame)
            },
            ListOpKind::TakeWhile => {
                let Value::Bool(keep) = value else {
                    return Err(Error::internal("条件函数未返回布尔值"));
                };
                if keep {
                    frame
                        .kept
                        .push_back(current.ok_or_else(|| Error::internal("循环缺少当前元素"))?);
                    self.advance_list_loop(frame)
                } else {
                    self.control = Control::Return(Value::List(Arc::new(frame.kept)));
                    Ok(())
                }
            },
            ListOpKind::DropWhile => {
                let Value::Bool(drop) = value else {
                    return Err(Error::internal("条件函数未返回布尔值"));
                };
                if drop {
                    self.advance_list_loop(frame)
                } else {
                    let mut result = frame.list;
                    result.push_front(
                        current.ok_or_else(|| Error::internal("循环缺少当前元素"))?,
                    );
                    self.control = Control::Return(Value::List(Arc::new(result)));
                    Ok(())
                }
            },
            ListOpKind::Any | ListOpKind::All => {
                let Value::Bool(verdict) = value else {
                    return Err(Error::internal("判定函数未返回布尔值"));
                };
                let expect = frame.op == ListOpKind::Any;
                if verdict == expect {
                    self.control = Control::Return(Value::Bool(expect));
                    Ok(())
                } else {
                    self.advance_list_loop(frame)
                }
            },
            ListOpKind::Fold | ListOpKind::PrefixFold => {
                frame.acc = value.clone();
                frame.kept.push_back(value);
                self.advance_list_loop(frame)
            },
            _ => Err(Error::internal("非法的列表循环操作")),
        }
    }
}
