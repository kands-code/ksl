//! 命名注册表原语.

use super::{Control, Machine};
use crate::ast::RegistryOp;
use crate::check::Type;
use crate::error::RuntimeError as Error;
use crate::source::Span;
use crate::value::{Value, option_none, option_some};

impl Machine {
    pub(super) fn start_registry_op(
        &mut self,
        op: RegistryOp,
        args: &[Value],
        arg_types: &[Type],
        ty: &Type,
        _span: Span,
    ) -> Result<(), Error> {
        let scheduler = self
            .scheduler
            .clone()
            .ok_or_else(|| Error::internal("命名注册表需要调度器"))?;
        let Some(Value::Str(text)) = args.first() else {
            return Err(Error::internal("注册表操作的名字不是字符串"));
        };
        let value = match op {
            RegistryOp::Register => {
                let Some(Value::Pid(pid)) = args.get(1) else {
                    return Err(Error::internal("Register 的目标不是 Pid"));
                };
                let pid_ty = arg_types
                    .get(1)
                    .cloned()
                    .ok_or_else(|| Error::internal("Register 缺少 Pid 类型"))?;
                Value::Bool(scheduler.register_name(
                    std::sync::Arc::clone(text),
                    std::sync::Arc::clone(pid),
                    pid_ty,
                ))
            },
            RegistryOp::WhereIs => {
                let Type::OptionT(inner) = ty else {
                    return Err(Error::internal("WhereIs 的结果类型不是 Option[Pid]"));
                };
                match scheduler.where_is_name(text, inner) {
                    Some(pid) => option_some(Value::Pid(pid)),
                    None => option_none(),
                }
            },
            RegistryOp::Unregister => Value::Bool(scheduler.unregister_name(text)),
        };
        self.control = Control::Return(value);
        Ok(())
    }
}
