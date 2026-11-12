//! 特殊形式与语句级语法.

use super::{Context, Parser};
use crate::ast::{
    BindingTarget, DataVariant, Def, Expr, ExprKind, MatchArm, Param, Special, TypeExpr,
};
use crate::error::{ErrorKind, KslError};
use crate::lexer::{Token, TokenKind};
use crate::symbol::{keywords, resolve};

impl Parser<'_> {
    #[allow(clippy::too_many_lines)]
    pub(super) fn special_form(&mut self) -> Result<ExprKind, KslError> {
        self.bump(); // @
        let name = self.ident()?;
        self.expect(&TokenKind::LBracket)?;
        let kw = keywords();
        let node = name.node;
        let form = if node == kw.let_ {
            if !matches!(self.context, Context::Top | Context::Statement) {
                return Err(KslError::new(ErrorKind::DefinitionNotAllowed, name.span));
            }
            self.form_let()?
        } else if node == kw.block_ {
            Special::Block(self.block_stmts()?)
        } else if node == kw.if_ {
            let cond = self.expr(Context::Expression)?;
            self.comma()?;
            let then = self.expr(Context::Expression)?;
            self.comma()?;
            let els = self.expr(Context::Expression)?;
            Special::If {
                cond: Box::new(cond),
                then: Box::new(then),
                els: Box::new(els),
            }
        } else if node == kw.fn_ {
            self.form_fn()?
        } else if node == kw.def_ {
            if !matches!(self.context, Context::Top | Context::Statement) {
                return Err(KslError::new(ErrorKind::DefinitionNotAllowed, name.span));
            }
            self.form_def()?
        } else if node == kw.exit_ {
            if self.context != Context::Top {
                return Err(KslError::new(ErrorKind::ExitNotTopLevel, name.span));
            }
            let code = self.expr(Context::Expression)?;
            Special::Exit {
                code: Box::new(code),
            }
        } else if node == kw.handler_ {
            let (params, ret, body) = self.params_ret_body()?;
            Special::Handler {
                params,
                ret,
                body: Box::new(body),
            }
        } else if node == kw.continue_ {
            Special::Continue {
                args: self.comma_exprs()?,
            }
        } else if node == kw.spawn_ {
            self.form_spawn(false)?
        } else if node == kw.spawn_detached_ {
            self.form_spawn(true)?
        } else if node == kw.send_ {
            self.form_send()?
        } else if node == kw.receive_ {
            self.form_receive()?
        } else if node == kw.join_ {
            self.form_join()?
        } else if node == kw.kill_ {
            self.form_kill()?
        } else if node == kw.monitor_ {
            self.form_monitor()?
        } else if node == kw.unmonitor_ {
            self.form_unmonitor()?
        } else if node == kw.pid_serial_ {
            self.form_pid_serial()?
        } else if node == kw.set_timer_ {
            self.form_set_timer()?
        } else if node == kw.cancel_timer_ {
            self.form_cancel_timer()?
        } else if node == kw.halt_ {
            self.form_halt()?
        } else if node == kw.sleep_ {
            self.form_sleep()?
        } else if node == kw.fix_ {
            if !matches!(self.context, Context::Top | Context::Statement) {
                return Err(KslError::new(ErrorKind::DefinitionNotAllowed, name.span));
            }
            self.form_fix()?
        } else if node == kw.data_ {
            if self.context != Context::Top {
                return Err(KslError::new(ErrorKind::DataNotTopLevel, name.span));
            }
            self.form_data()?
        } else if node == kw.export_ {
            if self.context != Context::Top {
                return Err(KslError::new(ErrorKind::ModuleFormNotTopLevel, name.span));
            }
            self.form_export()?
        } else if node == kw.use_ {
            if self.context != Context::Top {
                return Err(KslError::new(ErrorKind::ModuleFormNotTopLevel, name.span));
            }
            self.form_use()?
        } else if node == kw.match_ {
            self.form_match()?
        } else if node == kw.assert_ {
            let cond = self.expr(Context::Expression)?;
            self.comma()?;
            let message = self.expr(Context::Expression)?;
            Special::Assert {
                cond: Box::new(cond),
                message: Box::new(message),
            }
        } else if node == kw.panic_ {
            Special::Panic {
                message: Box::new(self.expr(Context::Expression)?),
            }
        } else if node == kw.todo_ {
            Special::Todo {
                message: Box::new(self.expr(Context::Expression)?),
            }
        } else if node == kw.unreachable_ {
            Special::Unreachable {
                message: Box::new(self.expr(Context::Expression)?),
            }
        } else if let Some(op) = crate::ast::option_op_kind(resolve(node).as_ref()) {
            // 形式闭括号由调用方统一消费, 此处只解析参数.
            Special::OptionOp {
                op,
                args: self.comma_exprs()?,
            }
        } else if let Some(op) = crate::ast::list_op_kind(resolve(node).as_ref()) {
            // 形式闭括号由调用方统一消费, 此处只解析参数.
            Special::ListOp {
                op,
                args: self.comma_exprs()?,
            }
        } else if let Some(op) = crate::ast::pair_op_kind(resolve(node).as_ref()) {
            Special::PairOp {
                op,
                args: self.comma_exprs()?,
            }
        } else if let Some(op) = crate::ast::dict_op_kind(resolve(node).as_ref()) {
            Special::DictOp {
                op,
                args: self.comma_exprs()?,
            }
        } else if let Some(op) = crate::ast::set_op_kind(resolve(node).as_ref()) {
            Special::SetOp {
                op,
                args: self.comma_exprs()?,
            }
        } else if let Some(op) = crate::ast::registry_op_kind(resolve(node).as_ref()) {
            Special::Registry {
                op,
                args: self.comma_exprs()?,
            }
        } else if resolve(node).as_ref() == "Dict" {
            Special::Dict {
                entries: self.comma_exprs()?,
            }
        } else if resolve(node).as_ref() == "Set" {
            Special::Set {
                items: self.comma_exprs()?,
            }
        } else {
            return Err(KslError::new(ErrorKind::UnknownSpecialForm, name.span));
        };
        self.expect(&TokenKind::RBracket)?;
        Ok(ExprKind::Special(form))
    }

    fn form_let(&mut self) -> Result<Special, KslError> {
        let target = self.binding_target()?;
        self.comma()?;
        let value = self.expr(Context::Expression)?;
        Ok(Special::Let {
            target,
            value: Box::new(value),
        })
    }

    /// 单个绑定目标: 名字或 `_`, 可带类型标注.
    fn binding_target(&mut self) -> Result<BindingTarget, KslError> {
        let name = match self.bump() {
            Token {
                kind: TokenKind::Ident(text),
                span,
            } => Some(self.spanned(*text, *span)),
            Token {
                kind: TokenKind::Discard,
                ..
            } => None,
            token => {
                return Err(KslError::new(ErrorKind::UnexpectedToken, token.span));
            },
        };
        let ty = if self.eat(&TokenKind::Colon) {
            Some(self.type_expr()?)
        } else {
            None
        };
        Ok(BindingTarget { name, ty })
    }

    /// `@Block` 的逗号分隔语句序列.
    fn block_stmts(&mut self) -> Result<Vec<Expr>, KslError> {
        let mut stmts = Vec::new();
        if self.at_simple(&TokenKind::RBracket) {
            return Ok(stmts);
        }
        loop {
            stmts.push(self.expr(Context::Statement)?);
            if self.continues(&TokenKind::RBracket) {
                continue;
            }
            return Ok(stmts);
        }
    }

    /// 参数列表, 返回类型与函数体: `[参数...] : 返回, 体`.
    fn params_ret_body(&mut self) -> Result<(Vec<Param>, TypeExpr, Expr), KslError> {
        self.expect(&TokenKind::LBracket)?;
        let mut params = Vec::new();
        if !self.at_simple(&TokenKind::RBracket) {
            loop {
                let name = self.ident()?;
                self.expect(&TokenKind::Colon)?;
                let ty = self.type_expr()?;
                params.push(Param { name, ty });
                if self.continues(&TokenKind::RBracket) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBracket)?;
        self.expect(&TokenKind::Colon)?;
        let ret = self.type_expr()?;
        self.comma()?;
        let body = self.expr(Context::Expression)?;
        Ok((params, ret, body))
    }

    /// 匿名函数 `@Fn[[x : i64, ...] : ret, 体]`.
    fn form_fn(&mut self) -> Result<Special, KslError> {
        let (params, ret, body) = self.params_ret_body()?;
        Ok(Special::Fn {
            params,
            ret,
            body: Box::new(body),
        })
    }

    /// `@Def[名字, [x : i64, ...] : ret, 体]`.
    fn form_def(&mut self) -> Result<Special, KslError> {
        Ok(Special::Def(self.def_body()?))
    }

    fn def_body(&mut self) -> Result<Def, KslError> {
        let name = self.ident()?;
        self.comma()?;
        let (params, ret, body) = self.params_ret_body()?;
        Ok(Def {
            name,
            params,
            ret,
            body: Box::new(body),
        })
    }

    fn form_spawn(&mut self, detached: bool) -> Result<Special, KslError> {
        let count = self.expr(Context::Expression)?;
        self.comma()?;
        let handler = self.expr(Context::Expression)?;
        // handler 之后每个实参都以逗号引导; 尾逗号由外层闭符号检查拒绝.
        let mut args = Vec::new();
        while self.continues(&TokenKind::RBracket) {
            args.push(self.expr(Context::Expression)?);
        }
        Ok(Special::Spawn {
            count: Box::new(count),
            handler: Box::new(handler),
            args,
            detached,
        })
    }

    fn form_send(&mut self) -> Result<Special, KslError> {
        let pid = self.expr(Context::Expression)?;
        self.comma()?;
        let index = self.expr(Context::Expression)?;
        self.comma()?;
        let message = self.expr(Context::Expression)?;
        Ok(Special::Send {
            pid: Box::new(pid),
            index: Box::new(index),
            message: Box::new(message),
        })
    }

    fn form_receive(&mut self) -> Result<Special, KslError> {
        let index = self.expr(Context::Expression)?;
        self.comma()?;
        let timeout = self.expr(Context::Expression)?;
        Ok(Special::Receive {
            index: Box::new(index),
            timeout: Box::new(timeout),
        })
    }

    fn form_join(&mut self) -> Result<Special, KslError> {
        let pid = self.expr(Context::Expression)?;
        Ok(Special::Join { pid: Box::new(pid) })
    }

    fn form_kill(&mut self) -> Result<Special, KslError> {
        let pid = self.expr(Context::Expression)?;
        Ok(Special::Kill { pid: Box::new(pid) })
    }

    fn form_monitor(&mut self) -> Result<Special, KslError> {
        let pid = self.expr(Context::Expression)?;
        Ok(Special::Monitor { pid: Box::new(pid) })
    }

    fn form_unmonitor(&mut self) -> Result<Special, KslError> {
        let id = self.expr(Context::Expression)?;
        Ok(Special::Unmonitor { id: Box::new(id) })
    }

    fn form_pid_serial(&mut self) -> Result<Special, KslError> {
        let pid = self.expr(Context::Expression)?;
        Ok(Special::PidSerial { pid: Box::new(pid) })
    }

    fn form_set_timer(&mut self) -> Result<Special, KslError> {
        let pid = self.expr(Context::Expression)?;
        self.comma()?;
        let index = self.expr(Context::Expression)?;
        self.comma()?;
        let message = self.expr(Context::Expression)?;
        self.comma()?;
        let millis = self.expr(Context::Expression)?;
        Ok(Special::SetTimer {
            pid: Box::new(pid),
            index: Box::new(index),
            message: Box::new(message),
            millis: Box::new(millis),
        })
    }

    fn form_cancel_timer(&mut self) -> Result<Special, KslError> {
        let id = self.expr(Context::Expression)?;
        Ok(Special::CancelTimer { id: Box::new(id) })
    }

    fn form_halt(&mut self) -> Result<Special, KslError> {
        if !self.at_simple(&TokenKind::RBracket) {
            return Err(KslError::new(ErrorKind::UnexpectedToken, self.peek_span()));
        }
        Ok(Special::Halt)
    }

    fn form_sleep(&mut self) -> Result<Special, KslError> {
        let millis = self.expr(Context::Expression)?;
        Ok(Special::Sleep {
            millis: Box::new(millis),
        })
    }

    /// `@Fix[@Def[...], ...]`: 仅接受具名函数定义.
    fn form_fix(&mut self) -> Result<Special, KslError> {
        let mut defs = Vec::new();
        if self.at_simple(&TokenKind::RBracket) {
            return Ok(Special::Fix(defs));
        }
        loop {
            self.expect(&TokenKind::At)?;
            let name = self.ident()?;
            if name.node != keywords().def_ {
                return Err(KslError::new(ErrorKind::ExpectedDef, name.span));
            }
            self.expect(&TokenKind::LBracket)?;
            defs.push(self.def_body()?);
            self.expect(&TokenKind::RBracket)?;
            if self.continues(&TokenKind::RBracket) {
                continue;
            }
            break;
        }
        Ok(Special::Fix(defs))
    }

    /// `@Export[名字, ..., ^类型, ...]`.
    fn form_export(&mut self) -> Result<Special, KslError> {
        let mut items = Vec::new();
        if !self.at_simple(&TokenKind::RBracket) {
            loop {
                if self.eat(&TokenKind::Caret) {
                    items.push(crate::ast::ExportItem::Type(self.ident()?));
                } else {
                    items.push(crate::ast::ExportItem::Value(self.ident()?));
                }
                if self.continues(&TokenKind::RBracket) {
                    continue;
                }
                break;
            }
        }
        Ok(Special::Export { items })
    }

    fn form_use(&mut self) -> Result<Special, KslError> {
        self.expect(&TokenKind::LBrace)?;
        let mut items = Vec::new();
        if !self.at_simple(&TokenKind::RBrace) {
            loop {
                items.push(self.use_item()?);
                if self.continues(&TokenKind::RBrace) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        self.comma()?;
        let path = self.string_literal()?;
        Ok(Special::Use { items, path })
    }

    fn use_item(&mut self) -> Result<crate::ast::UseItem, KslError> {
        if self.eat(&TokenKind::Caret) {
            if self.eat(&TokenKind::LParen) {
                let from = self.ident()?;
                self.comma()?;
                let to = self.ident()?;
                self.expect(&TokenKind::RParen)?;
                return Ok(crate::ast::UseItem::TypeRename { from, to });
            }
            return Ok(crate::ast::UseItem::Type(self.ident()?));
        }
        if self.eat(&TokenKind::LParen) {
            let from = self.ident()?;
            self.comma()?;
            let to = self.ident()?;
            self.expect(&TokenKind::RParen)?;
            return Ok(crate::ast::UseItem::Rename { from, to });
        }
        Ok(crate::ast::UseItem::Value(self.ident()?))
    }

    fn string_literal(&mut self) -> Result<String, KslError> {
        match self.bump() {
            Token {
                kind: TokenKind::Str(text) | TokenKind::RawStr(text),
                ..
            } => Ok(text.clone()),
            token => Err(KslError::new(ErrorKind::UnexpectedToken, token.span)),
        }
    }

    fn form_data(&mut self) -> Result<Special, KslError> {
        let name = self.ident()?;
        self.comma()?;
        self.expect(&TokenKind::LBrace)?;
        let mut variants = Vec::new();
        if !self.at_simple(&TokenKind::RBrace) {
            loop {
                let variant_name = self.ident()?;
                self.expect(&TokenKind::LBracket)?;
                let mut fields = Vec::new();
                if !self.at_simple(&TokenKind::RBracket) {
                    loop {
                        fields.push(self.type_expr()?);
                        if self.continues(&TokenKind::RBracket) {
                            continue;
                        }
                        break;
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                variants.push(DataVariant {
                    name: variant_name,
                    fields,
                });
                if self.continues(&TokenKind::RBrace) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Special::Data { name, variants })
    }

    /// `@Match[值, {(模式, 结果), ...}]`.
    fn form_match(&mut self) -> Result<Special, KslError> {
        let value = self.expr(Context::Expression)?;
        self.comma()?;
        self.expect(&TokenKind::LBrace)?;
        let mut arms = Vec::new();
        if !self.at_simple(&TokenKind::RBrace) {
            loop {
                self.expect(&TokenKind::LParen)?;
                let pattern = self.pattern()?;
                self.comma()?;
                let body = self.expr(Context::Expression)?;
                self.expect(&TokenKind::RParen)?;
                arms.push(MatchArm { pattern, body });
                if self.continues(&TokenKind::RBrace) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Special::Match {
            value: Box::new(value),
            arms,
        })
    }
}
