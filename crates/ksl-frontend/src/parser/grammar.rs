//! 从头实现的 winnow parser.
//!
//! 直接消费 lexer 的 token 流, 产出 `ast::Program`.
//! 特殊形式与位置约束分离, 位置由 `validate` 检查.

use ksl_base::name::Name;
use ksl_base::source::{SourceId, Span};
use ksl_diagnostics::error::{ErrorKind, KslError};
use winnow::Parser;
use winnow::error::{AddContext, ErrMode, ParserError};
use winnow::stream::{Location, Stateful, Stream, TokenSlice};

use crate::ast::{
    BindingTarget, DataVariant, Def, ExportItem, Expr, ExprKind, FStrAstPart, FormKind, Head,
    LinkDecl, LinkValue, MatchArm, Param, Pattern, PatternField, Program, Spanned, Special,
    TypeExpr, UseItem,
};
use crate::lexer::{Token, TokenKind};

/// 解析状态: 来源与当前嵌套深度.
#[derive(Debug, Clone, Copy)]
struct ParseState {
    source: SourceId,
    depth: usize,
}

/// 带来源标识的 token 流.
type Input<'i> = Stateful<TokenSlice<'i, Token>, ParseState>;
type PResult<T> = Result<T, ErrMode<ParseFailure>>;

/// 表达式与类型的最大嵌套层数.
///
/// 取值低于 release 构建的栈上限, 并为 debug 构建留出余量.
const MAX_NESTING: usize = 64;

/// 携带 ksl 错误的解析失败.
#[derive(Debug)]
pub struct ParseFailure(pub Box<KslError>);

impl<'i> ParserError<Input<'i>> for ParseFailure {
    type Inner = KslError;

    fn from_input(input: &Input<'i>) -> Self {
        let source = input.state.source;
        let start = input.current_token_start();
        let end = input.peek_token().map_or(start, |token| token.span.end());
        Self(Box::new(KslError::new(
            ErrorKind::UnexpectedToken,
            Span::new(source, start, end),
        )))
    }

    fn into_inner(self) -> Result<KslError, Self> {
        Ok(*self.0)
    }
}

impl<'i, C> AddContext<Input<'i>, C> for ParseFailure {
    fn add_context(
        self,
        _input: &Input<'i>,
        _token_start: &<Input<'i> as Stream>::Checkpoint,
        _context: C,
    ) -> Self {
        self
    }
}

impl Location for Token {
    fn previous_token_end(&self) -> usize {
        self.span.end()
    }

    fn current_token_start(&self) -> usize {
        self.span.start()
    }
}

/// 解析一段 token 流.
///
/// # Errors
///
/// 返回首个语法错误.
pub(crate) fn parse(tokens: &[Token], source: SourceId) -> Result<Program, KslError> {
    let mut input = Stateful {
        input: TokenSlice::new(tokens),
        state: ParseState { source, depth: 0 },
    };
    program.parse_next(&mut input).map_err(unwrap_failure)
}

fn program(input: &mut Input<'_>) -> PResult<Program> {
    let mut stmts = Vec::new();
    while !at(input, &TokenKind::Eof) {
        let expr = self::expr(input)?;
        if eat(input, &TokenKind::Semi) {
            stmts.push(expr);
        } else if at(input, &TokenKind::Eof) {
            return Ok(Program {
                stmts,
                tail: Some(expr),
            });
        } else {
            return Err(unexpected(input));
        }
    }
    Ok(Program { stmts, tail: None })
}

fn expr(input: &mut Input<'_>) -> PResult<Expr> {
    let start = current_start(input);
    let depth = enter_depth(input, span(input))?;
    let result = expr_body(input, start, depth);
    input.state.depth = depth - 1;
    result
}

