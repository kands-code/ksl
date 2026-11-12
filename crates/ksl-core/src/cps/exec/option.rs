//! Option 与 Result 原语.

use super::{Control, Kont, Machine, PrimArgs};
use crate::ast::OptionOpKind;
use crate::error::RuntimeError as Error;
use crate::source::Span;
use crate::symbol::intern;
use crate::value::Value;

impl Machine {
    fn variant_is(value: &Value, name: &str) -> Result<bool, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Option/Result 的实参不是 Data 值"));
        };
        Ok(data.variant == intern(name))
    }

    fn variant_payload(value: &Value, name: &str) -> Result<Option<Value>, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Option/Result 的实参不是 Data 值"));
        };
        if data.variant != intern(name) {
            return Ok(None);
        }
        Ok(data.fields.first().cloned())
    }

    pub(super) fn start_option_op(
        &mut self,
        op: OptionOpKind,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(), Error> {
        use OptionOpKind as O;
        let mut args = PrimArgs::new(args, span);
        let first = args.next()?;
        match op {
            O::IsSome | O::IsNone => {
                let is_some = Self::variant_is(&first, "Some")?;
                let value = if op == O::IsSome { is_some } else { !is_some };
                self.control = Control::Return(Value::Bool(value));
            },
            O::IsOk | O::IsErr => {
                let is_ok = Self::variant_is(&first, "Ok")?;
                let value = if op == O::IsOk { is_ok } else { !is_ok };
                self.control = Control::Return(Value::Bool(value));
            },
            O::OptionMap | O::OptionAndThen => {
                let Some(payload) = Self::variant_payload(&first, "Some")? else {
                    self.control = Control::Return(first);
                    return Ok(());
                };
                let function = args.next()?;
                self.kont.push(Kont::OptionMap { op });
                self.apply_value(function, vec![payload], Some(span))?;
            },
            O::ResultMap | O::ResultAndThen => {
                let Some(payload) = Self::variant_payload(&first, "Ok")? else {
                    self.control = Control::Return(first);
                    return Ok(());
                };
                let function = args.next()?;
                self.kont.push(Kont::OptionMap { op });
                self.apply_value(function, vec![payload], Some(span))?;
            },
            O::OptionGet => {
                let fallback = args.next()?;
                let value = Self::variant_payload(&first, "Some")?.unwrap_or(fallback);
                self.control = Control::Return(value);
            },
            O::ResultGet => {
                let fallback = args.next()?;
                let value = Self::variant_payload(&first, "Ok")?.unwrap_or(fallback);
                self.control = Control::Return(value);
            },
        }
        Ok(())
    }

    pub(super) fn parse_timeout(value: &Value) -> Result<Option<u64>, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Receive 的超时不是 Option[u64]"));
        };
        if data.variant == intern("None") {
            return Ok(None);
        }
        if data.variant == intern("Some") {
            let Some(Value::U64(millis)) = data.fields.first() else {
                return Err(Error::internal("Receive 的超时负载不是 u64"));
            };
            return Ok(Some(*millis));
        }
        Err(Error::internal("Receive 的超时不是 Option[u64]"))
    }
}
