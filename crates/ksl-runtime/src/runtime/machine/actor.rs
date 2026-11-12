//! Actor 原语辅助.

use ksl_base::source::Span;
use ksl_frontend::error::{ErrorKind, RuntimeError as Error};

use super::{Control, Machine, PrimArgs};
use crate::runtime::event::Event;
use crate::value::{ActorHandle, Value};

impl Machine {
    pub(super) fn pid_serial(&mut self, target: &ActorHandle) {
        self.control = Control::Return(Value::U64(target.id));
    }

    pub(super) fn start_send(&mut self, args: Vec<Value>, span: Span) -> Result<(), Error> {
        let mut args = PrimArgs::new(args, span);
        let target = args.pid("Send")?;
        let index = args.u8_count()?;
        let message = args.next()?;
        if target.is_finished() {
            self.control = Control::Return(Value::Bool(false));
            return Ok(());
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
        self.control = Control::Return(Value::Bool(true));
        Ok(())
    }
}
