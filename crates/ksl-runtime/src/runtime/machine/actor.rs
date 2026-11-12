//! Actor 原语辅助.

use ksl_base::source::Span;
use ksl_diagnostics::error::{ErrorKind, RuntimeError as Error};
use ksl_value::value::Value;

use super::{Machine, PrimArgs};
use crate::runtime::event::Event;

impl Machine {
    pub(super) fn send_prim(
        &mut self,
        args: &mut Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        let mut args = PrimArgs::new(args, span);
        let target = args.pid("Send")?;
        let index = args.u8_count()?;
        let message = args.next()?;
        if target.is_finished() {
            return Ok(Value::Bool(false));
        }
        if !target.send(index, message) {
            return Err(Error::runtime(ErrorKind::IndexOutOfRange, Some(span)));
        }
        if target.take_wake(index) {
            self.notify(Event::SendWake {
                target: target.id,
                index,
            });
        }
        Ok(Value::Bool(true))
    }
}
