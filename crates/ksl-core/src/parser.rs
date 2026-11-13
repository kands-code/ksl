//! 语法分析.
//!
//! 递归下降解析器, 把记号序列组织为 AST; 各 `@` 特殊形式的参数形态
//! 在此逐一定义, 未知形式名在解析层直接报错.

use crate::ast::{Expr, ExprKind, FStrAstPart, Head, NodeId, Program, Spanned};
use crate::error::{ErrorKind, KslError};
use crate::lexer::{Token, TokenKind, lex};
use crate::source::{SourceId, Span};
use crate::symbol::{Symbol, keywords};

/// 词法并解析一份源代码; 两个阶段统一返回语法错误形态.
///
/// # Errors
///
/// 词法或语法任一阶段失败时返回对应错误.
pub fn parse_text(text: &str, source: SourceId) -> Result<Program, KslError> {
    let tokens = lex(text, source)?;
    parse(&tokens)
}

/// 把记号序列解析为顶层程序.
///
/// 语句以分号结尾; 最后一条表达式可省略分号, 作为尾表达式承载程序值.
///
/// # Errors
///
/// 记号序列不符合语法时返回首个错误.
pub fn parse(tokens: &[Token]) -> Result<Program, KslError> {
    let mut parser = Parser {
        tokens,
        pos: 0,
        context: Context::Top,
        in_main: false,
        source: tokens.first().map_or(SourceId::default(), |t| t.span.source),
        next_node: 0,
    };
    let mut stmts = Vec::new();
    let mut tail = None;
    while !parser.at_simple(&TokenKind::Eof) {
        let expr = parser.expr(Context::Top)?;
        if parser.at_simple(&TokenKind::Semi) {
            parser.bump();
            stmts.push(expr);
        } else if parser.at_simple(&TokenKind::Eof) {
            // 省略分号的尾位置; 其后只允许文件结束.
            tail = Some(expr);
            break;
        } else {
            // 其后既非分号也非文件结束: 沿用 expect 的分号预期错误.
            parser.expect(&TokenKind::Semi)?;
            unreachable!("非分号位置已报错")
        }
    }
    Ok(Program { stmts, tail })
}

/// 解析上下文: 决定哪些定义形式可以出现.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    /// 顶层: `@Data` 与 `@Def` 均可.
    Top,
    /// `@Block` 的语句位置: 仅 `@Def`.
    Statement,
    /// 普通表达式位置: 不允许定义.
    Expression,
}

struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
    source: SourceId,
    /// 当前解析上下文; 进入嵌套表达式时切换, 返回时恢复.
    context: Context,
    /// 是否正在解析 `@Main` 体; `@Exit` 只在此处允许出现在非顶层位置.
    in_main: bool,
    /// 下一个节点编号; 每次解析从 0 开始, 与 `SourceId` 共同构成唯一键.
    next_node: u32,
}

impl<'t> Parser<'t> {
    // 记号游标基础操作

    fn next_node(&mut self) -> NodeId {
        let id = NodeId::from_index(self.next_node);
        self.next_node += 1;
        id
    }

    /// 为节点分配身份并包装区间.
    fn spanned<T>(&mut self, node: T, span: Span) -> Spanned<T> {
        let id = self.next_node();
        Spanned::new(node, span, id)
    }

