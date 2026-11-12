//! 延迟投递原语.

use std::time::{Duration, Instant};

use ksl_base::source::Span;
use ksl_frontend::error::{ErrorKind, RuntimeError as Error};

use super::{Control, Effect, Machine, PrimArgs};
use crate::value::Value;

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
        self.control = Control::Effect(Effect::SetTimer {
            target,
            index,
            message,
            deadline,
        });
        Ok(())
    }

    pub(super) fn start_cancel_timer(
        &mut self,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(), Error> {
        let mut args = PrimArgs::new(args, span);
        let Value::U64(id) = args.next()? else {
            return Err(Error::internal("CancelTimer 的编号不是 u64"));
        };
        self.control = Control::Effect(Effect::CancelTimer { id });
        Ok(())
    }
}