/// 解析表达式主体; 每组后置调用增加一层嵌套.
fn expr_body(input: &mut Input<'_>, start: usize, depth: usize) -> PResult<Expr> {
    let node = expr_inner(input)?;
    let mut expr =
        Spanned::new(node, Span::new(input.state.source, start, previous_end(input)));
    let mut wraps = 0usize;
    while at(input, &TokenKind::LBracket) {
        wraps += 1;
        let level = depth.saturating_add(wraps);
        if level > MAX_NESTING {
            return Err(nesting_error(input));
        }
        input.state.depth = level;
        bump(input);
        let (args, close) = bracket_args(input)?;
        expr = Spanned::new(
            ExprKind::Apply {
                callee: Box::new(expr),
                args,
            },
            Span::new(input.state.source, start, close.end()),
        );
    }
    Ok(expr)
}

fn expr_inner(input: &mut Input<'_>) -> PResult<ExprKind> {
    match peek(input) {
        Some(TokenKind::I64(value)) => Ok(literal(input, ExprKind::I64(*value))),
        Some(TokenKind::I32(value)) => Ok(literal(input, ExprKind::I32(*value))),
        Some(TokenKind::U8(value)) => Ok(literal(input, ExprKind::U8(*value))),
        Some(TokenKind::U32(value)) => Ok(literal(input, ExprKind::U32(*value))),
        Some(TokenKind::U64(value)) => Ok(literal(input, ExprKind::U64(*value))),
        Some(TokenKind::F32(value)) => Ok(literal(input, ExprKind::F32(*value))),
        Some(TokenKind::F64(value)) => Ok(literal(input, ExprKind::F64(*value))),
        Some(TokenKind::Char(value)) => Ok(literal(input, ExprKind::Char(*value))),
        Some(TokenKind::Str(value) | TokenKind::RawStr(value)) => {
            Ok(literal(input, ExprKind::Str(value.clone())))
        },
        Some(TokenKind::FStr(parts)) => {
            let parts = parts.clone();
            bump(input);
            Ok(ExprKind::FStr(fstr_parts(&parts)?))
        },
        Some(TokenKind::Ident(name)) if *name == ksl_base::name::keywords().true_ => {
            Ok(literal(input, ExprKind::Bool(true)))
        },
        Some(TokenKind::Ident(name)) if *name == ksl_base::name::keywords().false_ => {
            Ok(literal(input, ExprKind::Bool(false)))
        },
        Some(TokenKind::Ident(name)) if *name == ksl_base::name::keywords().nil_ => {
            Ok(literal(input, ExprKind::Nil))
        },
        Some(TokenKind::Ident(_)) => ident_expr(input),
        Some(TokenKind::LBrace) => list_expr(input),
        Some(TokenKind::LParen) => pair_expr(input),
        Some(TokenKind::At) => special_expr(input),
        _ => Err(unexpected(input)),
    }
}

fn literal(input: &mut Input<'_>, node: ExprKind) -> ExprKind {
    bump(input);
    node
}

fn ident_expr(input: &mut Input<'_>) -> PResult<ExprKind> {
    let first = ident(input)?;
    let head = if eat(input, &TokenKind::Slash) {
        let name = ident(input)?;
        Head {
            qualifier: Some(first),
            name,
        }
    } else {
        Head {
            qualifier: None,
            name: first,
        }
    };
    if at(input, &TokenKind::LBracket) {
        bump(input);
        let (args, _) = bracket_args(input)?;
        Ok(ExprKind::Call { head, args })
    } else {
        Ok(ExprKind::Ident(head.name.node))
    }
}

fn list_expr(input: &mut Input<'_>) -> PResult<ExprKind> {
    expect(input, &TokenKind::LBrace)?;
    let items = comma_exprs(input, &TokenKind::RBrace)?;
    Ok(ExprKind::List(items))
}

fn pair_expr(input: &mut Input<'_>) -> PResult<ExprKind> {
    expect(input, &TokenKind::LParen)?;
    let first = self::expr(input)?;
    expect(input, &TokenKind::Comma)?;
    let second = self::expr(input)?;
    expect(input, &TokenKind::RParen)?;
    Ok(ExprKind::Pair(Box::new(first), Box::new(second)))
}

