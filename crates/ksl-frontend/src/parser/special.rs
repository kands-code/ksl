//! 特殊形式与语句级语法.

use ksl_base::source::Span;
use ksl_base::symbol::{Symbol, resolve};

use super::{Context, Parser};
use crate::ast::{
    BindingTarget, DataVariant, Def, Expr, ExprKind, FormKind, LinkDecl, LinkValue, MatchArm,
    Param, Special, TypeExpr,
};
use crate::error::{ErrorKind, KslError};
use crate::lexer::{Token, TokenKind};

impl Parser<'_> {
    pub(super) fn special_form(&mut self) -> Result<ExprKind, KslError> {
        self.bump();
        let name = self.ident()?;
        self.expect(&TokenKind::LBracket)?;
        let form = self.dispatch_special(name.node, name.span)?;
        self.expect(&TokenKind::RBracket)?;
        Ok(ExprKind::Special(form))
    }

    /// 特殊形式分派: 先按形式表, 再按操作元数据.
    fn dispatch_special(&mut self, node: Symbol, span: Span) -> Result<Special, KslError> {
        if let Some(kind) = FormKind::from_name(resolve(node).as_ref()) {
            return self.form_of(kind, span);
        }
        // 形式闭括号由调用方统一消费, 仅解析参数.
        if let Some(op) = crate::ast::option_op_kind(resolve(node).as_ref()) {
            return Ok(Special::OptionOp {
                op,
                args: self.comma_exprs()?,
            });
        }
        if let Some(op) = crate::ast::list_op_kind(resolve(node).as_ref()) {
            return Ok(Special::ListOp {
                op,
                args: self.comma_exprs()?,
            });
        }
        if let Some(op) = crate::ast::pair_op_kind(resolve(node).as_ref()) {
            return Ok(Special::PairOp {
                op,
                args: self.comma_exprs()?,
            });
        }
        if let Some(op) = crate::ast::dict_op_kind(resolve(node).as_ref()) {
            return Ok(Special::DictOp {
                op,
                args: self.comma_exprs()?,
            });
        }
        if let Some(op) = crate::ast::set_op_kind(resolve(node).as_ref()) {
            return Ok(Special::SetOp {
                op,
                args: self.comma_exprs()?,
            });
        }
        if let Some(op) = crate::ast::registry_op_kind(resolve(node).as_ref()) {
            return Ok(Special::Registry {
                op,
                args: self.comma_exprs()?,
            });
        }
        Err(KslError::new(ErrorKind::UnknownSpecialForm, span))
    }

    /// 形式体: 上下文限制先行判定, 再按种类解析.
    fn form_of(&mut self, kind: FormKind, span: Span) -> Result<Special, KslError> {
        self.check_form_context(kind, span)?;
        let form = match kind {
            FormKind::Let => self.form_let()?,
            FormKind::Block => Special::Block(self.block_stmts()?),
            FormKind::If => self.form_if()?,
            FormKind::Which => self.form_which()?,
            FormKind::Fn => self.form_fn()?,
            FormKind::Def => self.form_def()?,
            FormKind::Exit => self.form_exit()?,
            FormKind::Handler => self.form_handler()?,
            FormKind::Main => self.form_main()?,
            FormKind::Continue => self.form_continue()?,
            FormKind::Spawn => self.form_spawn(false)?,
            FormKind::SpawnDetached => self.form_spawn(true)?,
            FormKind::Send => self.form_send()?,
            FormKind::Receive => self.form_receive()?,
            FormKind::Join => self.form_join()?,
            FormKind::Kill => self.form_kill()?,
            FormKind::Monitor => self.form_monitor()?,
            FormKind::Unmonitor => self.form_unmonitor()?,
            FormKind::PidSerial => self.form_pid_serial()?,
            FormKind::SetTimer => self.form_set_timer()?,
            FormKind::CancelTimer => self.form_cancel_timer()?,
            FormKind::Halt => self.form_halt()?,
            FormKind::Sleep => self.form_sleep()?,
            FormKind::Fix => self.form_fix()?,
            FormKind::Data => self.form_data()?,
            FormKind::Export => self.form_export()?,
            FormKind::Use => self.form_use()?,
            FormKind::Link => self.form_link()?,
            FormKind::Match => self.form_match()?,
            FormKind::Dict => Special::Dict {
                entries: self.comma_exprs()?,
            },
            FormKind::Set => Special::Set {
                items: self.comma_exprs()?,
            },
            FormKind::Assert => self.form_assert()?,
            FormKind::Panic => self.form_panic()?,
            FormKind::Todo => self.form_todo()?,
            FormKind::Unreachable => self.form_unreachable()?,
        };
        Ok(form)
    }

    /// 上下文限制: 定义形式只在顶层或语句位置, 模块形式只在顶层.
    fn check_form_context(&self, kind: FormKind, span: Span) -> Result<(), KslError> {
        match kind {
            FormKind::Let | FormKind::Def | FormKind::Fix => {
                if !matches!(self.context, Context::Top | Context::Statement) {
                    return Err(KslError::new(ErrorKind::DefinitionNotAllowed, span));
                }
            },
            FormKind::Data => {
                if self.context != Context::Top {
                    return Err(KslError::new(ErrorKind::DataNotTopLevel, span));
                }
            },
            FormKind::Export | FormKind::Use | FormKind::Link | FormKind::Main => {
                if self.context != Context::Top {
                    return Err(KslError::new(ErrorKind::ModuleFormNotTopLevel, span));
                }
            },
            FormKind::Exit if self.context != Context::Top && !self.in_main => {
                return Err(KslError::new(ErrorKind::ExitNotTopLevel, span));
            },
            _ => {},
        }
        Ok(())
    }

    /// `@If[condition, then, else]`.
    fn form_if(&mut self) -> Result<Special, KslError> {
        let cond = self.expr(Context::Expression)?;
        self.comma()?;
        let then = self.expr(Context::Expression)?;
        self.comma()?;
        let els = self.expr(Context::Expression)?;
        Ok(Special::If {
            cond: Box::new(cond),
            then: Box::new(then),
            els: Box::new(els),
        })
    }

    /// `@Exit[code]`.
    fn form_exit(&mut self) -> Result<Special, KslError> {
        let code = self.expr(Context::Expression)?;
        Ok(Special::Exit {
            code: Box::new(code),
        })
    }

    /// `@Handler[[参数] : 返回, 体]`.
    fn form_handler(&mut self) -> Result<Special, KslError> {
        let saved = self.in_main;
        self.in_main = false;
        let parsed = self.params_ret_body();
        self.in_main = saved;
        let (params, ret, body) = parsed?;
        Ok(Special::Handler {
            params,
            ret,
            body: Box::new(body),
        })
    }

    /// `@Main[[参数] : 返回, 体]`.
    fn form_main(&mut self) -> Result<Special, KslError> {
        let saved = self.in_main;
        self.in_main = true;
        let parsed = self.params_ret_body();
        self.in_main = saved;
        let (params, ret, body) = parsed?;
        Ok(Special::Main {
            params,
            ret,
            body: Box::new(body),
        })
    }

    /// `@Continue[状态参数...]`.
    fn form_continue(&mut self) -> Result<Special, KslError> {
        Ok(Special::Continue {
            args: self.comma_exprs()?,
        })
    }

    /// `@Assert[condition, message]`.
    fn form_assert(&mut self) -> Result<Special, KslError> {
        let cond = self.expr(Context::Expression)?;
        self.comma()?;
        let message = self.expr(Context::Expression)?;
        Ok(Special::Assert {
            cond: Box::new(cond),
            message: Box::new(message),
        })
    }

    /// `@Panic[message]`.
    fn form_panic(&mut self) -> Result<Special, KslError> {
        Ok(Special::Panic {
            message: Box::new(self.expr(Context::Expression)?),
        })
    }

    /// `@Todo[message]`.
    fn form_todo(&mut self) -> Result<Special, KslError> {
        Ok(Special::Todo {
            message: Box::new(self.expr(Context::Expression)?),
        })
    }

    /// `@Unreachable[message]`.
    fn form_unreachable(&mut self) -> Result<Special, KslError> {
        Ok(Special::Unreachable {
            message: Box::new(self.expr(Context::Expression)?),
        })
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
        let saved = self.in_main;
        self.in_main = false;
        let parsed = self.params_ret_body();
        self.in_main = saved;
        let (params, ret, body) = parsed?;
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
        let saved = self.in_main;
        self.in_main = false;
        let parsed = self.params_ret_body();
        self.in_main = saved;
        let (params, ret, body) = parsed?;
        Ok(Def {
            name,
            params,
            ret,
            body: Box::new(body),
        })
    }

    fn form_spawn(&mut self, detached: bool) -> Result<Special, KslError> {
        let handler = self.expr(Context::Expression)?;
        // handler 之后每个实参都以逗号引导; 尾逗号由外层闭符号检查拒绝.
        let mut args = Vec::new();
        while self.continues(&TokenKind::RBracket) {
            args.push(self.expr(Context::Expression)?);
        }
        Ok(Special::Spawn {
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
            if FormKind::from_name(resolve(name.node).as_ref()) != Some(FormKind::Def) {
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

    /// `@Link["插件名", {^类型, ...}, {名字 : 类型, ...}]`.
    fn form_link(&mut self) -> Result<Special, KslError> {
        let plugin = self.string_literal()?;
        self.comma()?;
        self.expect(&TokenKind::LBrace)?;
        let mut types = Vec::new();
        if !self.at_simple(&TokenKind::RBrace) {
            loop {
                self.expect(&TokenKind::Caret)?;
                types.push(self.ident()?);
                if self.continues(&TokenKind::RBrace) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        self.comma()?;
        self.expect(&TokenKind::LBrace)?;
        let mut values = Vec::new();
        if !self.at_simple(&TokenKind::RBrace) {
            loop {
                let name = self.ident()?;
                self.expect(&TokenKind::Colon)?;
                let ty = self.type_expr()?;
                values.push(LinkValue { name, ty });
                if self.continues(&TokenKind::RBrace) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(Special::Link(LinkDecl {
            plugin,
            types,
            values,
        }))
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

    /// `@Which[{(条件, 结果), ...}, fallback]`.
    fn form_which(&mut self) -> Result<Special, KslError> {
        self.expect(&TokenKind::LBrace)?;
        let mut branches = Vec::new();
        if !self.at_simple(&TokenKind::RBrace) {
            loop {
                self.expect(&TokenKind::LParen)?;
                let cond = self.expr(Context::Expression)?;
                self.comma()?;
                let value = self.expr(Context::Expression)?;
                self.expect(&TokenKind::RParen)?;
                branches.push((cond, value));
                if self.continues(&TokenKind::RBrace) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBrace)?;
        self.comma()?;
        let fallback = self.expr(Context::Expression)?;
        Ok(Special::Which {
            branches,
            fallback: Box::new(fallback),
        })
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
