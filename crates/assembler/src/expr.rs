//! Arithmetic expression evaluation.
//!
//! Grammar and precedence (from `docs/FILE_FORMATS.md` §1):
//!   1) `()` grouping
//!   2) `* / &`  (mul / div / bitwise-AND)  — tightest binary
//!   3) `+ - \ !`  (add / sub / XOR / OR)    — looser, left-associative
//!
//!      unary  `+`, `-`, `^` (NOT)
//!
//! Note the non-standard mapping: `\` is XOR and `!` is OR, while `^` is NOT.

use crate::error::{AsmError, Result, Span};
use crate::lexer::Token;

/// Resolver callback that maps an identifier to its value (`None` = undefined).
/// `None` is treated as a distinct "undefined" so that labels can evaluate to
/// nothing during pass 1 (forward references) without being an error yet.
pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<i64>;

/// A parsed expression tree (pre-order stack of ops/values). This lets us
/// resolve lazily across passes without re-lexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Number(i64),
    /// Identifies a symbol (label or constant).
    Symbol(String),
    /// Current program counter `.`.
    Here,
    Unary {
        negate: bool,
        not: bool,
        inner: Box<Expr>,
    },
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Paren(Box<Expr>),
}

/// Binary operators of the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    And,
    Xor,
    Or,
}

impl Expr {
    /// Evaluate with a resolver. Returns `Ok(None)` if an identifier is
    /// unresolved (needs a later pass), `Err` on division-by-zero/overflow.
    pub fn eval(&self, resolve: &Resolver, span: Span) -> Result<Option<i64>> {
        use Expr::*;
        let un = |e: &Expr| e.eval(resolve, span);
        match self {
            Number(n) => Ok(Some(*n)),
            Symbol(s) => Ok(resolve(s)),
            Here => unreachable!("`.` handled by caller"),
            Unary { negate, not, inner } => match un(inner)? {
                Some(v) => {
                    let v = if *not { !v } else { v };
                    Ok(Some(if *negate { -v } else { v }))
                }
                None => Ok(None),
            },
            Binary(op, l, r) => {
                let lv = match un(l)? {
                    Some(v) => v,
                    None => return Ok(None),
                };
                let rv = match un(r)? {
                    Some(v) => v,
                    None => return Ok(None),
                };
                let v = match op {
                    BinOp::Add => lv.checked_add(rv),
                    BinOp::Sub => lv.checked_sub(rv),
                    BinOp::Mul => lv.checked_mul(rv),
                    BinOp::And => Some(lv & rv),
                    BinOp::Div => {
                        if rv == 0 {
                            return Err(AsmError::new(span, "division by zero"));
                        } else {
                            lv.checked_div(rv)
                        }
                    }
                    BinOp::Xor => Some(lv ^ rv),
                    BinOp::Or => Some(lv | rv),
                };
                match v {
                    Some(v) => Ok(Some(v)),
                    None => Err(AsmError::new(span, "expression overflow")),
                }
            }
            Paren(inner) => un(inner),
        }
    }

    /// Replace every `Here` (`.`) node with its resolved program-counter value,
    /// so an expression tree may be evaluated when `.` is in scope. Used by the
    /// assembler before `eval`; the parser emits `Here`, never `Symbol(".")`.
    pub fn with_here(&self, pc: i64) -> Expr {
        use Expr::*;
        match self {
            Number(n) => Number(*n),
            Symbol(s) => Symbol(s.clone()),
            Here => Number(pc),
            Unary { negate, not, inner } => Unary {
                negate: *negate,
                not: *not,
                inner: Box::new(inner.with_here(pc)),
            },
            Binary(op, l, r) => Binary(*op, Box::new(l.with_here(pc)), Box::new(r.with_here(pc))),
            Paren(inner) => Paren(Box::new(inner.with_here(pc))),
        }
    }
}

// ---- Parser ----

/// Recursive-descent parser producing an [`Expr`] AST.
pub struct Parser<'a> {
    toks: &'a [Token],
    pos: usize,
    span: Span,
}

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [Token], span: Span) -> Self {
        Parser { toks, pos: 0, span }
    }

    pub fn peek(&self) -> &Token {
        self.toks.get(self.pos).unwrap_or(&Token::Eof)
    }
    pub fn bump(&mut self) {
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
    }

    /// Parse a full expression, requiring the caller to then expect `Eof`.
    pub fn parse(mut self) -> Result<Expr> {
        let e = self.addition()?;
        if !matches!(self.peek(), Token::Eof) {
            return Err(AsmError::new(
                self.span,
                format!("unexpected {}", self.peek().describe()),
            ));
        }
        Ok(e)
    }

    fn addition(&mut self) -> Result<Expr> {
        let mut left = self.mul()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinOp::Add,
                Token::Minus => BinOp::Sub,
                Token::Backslash => BinOp::Xor,
                Token::Bang => BinOp::Or,
                _ => break,
            };
            self.bump();
            let right = self.mul()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn mul(&mut self) -> Result<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Token::Star => BinOp::Mul,
                Token::Slash => BinOp::Div,
                Token::Amp => BinOp::And,
                _ => break,
            };
            self.bump();
            let right = self.unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Token::Minus => {
                self.bump();
                Ok(Expr::Unary {
                    negate: true,
                    not: false,
                    inner: Box::new(self.unary()?),
                })
            }
            Token::Plus => {
                self.bump();
                self.unary()
            }
            Token::Caret => {
                self.bump();
                Ok(Expr::Unary {
                    negate: false,
                    not: true,
                    inner: Box::new(self.unary()?),
                })
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr> {
        match self.peek().clone() {
            Token::Number(n) => {
                self.bump();
                Ok(Expr::Number(n))
            }
            Token::Sym(s) => {
                self.bump();
                Ok(Expr::Symbol(s))
            }
            Token::Dot => {
                self.bump();
                Ok(Expr::Here)
            }
            Token::LParen => {
                self.bump();
                let inner = self.addition()?;
                match self.peek() {
                    Token::RParen => {
                        self.bump();
                        Ok(Expr::Paren(Box::new(inner)))
                    }
                    _ => Err(AsmError::new(self.span, "expected `)`")),
                }
            }
            other => Err(AsmError::new(
                self.span,
                format!("expected expression, found {}", other.describe()),
            )),
        }
    }
}