fn bracket_args(input: &mut Input<'_>) -> PResult<(Vec<Expr>, Span)> {
    let args = if at(input, &TokenKind::RBracket) {
        Vec::new()
    } else {
        let mut args = Vec::new();
        loop {
            args.push(self::expr(input)?);
            if eat(input, &TokenKind::Comma) {
                continue;
            }
            break;
        }
        args
    };
    let close = expect(input, &TokenKind::RBracket)?;
    Ok((args, close))
}

fn args_until(input: &mut Input<'_>, close: &TokenKind) -> PResult<Vec<Expr>> {
    let mut items = Vec::new();
    if at(input, close) {
        return Ok(items);
    }
    loop {
        items.push(self::expr(input)?);
        if eat(input, &TokenKind::Comma) {
            continue;
        }
        break;
    }
    Ok(items)
}

fn comma_exprs(input: &mut Input<'_>, close: &TokenKind) -> PResult<Vec<Expr>> {
    let items = args_until(input, close)?;
    expect(input, close)?;
    Ok(items)
}

fn fstr_parts(parts: &[crate::lexer::FStrPart]) -> PResult<Vec<FStrAstPart>> {
    let mut ast = Vec::new();
    for part in parts {
        match part {
            crate::lexer::FStrPart::Text(text) => ast.push(FStrAstPart::Text(text.clone())),
            crate::lexer::FStrPart::Interp { expr, spec, span } => {
                ast.push(FStrAstPart::Interp {
                    expr: Box::new(fstr_expr(expr, *span)?),
                    spec: spec.clone(),
                });
            },
        }
    }
    Ok(ast)
}

fn fstr_expr(text: &str, span: Span) -> PResult<Expr> {
    let tokens = crate::lexer::lex(text, span.source)
        .map_err(|_| fail(ErrorKind::InvalidFStrInterp, span))?;
    let mut meaningful = tokens.into_iter().filter(|token| token.kind != TokenKind::Eof);
    let Some(token) = meaningful.next() else {
        return Err(fail(ErrorKind::InvalidFStrInterp, span));
    };
    if meaningful.next().is_some() {
        return Err(fail(ErrorKind::InvalidFStrInterp, span));
    }
    let node = match token.kind {
        TokenKind::Ident(name) if name == ksl_base::name::keywords().true_ => {
            ExprKind::Bool(true)
        },
        TokenKind::Ident(name) if name == ksl_base::name::keywords().false_ => {
            ExprKind::Bool(false)
        },
        TokenKind::Ident(name) if name == ksl_base::name::keywords().nil_ => ExprKind::Nil,
        TokenKind::Ident(name) => ExprKind::Ident(name),
        TokenKind::I64(value) => ExprKind::I64(value),
        TokenKind::I32(value) => ExprKind::I32(value),
        TokenKind::U8(value) => ExprKind::U8(value),
        TokenKind::U32(value) => ExprKind::U32(value),
        TokenKind::U64(value) => ExprKind::U64(value),
        TokenKind::F32(value) => ExprKind::F32(value),
        TokenKind::F64(value) => ExprKind::F64(value),
        TokenKind::Char(value) => ExprKind::Char(value),
        TokenKind::Str(value) | TokenKind::RawStr(value) => ExprKind::Str(value),
        _ => return Err(fail(ErrorKind::InvalidFStrInterp, span)),
    };
    Ok(Spanned::new(node, span))
}

