//! 列表原语与高阶循环.

use std::sync::Arc;

use ksl_base::source::Span;
use ksl_base::types::Type;
use ksl_catalog::ops::{ListOpKind, PairOpKind};
use ksl_diagnostics::error::{ErrorKind, RuntimeError as Error};
use ksl_value::value::{List, Value, list_owned, option_none, option_some};

use super::{Control, Kont, ListLoop, Machine, PrimArgs};

impl Machine {
    /// 配对访问: 取首分量或次分量.
    pub(super) fn pair_prim(
        op: PairOpKind,
        args: &mut Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        let mut args = PrimArgs::new(args, span);
        let Value::Pair(pair) = args.next()? else {
            return Err(Error::internal("配对访问的实参不是配对"));
        };
        Ok(match op {
            PairOpKind::Fst => pair.0.clone(),
            PairOpKind::Snd => pair.1.clone(),
        })
    }

    /// 不回调用户函数的列表原语.
    pub(super) fn list_prim(
        op: ListOpKind,
        args: &mut Vec<Value>,
        ty: &Type,
        span: Span,
    ) -> Result<Value, Error> {
        use ListOpKind as K;
        debug_assert!(!op.is_higher_order(), "高阶操作由循环处理");
        let mut args = PrimArgs::new(args, span);
        match op {
            K::Get | K::Lookup | K::Head | K::IsEmpty => Self::list_access(op, &mut args),
            K::Update
            | K::Prepend
            | K::Append
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
            | K::Chunk => Self::list_build(op, &mut args, ty),
            _ => unreachable!("高阶操作由循环处理"),
        }
    }

