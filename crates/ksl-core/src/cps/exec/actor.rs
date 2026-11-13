//! Actor 原语辅助.

use std::sync::Arc;

use super::{Control, Machine};
use crate::error::RuntimeError as Error;
use crate::source::Span;
use crate::value::{ActorHandle, Value};

impl Machine {
    pub(super) fn pid_serial(&mut self, target: &ActorHandle) {
        self.control = Control::Return(Value::U64(target.id));
    }

    pub(super) fn start_send(&mut self, args: Vec<Value>, span: Span) -> Result<(), Error> {
        let mut args = super::PrimArgs::new(args, span);
        let target = args.pid("Send")?;
        let index = args.u8_count()?;
        let message = args.next()?;
        let delivered = !target.is_finished();
        if delivered {
            if !target.send(index, message) {
                return Err(Error::runtime(
                    crate::error::ErrorKind::IndexOutOfRange,
                    Some(span),
                ));
            }
            if let Some(scheduler) = &self.scheduler {
                scheduler.wake_send(target.id, index);
            }
        }
        self.control = Control::Return(Value::Bool(delivered));
        Ok(())
    }

    pub(super) fn spawn_actor(
        &mut self,
        handler: Value,
        count: usize,
        args: Vec<Value>,
        detached: bool,
        _span: Span,
    ) -> Result<(), Error> {
        let Some(scheduler) = self.scheduler.clone() else {
            return Err(Error::internal("Spawn 需要调度器"));
        };
        let parent = self.actor.as_ref().map(|actor| Arc::clone(&actor.pid));
        let pid = scheduler.spawn(parent.as_ref(), count, detached, handler, args);
        self.control = Control::Return(pid);
        Ok(())
    }
}