fn special_expr(input: &mut Input<'_>) -> PResult<ExprKind> {
    bump(input);
    let name = ident(input)?;
    let text = name.node.as_str();
    if let Some(kind) = FormKind::from_name(text) {
        expect(input, &TokenKind::LBracket)?;
        let form = form(input, kind)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(ExprKind::Special(form));
    }
    if let Some(op) = crate::ast::option_op_kind(text) {
        expect(input, &TokenKind::LBracket)?;
        let args = args_until(input, &TokenKind::RBracket)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(ExprKind::Special(Special::OptionOp { op, args }));
    }
    if let Some(op) = crate::ast::list_op_kind(text) {
        expect(input, &TokenKind::LBracket)?;
        let args = args_until(input, &TokenKind::RBracket)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(ExprKind::Special(Special::ListOp { op, args }));
    }
    if let Some(op) = crate::ast::pair_op_kind(text) {
        expect(input, &TokenKind::LBracket)?;
        let args = args_until(input, &TokenKind::RBracket)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(ExprKind::Special(Special::PairOp { op, args }));
    }
    if let Some(op) = crate::ast::dict_op_kind(text) {
        expect(input, &TokenKind::LBracket)?;
        let args = args_until(input, &TokenKind::RBracket)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(ExprKind::Special(Special::DictOp { op, args }));
    }
    if let Some(op) = crate::ast::set_op_kind(text) {
        expect(input, &TokenKind::LBracket)?;
        let args = args_until(input, &TokenKind::RBracket)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(ExprKind::Special(Special::SetOp { op, args }));
    }
    Err(fail(ErrorKind::UnknownSpecialForm, name.span))
}

fn form(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        kind @ (FormKind::Let | FormKind::Def | FormKind::Fix) => form_binding(input, kind)?,
        kind @ (FormKind::Fn | FormKind::Handler | FormKind::Main) => {
            form_callable(input, kind)?
        },
        FormKind::Data => form_data(input, FormKind::Data)?,
        kind @ (FormKind::Export | FormKind::Use) => form_import_export(input, kind)?,
        FormKind::Link => form_link(input, FormKind::Link)?,
        kind @ (FormKind::Block
        | FormKind::If
        | FormKind::Which
        | FormKind::Exit
        | FormKind::Match
        | FormKind::DataGet) => form_control(input, kind)?,
        kind @ (FormKind::Continue
        | FormKind::Spawn
        | FormKind::SpawnDetached
        | FormKind::Send
        | FormKind::Receive
        | FormKind::SetTimer
        | FormKind::CancelTimer
        | FormKind::Sleep) => form_message(input, kind)?,
        kind @ (FormKind::Join | FormKind::Kill | FormKind::PidSerial | FormKind::Halt) => {
            form_actor_simple(input, kind)?
        },
        _ => form_value(input, kind)?,
    };
    Ok(special)
}

