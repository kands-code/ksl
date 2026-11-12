//! 命名注册表原语.

use ksl_base::source::Span;
use ksl_frontend::ast::RegistryOp;
use ksl_frontend::check::Type;
use ksl_frontend::error::RuntimeError as Error;

use super::{Control, Effect, Machine};
use crate::value::Value;

impl Machine {
    pub(super) fn start_registry_op(
        &mut self,
        op: RegistryOp,
        args: &[Value],
        arg_types: &[Type],
        ty: &Type,
        _span: Span,
    ) -> Result<(), Error> {
        let Some(Value::Str(text)) = args.first() else {
            return Err(Error::internal("注册表操作的名字不是字符串"));
        };
        match op {
            RegistryOp::Register => {
                let Some(Value::Pid(pid)) = args.get(1) else {
                    return Err(Error::internal("Register 的目标不是 Pid"));
                };
                let pid_ty = arg_types
                    .get(1)
                    .cloned()
                    .ok_or_else(|| Error::internal("Register 缺少 Pid 类型"))?;
                self.control = Control::Effect(Effect::Register {
                    name: text.clone(),
                    pid: std::sync::Arc::clone(pid),
                    ty: pid_ty,
                });
            },
            RegistryOp::WhereIs => {
                let Some(inner) = ty.as_option() else {
                    return Err(Error::internal("WhereIs 的结果类型不是 Option[Pid]"));
                };
                self.control = Control::Effect(Effect::WhereIs {
                    name: text.clone(),
                    ty: inner.clone(),
                });
            },
            RegistryOp::Unregister => {
                self.control = Control::Effect(Effect::Unregister { name: text.clone() });
            },
        }
        Ok(())
    }
}
