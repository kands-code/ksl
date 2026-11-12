//! Option 与 Result 原语.

use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, keywords};
use ksl_frontend::error::RuntimeError as Error;

use super::{Control, Kont, Machine, PrimArgs};
use crate::cps::ir::OptionPrim;
use crate::value::Value;

impl Machine {
    fn variant_is(value: &Value, variant: Symbol) -> Result<bool, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Option/Result 的实参不是 Data 值"));
        };
        Ok(data.variant == variant)
    }

    fn variant_payload(value: &Value, variant: Symbol) -> Result<Option<Value>, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Option/Result 的实参不是 Data 值"));
        };
        if data.variant != variant {
            return Ok(None);
        }
        Ok(data.fields.first().cloned())
    }

    pub(super) fn start_option_op(
        &mut self,
        op: OptionPrim,
        args: Vec<Value>,
        span: Span,
    ) -> Result<(), Error> {
        use OptionPrim as O;
        let mut args = PrimArgs::new(args, span);
        let first = args.next()?;
        match op {
            O::IsSome | O::IsNone => {
                let is_some = Self::variant_is(&first, keywords().some_)?;
                let value = if op == O::IsSome { is_some } else { !is_some };
                self.control = Control::Return(Value::Bool(value));
            },
            O::IsOk | O::IsErr => {
                let is_ok = Self::variant_is(&first, keywords().ok_)?;
                let value = if op == O::IsOk { is_ok } else { !is_ok };
                self.control = Control::Return(Value::Bool(value));
            },
            O::OptionMap | O::OptionAndThen => {
                let Some(payload) = Self::variant_payload(&first, keywords().some_)? else {
                    self.control = Control::Return(first);
                    return Ok(());
                };
                let function = args.next()?;
                self.kont.push(Kont::OptionMap { op });
                self.apply_value(function, vec![payload], Some(span))?;
            },
            O::ResultMap | O::ResultAndThen => {
                let Some(payload) = Self::variant_payload(&first, keywords().ok_)? else {
                    self.control = Control::Return(first);
                    return Ok(());
                };
                let function = args.next()?;
                self.kont.push(Kont::OptionMap { op });
                self.apply_value(function, vec![payload], Some(span))?;
            },
            // 替代类操作: 成功时原样返回, 失败时调用替代函数, 其返回值即结果.
            O::ResultOrElse => {
                if Self::variant_is(&first, keywords().ok_)? {
                    self.control = Control::Return(first);
                    return Ok(());
                }
                let Some(error) = Self::variant_payload(&first, keywords().err_)? else {
                    return Err(Error::internal("Result 的实参不是 Ok 或 Err"));
                };
                let function = args.next()?;
                self.apply_value(function, vec![error], Some(span))?;
            },
        }
        Ok(())
    }

    pub(super) fn parse_timeout(value: &Value) -> Result<Option<u64>, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Receive 的超时不是 Option[u64]"));
        };
        if data.variant == keywords().none_ {
            return Ok(None);
        }
        if data.variant == keywords().some_ {
            let Some(Value::U64(millis)) = data.fields.first() else {
                return Err(Error::internal("Receive 的超时负载不是 u64"));
            };
            return Ok(Some(*millis));
        }
        Err(Error::internal("Receive 的超时不是 Option[u64]"))
    }
}