fn form_binding(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Let => {
            let target = binding_target(input)?;
            expect(input, &TokenKind::Comma)?;
            let value = self::expr(input)?;
            Special::Let {
                target,
                value: Box::new(value),
            }
        },
        FormKind::Def => Special::Def(def_body(input)?),
        FormKind::Fix => {
            let mut defs = Vec::new();
            if !at(input, &TokenKind::RBracket) {
                loop {
                    expect(input, &TokenKind::At)?;
                    let name = ident(input)?;
                    if FormKind::from_name(name.node.as_str()) != Some(FormKind::Def) {
                        return Err(fail(ErrorKind::ExpectedDef, name.span));
                    }
                    expect(input, &TokenKind::LBracket)?;
                    defs.push(def_body(input)?);
                    expect(input, &TokenKind::RBracket)?;
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            Special::Fix(defs)
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_callable(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Fn => {
            let (params, ret, body) = params_ret_body(input)?;
            Special::Fn {
                params,
                ret,
                body: Box::new(body),
            }
        },
        FormKind::Handler => {
            let (params, ret, body) = params_ret_body(input)?;
            Special::Handler {
                params,
                ret,
                body: Box::new(body),
            }
        },
        FormKind::Main => {
            let (params, ret, body) = params_ret_body(input)?;
            Special::Main {
                params,
                ret,
                body: Box::new(body),
            }
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_data(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Data => {
            let name = ident(input)?;
            expect(input, &TokenKind::Comma)?;
            expect(input, &TokenKind::LBrace)?;
            let mut variants = Vec::new();
            if !at(input, &TokenKind::RBrace) {
                loop {
                    let variant_name = ident(input)?;
                    expect(input, &TokenKind::LBracket)?;
                    let mut fields = Vec::new();
                    if !at(input, &TokenKind::RBracket) {
                        loop {
                            fields.push(type_expr(input)?);
                            if eat(input, &TokenKind::Comma) {
                                continue;
                            }
                            break;
                        }
                    }
                    expect(input, &TokenKind::RBracket)?;
                    variants.push(DataVariant {
                        name: variant_name,
                        fields,
                    });
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            expect(input, &TokenKind::RBrace)?;
            Special::Data { name, variants }
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_import_export(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Export => {
            let mut items = Vec::new();
            if !at(input, &TokenKind::RBracket) {
                loop {
                    if eat(input, &TokenKind::Caret) {
                        items.push(ExportItem::Type(ident(input)?));
                    } else {
                        items.push(ExportItem::Value(ident(input)?));
                    }
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            Special::Export { items }
        },
        FormKind::Use => {
            expect(input, &TokenKind::LBrace)?;
            let mut items = Vec::new();
            if !at(input, &TokenKind::RBrace) {
                loop {
                    items.push(use_item(input)?);
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            expect(input, &TokenKind::RBrace)?;
            expect(input, &TokenKind::Comma)?;
            let path = string_literal(input)?;
            Special::Use { items, path }
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_link(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Link => {
            let plugin = string_literal(input)?;
            expect(input, &TokenKind::Comma)?;
            expect(input, &TokenKind::LBrace)?;
            let mut types = Vec::new();
            if !at(input, &TokenKind::RBrace) {
                loop {
                    expect(input, &TokenKind::Caret)?;
                    types.push(ident(input)?);
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            expect(input, &TokenKind::RBrace)?;
            expect(input, &TokenKind::Comma)?;
            expect(input, &TokenKind::LBrace)?;
            let mut values = Vec::new();
            if !at(input, &TokenKind::RBrace) {
                loop {
                    let name = ident(input)?;
                    expect(input, &TokenKind::Colon)?;
                    let ty = type_expr(input)?;
                    values.push(LinkValue { name, ty });
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            expect(input, &TokenKind::RBrace)?;
            Special::Link(LinkDecl {
                plugin,
                types,
                values,
            })
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_control(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Block => Special::Block(args_until(input, &TokenKind::RBracket)?),
        FormKind::If => {
            let cond = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let then = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let els = self::expr(input)?;
            Special::If {
                cond: Box::new(cond),
                then: Box::new(then),
                els: Box::new(els),
            }
        },
        FormKind::Which => {
            expect(input, &TokenKind::LBrace)?;
            let mut branches = Vec::new();
            if !at(input, &TokenKind::RBrace) {
                loop {
                    expect(input, &TokenKind::LParen)?;
                    let cond = self::expr(input)?;
                    expect(input, &TokenKind::Comma)?;
                    let value = self::expr(input)?;
                    expect(input, &TokenKind::RParen)?;
                    branches.push((cond, value));
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            expect(input, &TokenKind::RBrace)?;
            expect(input, &TokenKind::Comma)?;
            let fallback = self::expr(input)?;
            Special::Which {
                branches,
                fallback: Box::new(fallback),
            }
        },
        FormKind::Exit => {
            let code = self::expr(input)?;
            Special::Exit {
                code: Box::new(code),
            }
        },
        FormKind::DataGet => {
            let value = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let index = self::expr(input)?;
            Special::DataGet {
                value: Box::new(value),
                index: Box::new(index),
            }
        },
        FormKind::Match => {
            let value = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            expect(input, &TokenKind::LBrace)?;
            let mut arms = Vec::new();
            if !at(input, &TokenKind::RBrace) {
                loop {
                    expect(input, &TokenKind::LParen)?;
                    let pattern = pattern(input)?;
                    expect(input, &TokenKind::Comma)?;
                    let body = self::expr(input)?;
                    expect(input, &TokenKind::RParen)?;
                    arms.push(MatchArm { pattern, body });
                    if eat(input, &TokenKind::Comma) {
                        continue;
                    }
                    break;
                }
            }
            expect(input, &TokenKind::RBrace)?;
            Special::Match {
                value: Box::new(value),
                arms,
            }
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_message(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Continue => Special::Continue {
            args: args_until(input, &TokenKind::RBracket)?,
        },
        FormKind::Spawn => form_spawn(input, false)?,
        FormKind::SpawnDetached => form_spawn(input, true)?,
        FormKind::Send => {
            let pid = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let index = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let message = self::expr(input)?;
            Special::Send {
                pid: Box::new(pid),
                index: Box::new(index),
                message: Box::new(message),
            }
        },
        FormKind::Receive => {
            let index = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let timeout = self::expr(input)?;
            Special::Receive {
                index: Box::new(index),
                timeout: Box::new(timeout),
            }
        },
        FormKind::SetTimer => {
            let pid = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let index = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let message = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let millis = self::expr(input)?;
            Special::SetTimer {
                pid: Box::new(pid),
                index: Box::new(index),
                message: Box::new(message),
                millis: Box::new(millis),
            }
        },
        FormKind::CancelTimer => {
            let id = self::expr(input)?;
            Special::CancelTimer { id: Box::new(id) }
        },
        FormKind::Sleep => {
            let millis = self::expr(input)?;
            Special::Sleep {
                millis: Box::new(millis),
            }
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_actor_simple(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Join => {
            let pid = self::expr(input)?;
            Special::Join { pid: Box::new(pid) }
        },
        FormKind::Kill => {
            let pid = self::expr(input)?;
            Special::Kill { pid: Box::new(pid) }
        },
        FormKind::PidSerial => {
            let pid = self::expr(input)?;
            Special::PidSerial { pid: Box::new(pid) }
        },
        FormKind::Halt => Special::Halt,
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_value(input: &mut Input<'_>, kind: FormKind) -> PResult<Special> {
    let special = match kind {
        FormKind::Dict => Special::Dict {
            entries: args_until(input, &TokenKind::RBracket)?,
        },
        FormKind::Set => Special::Set {
            items: args_until(input, &TokenKind::RBracket)?,
        },
        FormKind::Assert => {
            let cond = self::expr(input)?;
            expect(input, &TokenKind::Comma)?;
            let message = self::expr(input)?;
            Special::Assert {
                cond: Box::new(cond),
                message: Box::new(message),
            }
        },
        FormKind::Panic => {
            let message = self::expr(input)?;
            Special::Panic {
                message: Box::new(message),
            }
        },
        FormKind::Todo => {
            let message = self::expr(input)?;
            Special::Todo {
                message: Box::new(message),
            }
        },
        FormKind::Unreachable => {
            let message = self::expr(input)?;
            Special::Unreachable {
                message: Box::new(message),
            }
        },
        _ => unreachable!("特殊形式分组不匹配"),
    };
    Ok(special)
}

fn form_spawn(input: &mut Input<'_>, detached: bool) -> PResult<Special> {
    let handler = self::expr(input)?;
    let mut args = Vec::new();
    while eat(input, &TokenKind::Comma) {
        args.push(self::expr(input)?);
    }
    Ok(Special::Spawn {
        handler: Box::new(handler),
        args,
        detached,
    })
}

fn binding_target(input: &mut Input<'_>) -> PResult<BindingTarget> {
    let span = span(input);
    let name = match bump(input) {
        Some(Token {
            kind: TokenKind::Ident(name),
            ..
        }) => Some(Spanned::new(name, span)),
        Some(Token {
            kind: TokenKind::Discard,
            ..
        }) => None,
        _ => return Err(fail(ErrorKind::UnexpectedToken, span)),
    };
    let ty = if eat(input, &TokenKind::Colon) {
        Some(type_expr(input)?)
    } else {
        None
    };
    Ok(BindingTarget { name, ty })
}

fn params_ret_body(input: &mut Input<'_>) -> PResult<(Vec<Param>, TypeExpr, Expr)> {
    expect(input, &TokenKind::LBracket)?;
    let mut params = Vec::new();
    if !at(input, &TokenKind::RBracket) {
        loop {
            let name = ident(input)?;
            expect(input, &TokenKind::Colon)?;
            let ty = type_expr(input)?;
            params.push(Param { name, ty });
            if eat(input, &TokenKind::Comma) {
                continue;
            }
            break;
        }
    }
    expect(input, &TokenKind::RBracket)?;
    expect(input, &TokenKind::Colon)?;
    let ret = type_expr(input)?;
    expect(input, &TokenKind::Comma)?;
    let body = self::expr(input)?;
    Ok((params, ret, body))
}

fn def_body(input: &mut Input<'_>) -> PResult<Def> {
    let name = ident(input)?;
    expect(input, &TokenKind::Comma)?;
    let (params, ret, body) = params_ret_body(input)?;
    Ok(Def {
        name,
        params,
        ret,
        body: Box::new(body),
    })
}

fn use_item(input: &mut Input<'_>) -> PResult<UseItem> {
    if eat(input, &TokenKind::Caret) {
        if eat(input, &TokenKind::LParen) {
            let from = ident(input)?;
            expect(input, &TokenKind::Comma)?;
            let to = ident(input)?;
            expect(input, &TokenKind::RParen)?;
            return Ok(UseItem::TypeRename { from, to });
        }
        return Ok(UseItem::Type(ident(input)?));
    }
    if eat(input, &TokenKind::LParen) {
        let from = ident(input)?;
        expect(input, &TokenKind::Comma)?;
        let to = ident(input)?;
        expect(input, &TokenKind::RParen)?;
        return Ok(UseItem::Rename { from, to });
    }
    Ok(UseItem::Value(ident(input)?))
}

fn string_literal(input: &mut Input<'_>) -> PResult<String> {
    let span = span(input);
    match bump(input) {
        Some(Token {
            kind: TokenKind::Str(text) | TokenKind::RawStr(text),
            ..
        }) => Ok(text),
        _ => Err(fail(ErrorKind::UnexpectedToken, span)),
    }
}

fn type_expr(input: &mut Input<'_>) -> PResult<TypeExpr> {
    let depth = enter_depth(input, span(input))?;
    let result = type_expr_body(input);
    input.state.depth = depth - 1;
    result
}

fn type_expr_body(input: &mut Input<'_>) -> PResult<TypeExpr> {
    let name = ident(input)?;
    if name.node == ksl_base::name::keywords().fn_ && at(input, &TokenKind::LBracket) {
        bump(input);
        expect(input, &TokenKind::LBracket)?;
        let params = type_exprs(input)?;
        expect(input, &TokenKind::RBracket)?;
        expect(input, &TokenKind::Colon)?;
        let ret = type_expr(input)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(TypeExpr::Fn {
            span: name.span,
            params,
            ret: Box::new(ret),
        });
    }
    if at(input, &TokenKind::LBracket) {
        bump(input);
        let args = type_exprs(input)?;
        expect(input, &TokenKind::RBracket)?;
        return Ok(TypeExpr::Generic { name, args });
    }
    Ok(TypeExpr::Named(name))
}

fn type_exprs(input: &mut Input<'_>) -> PResult<Vec<TypeExpr>> {
    let mut types = Vec::new();
    if at(input, &TokenKind::RBracket) {
        return Ok(types);
    }
    loop {
        types.push(type_expr(input)?);
        if eat(input, &TokenKind::Comma) {
            continue;
        }
        break;
    }
    Ok(types)
}

fn pattern(input: &mut Input<'_>) -> PResult<Pattern> {
    if eat(input, &TokenKind::Discard) {
        return Ok(Pattern {
            qualifier: None,
            variant: None,
            fields: Vec::new(),
        });
    }
    let (qualifier, variant) = if eat(input, &TokenKind::Slash) {
        (None, ident(input)?)
    } else {
        let qualifier = ident(input)?;
        expect(input, &TokenKind::Slash)?;
        (Some(qualifier), ident(input)?)
    };
    expect(input, &TokenKind::LBracket)?;
    let mut fields = Vec::new();
    if !at(input, &TokenKind::RBracket) {
        loop {
            let span = span(input);
            let field = match bump(input) {
                Some(Token {
                    kind: TokenKind::Ident(name),
                    ..
                }) => PatternField::Bind(Spanned::new(name, span)),
                Some(Token {
                    kind: TokenKind::Discard,
                    ..
                }) => PatternField::Wildcard,
                _ => return Err(fail(ErrorKind::UnexpectedToken, span)),
            };
            fields.push(field);
            if eat(input, &TokenKind::Comma) {
                continue;
            }
            break;
        }
    }
    expect(input, &TokenKind::RBracket)?;
    Ok(Pattern {
        qualifier,
        variant: Some(variant),
        fields,
    })
}

fn peek<'a>(input: &'a Input<'_>) -> Option<&'a TokenKind> {
    input.peek_token().map(|token| &token.kind)
}

fn at(input: &Input<'_>, kind: &TokenKind) -> bool {
    peek(input) == Some(kind)
}

fn span(input: &Input<'_>) -> Span {
    input.peek_token().map_or_else(
        || Span::new(input.state.source, previous_end(input), previous_end(input)),
        |token| token.span,
    )
}

/// 进入一层表达式或类型; 超过上限时报语法错误.
fn enter_depth(input: &mut Input<'_>, span: Span) -> PResult<usize> {
    let depth = input.state.depth.saturating_add(1);
    if depth > MAX_NESTING {
        return Err(fail(ErrorKind::NestingTooDeep { limit: MAX_NESTING }, span));
    }
    input.state.depth = depth;
    Ok(depth)
}

fn nesting_error(input: &Input<'_>) -> ErrMode<ParseFailure> {
    fail(ErrorKind::NestingTooDeep { limit: MAX_NESTING }, span(input))
}

fn current_start(input: &Input<'_>) -> usize {
    span(input).start()
}

fn previous_end(input: &Input<'_>) -> usize {
    input.previous_token_end()
}

fn bump(input: &mut Input<'_>) -> Option<Token> {
    input.next_token().cloned()
}

fn eat(input: &mut Input<'_>, kind: &TokenKind) -> bool {
    if at(input, kind) {
        bump(input);
        true
    } else {
        false
    }
}

fn expect(input: &mut Input<'_>, kind: &TokenKind) -> PResult<Span> {
    let span = span(input);
    if eat(input, kind) {
        Ok(span)
    } else if at(input, &TokenKind::Eof) {
        Err(fail(ErrorKind::Unterminated, span))
    } else {
        Err(fail(ErrorKind::UnexpectedToken, span))
    }
}

fn ident(input: &mut Input<'_>) -> PResult<Spanned<Name>> {
    let span = span(input);
    match bump(input) {
        Some(Token {
            kind: TokenKind::Ident(name),
            ..
        }) => Ok(Spanned::new(name, span)),
        _ => Err(fail(ErrorKind::UnexpectedToken, span)),
    }
}

fn unexpected(input: &Input<'_>) -> ErrMode<ParseFailure> {
    let span = span(input);
    let kind = if at(input, &TokenKind::Eof) {
        ErrorKind::Unterminated
    } else {
        ErrorKind::UnexpectedToken
    };
    fail(kind, span)
}

fn unwrap_failure(error: ErrMode<ParseFailure>) -> KslError {
    match error {
        ErrMode::Backtrack(failure) | ErrMode::Cut(failure) => *failure.0,
        ErrMode::Incomplete(_) => KslError::without_span(ErrorKind::Unterminated),
    }
}

fn fail(kind: ErrorKind, span: Span) -> ErrMode<ParseFailure> {
    ErrMode::Cut(ParseFailure(Box::new(KslError::new(kind, span))))
}
