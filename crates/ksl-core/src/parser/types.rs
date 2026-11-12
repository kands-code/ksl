//! 模式与类型表达式.

use super::Parser;
use crate::ast::{Pattern, PatternField, TypeExpr};
use crate::error::{ErrorKind, KslError};
use crate::lexer::{Token, TokenKind};
use crate::symbol::keywords;

impl Parser<'_> {
    /// 构造器模式: `[类型/]构造器[字段绑定, ...]`; 省略类型时以 `/` 开头.
    pub(super) fn pattern(&mut self) -> Result<Pattern, KslError> {
        // 通配模式: 匹配任意变体, 不绑定字段.
        if self.at_simple(&TokenKind::Discard) {
            self.bump();
            return Ok(Pattern {
                qualifier: None,
                variant: None,
                fields: Vec::new(),
            });
        }
        let (qualifier, variant) = if self.at_simple(&TokenKind::Slash) {
            self.bump();
            (None, self.ident()?)
        } else {
            let qualifier = self.ident()?;
            self.expect(&TokenKind::Slash)?;
            (Some(qualifier), self.ident()?)
        };
        self.expect(&TokenKind::LBracket)?;
        let mut fields = Vec::new();
        if !self.at_simple(&TokenKind::RBracket) {
            loop {
                let field = match self.bump() {
                    Token {
                        kind: TokenKind::Ident(text),
                        span,
                    } => PatternField::Bind(self.spanned(*text, *span)),
                    Token {
                        kind: TokenKind::Discard,
                        ..
                    } => PatternField::Wildcard,
                    token => {
                        return Err(KslError::new(ErrorKind::UnexpectedToken, token.span));
                    },
                };
                fields.push(field);
                if self.continues(&TokenKind::RBracket) {
                    continue;
                }
                break;
            }
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(Pattern {
            qualifier,
            variant: Some(variant),
            fields,
        })
    }

    // 类型表达式

    /// 类型表达式: 具名类型, 内置参数化容器, 或函数类型.
    pub(super) fn type_expr(&mut self) -> Result<TypeExpr, KslError> {
        let name = self.ident()?;
        if name.node == keywords().fn_ && self.at_simple(&TokenKind::LBracket) {
            // 函数类型与 `@Fn` 同构: `Fn[[参数, ...] : 返回]`.
            self.bump();
            self.expect(&TokenKind::LBracket)?;
            let params = self.type_exprs()?;
            self.expect(&TokenKind::RBracket)?;
            self.expect(&TokenKind::Colon)?;
            let ret = self.type_expr()?;
            self.expect(&TokenKind::RBracket)?;
            return Ok(TypeExpr::Fn {
                id: self.next_node(),
                span: name.span,
                params,
                ret: Box::new(ret),
            });
        }
        if self.at_simple(&TokenKind::LBracket) {
            self.bump();
            let args = self.type_exprs()?;
            self.expect(&TokenKind::RBracket)?;
            return Ok(TypeExpr::Generic { name, args });
        }
        Ok(TypeExpr::Named(name))
    }

    /// 逗号分隔的类型表达式序列, 直到右方括号前; 不消费右方括号.
    fn type_exprs(&mut self) -> Result<Vec<TypeExpr>, KslError> {
        let mut types = Vec::new();
        if self.at_simple(&TokenKind::RBracket) {
            return Ok(types);
        }
        loop {
            types.push(self.type_expr()?);
            if self.continues(&TokenKind::RBracket) {
                continue;
            }
            return Ok(types);
        }
    }
}