    /// 不改变列表形态的访问与判定: 索引, 头部与空判定.
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
            K::Lookup => {
                let list = args.list()?;
                let index = args.count()?;
                match list.get_value(index) {
                    Some(value) => option_some(value),
                    None => option_none(),
                }
            },
            K::Head => args
                .list()?
                .front_value()
                .ok_or_else(|| Error::runtime(ErrorKind::EmptyListHead, Some(span)))?,
            K::IsEmpty => Value::Bool(args.list()?.is_empty()),
            _ => unreachable!("list_access 只接收访问类操作"),
        })
    }

    /// 构造新列表的操作: 索引更新, 截取与窗口.
    fn list_build(op: ListOpKind, args: &mut PrimArgs, ty: &Type) -> Result<Value, Error> {
        use ListOpKind as K;
        match op {
            K::Update | K::Prepend | K::Append | K::Tail | K::Reverse | K::Take | K::Drop => {
                Self::list_edit(op, args)
            },
            K::Chunk | K::Window | K::Riffle | K::Zip => Self::list_group(op, args),
            K::Repeat | K::Flatten => Self::list_expand(op, args, ty),
            K::Length => Self::list_length(args),
            _ => unreachable!("list_build 只接收构造类操作"),
        }
    }

    /// 修改或截取已有列表.
    fn list_edit(op: ListOpKind, args: &mut PrimArgs) -> Result<Value, Error> {
        use ListOpKind as K;
        let span = args.span;
        let value = match op {
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
            K::Tail => Value::List(Arc::new(args.list()?.skip(1))),
            K::Reverse => Value::List(Arc::new(args.list()?.reverse())),
            K::Take | K::Drop => {
                let list = args.list()?;
                let count = args.count()?;
                Value::List(Arc::new(if op == K::Take {
                    list.take(count)
                } else {
                    list.skip(count)
                }))
            },
            _ => unreachable!("list_edit 只接收修改与截取操作"),
        };
        Ok(value)
    }

    /// 分组, 插值, 窗口与配对.
    fn list_group(op: ListOpKind, args: &mut PrimArgs) -> Result<Value, Error> {
        use ListOpKind as K;
        let span = args.span;
        let value = match op {
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
            _ => unreachable!("list_group 只接收分组类操作"),
        };
        Ok(value)
    }

    /// 复制, 摊平与长度.
    fn list_expand(op: ListOpKind, args: &mut PrimArgs, ty: &Type) -> Result<Value, Error> {
        use ListOpKind as K;
        let value = match op {
            K::Repeat => {
                let value = args.next()?;
                let count = args.count()?;
                let mut result = List::empty_for(ty);
                for _ in 0..count {
                    result.push_back(value.clone());
                }
                Value::List(Arc::new(result))
            },
            K::Flatten => {
                let list = args.list()?;
                let mut result = List::empty_for(ty);
                for inner in list.to_values() {
                    let Value::List(inner) = inner else {
                        return Err(Error::internal("Flatten 的元素不是 List"));
                    };
                    for item in inner.to_values() {
                        result.push_back(item);
                    }
                }
                Value::List(Arc::new(result))
            },
            _ => unreachable!("list_expand 只接收复制与摊平操作"),
        };
        Ok(value)
    }

    fn list_length(args: &mut PrimArgs) -> Result<Value, Error> {
        let list = args.list()?;
        Ok(Value::U64(
            u64::try_from(list.len()).map_err(|_| Error::internal("List 长度超出 u64 上限"))?,
        ))
    }

    pub(super) fn start_list_loop(
        &mut self,
        op: ListOpKind,
        mut args: Vec<Value>,
        ty: &Type,
        span: Span,
    ) -> Result<(), Error> {
        let mut args = PrimArgs::new(&mut args, span);
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
            | ListOpKind::ZipWith => List::empty_for(ty),
            ListOpKind::Filter | ListOpKind::TakeWhile | ListOpKind::DropWhile => {
                list.same_shape()
            },
            _ => List::empty_values(),
        };
        if op == ListOpKind::PrefixFold {
            kept.push_back(acc.clone());
        }
        self.advance_list_loop(Box::new(ListLoop {
            op,
            function,
            list,
            pos: 0,
            second,
            pos2: 0,
            current: None,
            current2: None,
            index: 0,
            kept,
            acc,
            span,
        }))
    }

    fn advance_list_loop(&mut self, mut frame: Box<ListLoop>) -> Result<(), Error> {
        // 下标游标逐元素推进, 不做逐元素的 skip; 列表整体留在帧中供环回收追踪.
        let item = if frame.op == ListOpKind::ZipWith {
            let Some(first) = frame.list.get_value(frame.pos) else {
                return self.finish_list_loop(frame);
            };
            let Some(second) = frame
                .second
                .as_ref()
                .and_then(|list| list.get_value(frame.pos2))
            else {
                return self.finish_list_loop(frame);
            };
            frame.pos += 1;
            frame.pos2 += 1;
            frame.current2 = Some(second.clone());
            (first, Some(second))
        } else {
            let Some(item) = frame.list.get_value(frame.pos) else {
                return self.finish_list_loop(frame);
            };
            frame.pos += 1;
            (item, None)
        };
        let mut args = self.take_values();
        match frame.op {
            ListOpKind::Fold | ListOpKind::PrefixFold => {
                args.push(item.0.clone());
                args.push(frame.acc.clone());
            },
            ListOpKind::MapIndexed => {
                let index = frame.index;
                frame.index += 1;
                args.push(Value::U64(index));
                args.push(item.0.clone());
            },
            ListOpKind::FindIndex => {
                frame.index += 1;
                args.push(item.0.clone());
            },
            ListOpKind::ZipWith => {
                args.push(item.0.clone());
                args.push(item.1.expect("ZipWith 本轮应有第二元素"));
            },
            _ => args.push(item.0.clone()),
        }
        // 只有判定类操作需要保留原元素以决定是否放入结果.
        let keep_current = matches!(
            frame.op,
            ListOpKind::Filter
                | ListOpKind::FindIndex
                | ListOpKind::TakeWhile
                | ListOpKind::DropWhile
        );
        frame.current = keep_current.then_some(item.0);
        let function = frame.function.clone();
        let span = frame.span;
        self.kont.push(Kont::ListLoop(frame));
        self.apply_value(function, args, Some(span))
    }

    fn finish_list_loop(&mut self, frame: Box<ListLoop>) -> Result<(), Error> {
        let frame = *frame;
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
            ListOpKind::FindIndex => option_none(),
            ListOpKind::Fold => frame.acc,
            _ => return Err(Error::internal("非法的 List 循环操作")),
        };
        self.control = Control::Return(value);
        Ok(())
    }

    pub(super) fn list_loop_return(
        &mut self,
        mut frame: Box<ListLoop>,
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
                    return Err(Error::internal("过滤函数未返回 bool"));
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
                    return Err(Error::internal("条件函数未返回 bool"));
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
                    return Err(Error::internal("条件函数未返回 bool"));
                };
                if drop {
                    self.advance_list_loop(frame)
                } else {
                    let mut result = frame.list.skip(frame.pos);
                    result.push_front(
                        current.ok_or_else(|| Error::internal("循环缺少当前元素"))?,
                    );
                    self.control = Control::Return(Value::List(Arc::new(result)));
                    Ok(())
                }
            },
            ListOpKind::Any | ListOpKind::All => {
                let Value::Bool(verdict) = value else {
                    return Err(Error::internal("判定函数未返回 bool"));
                };
                let expect = frame.op == ListOpKind::Any;
                if verdict == expect {
                    self.control = Control::Return(Value::Bool(expect));
                    Ok(())
                } else {
                    self.advance_list_loop(frame)
                }
            },
            ListOpKind::FindIndex => {
                let Value::Bool(found) = value else {
                    return Err(Error::internal("查找函数未返回 bool"));
                };
                if found {
                    let index = frame
                        .index
                        .checked_sub(1)
                        .ok_or_else(|| Error::internal("查找循环缺少下标"))?;
                    self.control = Control::Return(option_some(Value::U64(index)));
                    Ok(())
                } else {
                    self.advance_list_loop(frame)
                }
            },
            ListOpKind::Fold => {
                frame.acc = value;
                self.advance_list_loop(frame)
            },
            ListOpKind::PrefixFold => {
                frame.acc = value.clone();
                frame.kept.push_back(value);
                self.advance_list_loop(frame)
            },
            _ => Err(Error::internal("非法的 List 循环操作")),
        }
    }
}
