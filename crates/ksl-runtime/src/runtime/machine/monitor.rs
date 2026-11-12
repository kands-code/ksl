//! Actor 监视原语.

use ksl_base::source::Span;
use ksl_frontend::error::RuntimeError as Error;

use super::{Control, Effect, Machine, PrimArgs};
use crate::value::Value;

impl Machine {
    pub(super) fn start_monitor(&mut self, args: Vec<Value>, span: Span) -> Result<(), Error> {
        let mut args = PrimArgs::new(args, span);
        let target = args.pid("Monitor")?;
        self.control = Control::Effect(Effect::Monitor { target });
        Ok(())
    }

    pub(super) fn start_unmonitor(
        &mut self,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(), Error> {
        let mut args = PrimArgs::new(args, span);
        let Value::U64(id) = args.next()? else {
            return Err(Error::internal("Unmonitor 的编号不是 u64"));
        };
        self.control = Control::Effect(Effect::Unmonitor { id });
        Ok(())
    }
}
