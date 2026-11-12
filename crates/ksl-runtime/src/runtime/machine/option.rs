//! Option 与 Result 原语.

use ksl_base::name::{Name, keywords};
use ksl_base::source::Span;
use ksl_diagnostics::error::RuntimeError as Error;
use ksl_plan::OptionPrim;
use ksl_value::value::Value;

use super::{Control, Kont, Machine, PrimArgs};

impl Machine {
    fn variant_is(value: &Value, variant: &Name) -> Result<bool, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Option/Result 的实参不是 Data 值"));
        };
        Ok(data.variant.as_str() == variant.as_str())
    }

    fn variant_payload(value: &Value, variant: &Name) -> Result<Option<Value>, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Option/Result 的实参不是 Data 值"));
        };
        if data.variant.as_str() != variant.as_str() {
            return Ok(None);
        }
        Ok(data.fields.first().cloned())
    }

    /// 不回调用户函数的 Option/Result 判定.
    pub(super) fn option_prim(
        op: &OptionPrim,
        args: &mut Vec<Value>,
        span: Span,
    ) -> Result<Value, Error> {
        use OptionPrim as O;
        let mut args = PrimArgs::new(args, span);
        let first = args.next()?;
        match op {
            O::IsSome | O::IsNone => {
                let is_some = Self::variant_is(&first, &keywords().some_.clone())?;
                Ok(Value::Bool(if *op == O::IsSome { is_some } else { !is_some }))
            },
            O::IsOk | O::IsErr => {
                let is_ok = Self::variant_is(&first, &keywords().ok_.clone())?;
                Ok(Value::Bool(if *op == O::IsOk { is_ok } else { !is_ok }))
            },
            _ => Err(Error::internal("Option 映射操作由异步通道处理")),
        }
    }

    /// 映射与替代操作: 需要回调用户函数.
    pub(super) fn start_option_map(
        &mut self,
        op: OptionPrim,
        mut args: Vec<Value>,
        span: Span,
    ) -> Result<(), Error> {
        use OptionPrim as O;
        let mut args = PrimArgs::new(&mut args, span);
        let first = args.next()?;
        match op {
            O::OptionMap | O::OptionAndThen => {
                let Some(payload) = Self::variant_payload(&first, &keywords().some_.clone())?
                else {
                    self.control = Control::Return(first);
                    return Ok(());
                };
                let function = args.next()?;
                let mut call_args = self.take_values();
                call_args.push(payload);
                self.kont.push(Kont::OptionMap { op });
                self.apply_value(function, call_args, Some(span))?;
            },
            O::ResultMap | O::ResultAndThen => {
                let Some(payload) = Self::variant_payload(&first, &keywords().ok_.clone())?
                else {
                    self.control = Control::Return(first);
                    return Ok(());
                };
                let function = args.next()?;
                let mut call_args = self.take_values();
                call_args.push(payload);
                self.kont.push(Kont::OptionMap { op });
                self.apply_value(function, call_args, Some(span))?;
            },
            // 替代类操作: 成功时原样返回, 失败时调用替代函数, 其返回值即结果.
            O::ResultOrElse => {
                if Self::variant_is(&first, &keywords().ok_.clone())? {
                    self.control = Control::Return(first);
                    return Ok(());
                }
                let Some(error) = Self::variant_payload(&first, &keywords().err_.clone())?
                else {
                    return Err(Error::internal("Result 的实参不是 Ok 或 Err"));
                };
                let function = args.next()?;
                let mut call_args = self.take_values();
                call_args.push(error);
                self.apply_value(function, call_args, Some(span))?;
            },
            _ => return Err(Error::internal("Option 判定操作由同步通道处理")),
        }
        Ok(())
    }

    pub(super) fn parse_timeout(value: &Value) -> Result<Option<u64>, Error> {
        let Value::Data(data) = value else {
            return Err(Error::internal("Receive 的超时不是 Option[u64]"));
        };
        if data.variant.as_str() == keywords().none_.as_str() {
            return Ok(None);
        }
        if data.variant.as_str() == keywords().some_.as_str() {
            let Some(Value::U64(millis)) = data.fields.first() else {
                return Err(Error::internal("Receive 的超时负载不是 u64"));
            };
            return Ok(Some(*millis));
        }
        Err(Error::internal("Receive 的超时不是 Option[u64]"))
    }
}
