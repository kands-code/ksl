//! 协调器的任务定时器.
//!
//! 每个 Actor 至多一个活动定时器; 睡眠与带超时的接收共用该堆.

use super::{Coordinator, Duration, Error, ErrorKind, Instant, Span, Value, option_none};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum TimerKind {
    Sleep,
    Receive,
}

impl Coordinator {
    pub(super) fn add_timer(
        &mut self,
        millis: u64,
        actor: u64,
        kind: TimerKind,
        span: Span,
    ) -> Result<(), Error> {
        let duration = Duration::from_millis(millis);
        let Some(deadline) = Instant::now().checked_add(duration) else {
            return Err(Error::runtime(ErrorKind::TimeoutOutOfRange, Some(span)));
        };
        self.timers.insert(deadline, actor, kind);
        Ok(())
    }

    pub(in crate::runtime::scheduler) fn fire_timers(&mut self) {
        // 没有活动定时器时不取当前时间. 调度内循环的热路径上, 取时间是系统调用开销.
        if self.timers.is_empty() {
            return;
        }
        for (actor, kind) in self.timers.take_due(Instant::now()) {
            let value = match kind {
                TimerKind::Sleep => Value::Nil,
                TimerKind::Receive => option_none(),
            };
            self.resume_task(actor, value);
        }
    }
}