    fn peek(&self) -> &'t TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn at_simple(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn bump(&mut self) -> &'t Token {
        let token = &self.tokens[self.pos];
        if !matches!(token.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        token
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at_simple(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<Span, KslError> {
        if self.at_simple(kind) {
            Ok(self.bump().span)
        } else {
            Err(KslError::new(ErrorKind::UnexpectedToken, self.peek_span()))
        }
    }

    fn ident(&mut self) -> Result<Spanned<Symbol>, KslError> {
        match self.bump() {
            Token {
                kind: TokenKind::Ident(name),
                span,
            } => Ok(self.spanned(*name, *span)),
            token => Err(KslError::new(ErrorKind::UnexpectedToken, token.span)),
        }
    }

    fn comma(&mut self) -> Result<(), KslError> {
        self.expect(&TokenKind::Comma).map(|_| ())
    }

    /// 列表继续判定: 消费逗号后若未到闭符号则继续.
    ///
    /// 尾逗号不被接受: 若逗号后直接是闭符号, 保留逗号不消费, 由调用方的
    /// 闭符号检查报告错误.
    fn continues(&mut self, closer: &TokenKind) -> bool {
        if !self.at_simple(&TokenKind::Comma) {
            return false;
        }
        if self.tokens.get(self.pos + 1).map(|token| &token.kind) == Some(closer) {
            return false;
        }
        self.bump();
        true
    }

    /// 逗号分隔的表达式序列, 直到右方括号前; 不消费右方括号.
    ///
    /// 普通调用与各操作类特殊形式共用同一套尾逗号规则.
    fn comma_exprs(&mut self) -> Result<Vec<Expr>, KslError> {
        let mut args = Vec::new();
        if self.at_simple(&TokenKind::RBracket) {
            return Ok(args);
        }
        loop {
            args.push(self.expr(Context::Expression)?);
            if self.continues(&TokenKind::RBracket) {
                continue;
            }
            return Ok(args);
        }
    }

    /// 逗号分隔的表达式序列, 消费右方括号并返回闭符号区间.
    fn bracket_exprs(&mut self) -> Result<(Vec<Expr>, Span), KslError> {
        let args = self.comma_exprs()?;
        let close = self.expect(&TokenKind::RBracket)?;
        Ok((args, close))
    }

    // 表达式

    fn expr(&mut self, context: Context) -> Result<Expr, KslError> {
        let start = self.peek_span().start;
        let saved = self.context;
        self.context = context;
        let result = self.expr_inner();
        self.context = saved;
        let node = result?;
        let end = self.tokens[self.pos.saturating_sub(1)].span.end;
        let mut expr = self.spanned(node, Span::new(self.source, start, end));
        // 后缀调用: 任意表达式后可跟任意多 `[参数]`; 与名字调用共用
        // 同一套参数语法. 是否可调用由检查器判定, 语法层不做限制.
        while self.at_simple(&TokenKind::LBracket) {
            self.bump();
            let (args, close) = self.bracket_exprs()?;
            expr = self.spanned(
                ExprKind::Apply {
                    callee: Box::new(expr),
                    args,
                },
                Span::new(self.source, start, close.end),
            );
        }
        Ok(expr)
    }

    fn expr_inner(&mut self) -> Result<ExprKind, KslError> {
        match self.peek() {
            TokenKind::I64(v) => {
                let v = *v;
                self.bump();
                Ok(ExprKind::I64(v))
            },
            TokenKind::U8(v) => {
                let v = *v;
                self.bump();
                Ok(ExprKind::U8(v))
            },
            TokenKind::U64(v) => {
                let v = *v;
                self.bump();
                Ok(ExprKind::U64(v))
            },
            TokenKind::F64(v) => {
                let v = *v;
                self.bump();
                Ok(ExprKind::F64(v))
            },
            TokenKind::Char(c) => {
                let c = *c;
                self.bump();
                Ok(ExprKind::Char(c))
            },
            TokenKind::Str(text) => {
                let text = text.clone();
                self.bump();
                Ok(ExprKind::Str(text))
            },
            TokenKind::RawStr(text) => {
                // 原始字符串与普通字符串同类型, 词法层已保证内容无需再处理.
                let text = text.clone();
                self.bump();
                Ok(ExprKind::Str(text))
            },
            TokenKind::FStr(parts) => self.fstring(parts),
            TokenKind::Ident(_) => self.ident_expr(),
            TokenKind::LBrace => self.list_literal(),
            TokenKind::LParen => self.pair(),
            TokenKind::At => self.special_form(),
            TokenKind::Eof => Err(KslError::new(ErrorKind::Unterminated, self.peek_span())),
            _ => Err(KslError::new(ErrorKind::UnexpectedToken, self.peek_span())),
        }
    }

    /// 标识符开头: 变量引用, 普通调用, 或受限名字构造器调用.
    fn ident_expr(&mut self) -> Result<ExprKind, KslError> {
        let head = self.head()?;
        if head.qualifier.is_some() && !self.at_simple(&TokenKind::LBracket) {
            // 受限名字必须构成调用, 如 `Shape/Circle[3.2]`.
            return Err(KslError::new(ErrorKind::UnexpectedToken, self.peek_span()));
        }
        if self.at_simple(&TokenKind::LBracket) {
            self.bump();
            let (args, _) = self.bracket_exprs()?;
            return Ok(ExprKind::Call { head, args });
        }
        let node = head.name.node;
        if head.qualifier.is_none() {
            let kw = keywords();
            if node == kw.true_ {
                return Ok(ExprKind::Bool(true));
            }
            if node == kw.false_ {
                return Ok(ExprKind::Bool(false));
            }
            if node == kw.nil_ {
                return Ok(ExprKind::Nil);
            }
        }
        Ok(ExprKind::Ident(node))
    }

    /// 调用头部: `name` 或 `Type/name`.
    fn head(&mut self) -> Result<Head, KslError> {
        let first = self.ident()?;
        if self.at_simple(&TokenKind::Slash) {
            self.bump();
            let name = self.ident()?;
            Ok(Head {
                qualifier: Some(first),
                name,
            })
        } else {
            Ok(Head {
                qualifier: None,
                name: first,
            })
        }
    }

    /// 列表字面量 `{a, b, c}`.
    fn list_literal(&mut self) -> Result<ExprKind, KslError> {
        self.bump();
        let mut items = Vec::new();
        if self.eat(&TokenKind::RBrace) {
            return Ok(ExprKind::List(items));
        }
        loop {
            items.push(self.expr(Context::Expression)?);
            if self.continues(&TokenKind::RBrace) {
                continue;
            }
            self.expect(&TokenKind::RBrace)?;
            return Ok(ExprKind::List(items));
        }
    }

    /// 配对记号 `(a, b)`.
    fn pair(&mut self) -> Result<ExprKind, KslError> {
        self.bump();
        let first = self.expr(Context::Expression)?;
        self.comma()?;
        let second = self.expr(Context::Expression)?;
        self.expect(&TokenKind::RParen)?;
        Ok(ExprKind::Pair(Box::new(first), Box::new(second)))
    }

    /// 格式化字符串; 插值只能是变量或字面值, 此处校验形态.
    fn fstring(&mut self, parts: &[crate::lexer::FStrPart]) -> Result<ExprKind, KslError> {
        let token_span = self.peek_span();
        let mut ast_parts = Vec::new();
        for part in parts {
            match part {
                crate::lexer::FStrPart::Text(text) => {
                    ast_parts.push(FStrAstPart::Text(text.clone()));
                },
                crate::lexer::FStrPart::Interp { expr, spec } => {
                    let ast_expr = self.fstr_interp_expr(expr, token_span)?;
                    ast_parts.push(FStrAstPart::Interp {
                        expr: Box::new(ast_expr),
                        spec: spec.clone(),
                    });
                },
            }
        }
        self.bump();
        Ok(ExprKind::FStr(ast_parts))
    }

    /// 校验插值原文恰好是一个变量或一个字面值.
    fn fstr_interp_expr(&mut self, text: &str, span: Span) -> Result<Expr, KslError> {
        let tokens = lex(text, span.source)
            .map_err(|_| KslError::new(ErrorKind::InvalidFStrInterp, span))?;
        let mut meaningful = tokens.iter().filter(|t| !matches!(t.kind, TokenKind::Eof));
        let (Some(token), None) = (meaningful.next(), meaningful.next()) else {
            return Err(KslError::new(ErrorKind::InvalidFStrInterp, span));
        };
        let expr = match &token.kind {
            TokenKind::Ident(sym) if *sym == keywords().true_ => ExprKind::Bool(true),
            TokenKind::Ident(sym) if *sym == keywords().false_ => ExprKind::Bool(false),
            TokenKind::Ident(sym) if *sym == keywords().nil_ => ExprKind::Nil,
            TokenKind::Ident(sym) => ExprKind::Ident(*sym),
            TokenKind::I64(v) => ExprKind::I64(*v),
            TokenKind::U8(v) => ExprKind::U8(*v),
            TokenKind::U64(v) => ExprKind::U64(*v),
            TokenKind::F64(v) => ExprKind::F64(*v),
            TokenKind::Char(c) => ExprKind::Char(*c),
            TokenKind::Str(text) | TokenKind::RawStr(text) => ExprKind::Str(text.clone()),
            _ => return Err(KslError::new(ErrorKind::InvalidFStrInterp, span)),
        };
        Ok(self.spanned(expr, span))
    }
}

mod special;
mod types;

#[cfg(test)]
mod tests;
