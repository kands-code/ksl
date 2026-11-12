//! 延迟投递原语.

use std::time::{Duration, Instant};

use super::{Control, Machine, PrimArgs};
use crate::error::{ErrorKind, RuntimeError as Error};
use crate::source::Span;
use crate::value::{Value, option_none, option_some};

impl Machine {
    pub(super) fn start_set_timer(
        &mut self,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(), Error> {
        let mut args = PrimArgs::new(args, span);
        let target = args.pid("SetTimer")?;
        let index = args.u8_count()?;
        let message = args.next()?;
        let Value::U64(millis) = args.next()? else {
            return Err(Error::internal("SetTimer 的延迟不是 u64"));
        };
        let Some(deadline) = Instant::now().checked_add(Duration::from_millis(millis)) else {
            return Err(Error::runtime(ErrorKind::TimeoutOutOfRange, Some(span)));
        };
        let owner = self.actor.as_ref().map(|actor| actor.pid.id);
        let Some(scheduler) = &self.scheduler else {
            return Err(Error::internal("@SetTimer 需要调度器"));
        };
        let value = match scheduler.set_timer(&target, index, message, deadline, owner) {
            Some(id) => option_some(Value::U64(id)),
            None => option_none(),
        };
        self.control = Control::Return(value);
        Ok(())
    }

    pub(super) fn start_cancel_timer(&mut self, id: u64, _span: Span) -> Result<(), Error> {
        let owner = self.actor.as_ref().map(|actor| actor.pid.id);
        let Some(scheduler) = &self.scheduler else {
            return Err(Error::internal("@CancelTimer 需要调度器"));
        };
        let cancelled = scheduler.cancel_timer(id, owner);
        self.control = Control::Return(Value::Bool(cancelled));
        Ok(())
    }
}
