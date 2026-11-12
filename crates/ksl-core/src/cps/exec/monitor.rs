//! Actor 监视原语.

use std::sync::Arc;

use super::{Control, Machine};
use crate::error::RuntimeError as Error;
use crate::source::Span;
use crate::value::{ActorHandle, Value, option_none, option_some};

impl Machine {
    pub(super) fn start_monitor(
        &mut self,
        target: &Arc<ActorHandle>,
        _span: Span,
    ) -> Result<(), Error> {
        let watcher = self
            .actor
            .as_ref()
            .map(|actor| Arc::clone(&actor.pid))
            .ok_or_else(|| Error::internal("@Monitor 出现在 Handler 外"))?;
        let scheduler = self
            .scheduler
            .clone()
            .ok_or_else(|| Error::internal("@Monitor 需要调度器"))?;
        let value = match scheduler.monitor(&watcher, target) {
            Some(id) => option_some(Value::U64(id)),
            None => option_none(),
        };
        self.control = Control::Return(value);
        Ok(())
    }

    pub(super) fn start_unmonitor(&mut self, id: u64, _span: Span) -> Result<(), Error> {
        let watcher = self
            .actor
            .as_ref()
            .map(|actor| actor.pid.id)
            .ok_or_else(|| Error::internal("@Unmonitor 出现在 Handler 外"))?;
        let scheduler = self
            .scheduler
            .clone()
            .ok_or_else(|| Error::internal("@Unmonitor 需要调度器"))?;
        let removed = scheduler.unmonitor(id, watcher);
        self.control = Control::Return(Value::Bool(removed));
        Ok(())
    }
}
