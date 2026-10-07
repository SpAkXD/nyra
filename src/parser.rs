//! Recursive-descent parser with precedence climbing for expressions.
//! On an error it records a diagnostic and skips to the next statement, so one
//! run reports as many errors as possible.

use crate::ast::*;
use crate::diag::Diag;
use crate::lexer::{Tok, Token};

type PResult<T> = Result<T, Diag>;

pub fn parse(toks: Vec<Token>) -> (Program, Vec<Diag>) {
    let mut p = Parser { toks, pos: 0, errs: Vec::new() };
    let prog = p.program();
    (prog, p.errs)
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    errs: Vec<Diag>,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn skip_newlines(&mut self) {
        while self.at(&Tok::Newline) {
            self.bump();
        }
    }

    fn unexpected(&self, expected: &str) -> Diag {
        Diag::new("E0101", format!("expected {expected}, found {}", self.peek().describe()), self.span())
    }

    fn expect(&mut self, t: Tok, expected: &str) -> PResult<Span> {
        if self.at(&t) {
            Ok(self.bump().span)
        } else {
            Err(self.unexpected(expected))
        }
    }

    fn ident(&mut self, what: &str) -> PResult<(String, Span)> {
        if let Tok::Ident(name) = self.peek().clone() {
            Ok((name, self.bump().span))
        } else {
            Err(self.unexpected(what))
        }
    }

    // ---- top level -------------------------------------------------------

    fn program(&mut self) -> Program {
        let mut funcs = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::Eof => break,
                Tok::Fn => match self.func() {
                    Ok(f) => funcs.push(f),
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_top();
                    }
                },
                _ => {
                    let d = self
                        .unexpected("`fn`")
                        .hint("the top level of a file may only contain `fn` definitions");
                    self.errs.push(d);
                    self.sync_top();
                }
            }
        }
        Program { funcs }
    }

    fn sync_top(&mut self) {
        if !self.at(&Tok::Fn) {
            self.bump();
        }
        while !matches!(self.peek(), Tok::Fn | Tok::Eof) {
            self.bump();
        }
    }

    fn func(&mut self) -> PResult<Func> {
        self.expect(Tok::Fn, "`fn`")?;
        let (name, span) = self.ident("a function name")?;
        self.expect(Tok::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            let (pname, pspan) = self.ident("a parameter name")?;
            self.expect(Tok::Colon, "`:` and a type")
                .map_err(|d| d.hint(format!("parameters need a type, e.g. `{pname}: int`")))?;
            let ty = self.ty()?;
            params.push(Param { name: pname, ty, span: pspan });
            if !self.at(&Tok::RParen) {
                self.expect(Tok::Comma, "`,` or `)`")?;
            }
        }
        self.bump();
        let ret = if self.at(&Tok::Arrow) {
            self.bump();
            self.ty()?
        } else {
            Type::Void
        };
        let body = if self.at(&Tok::Assign) {
            // one-line function: the expression is the body (and the return value)
            self.bump();
            let e = self.expr()?;
            self.end_stmt()?;
            let espan = e.span;
            let kind = if ret == Type::Void { StmtKind::Expr(e) } else { StmtKind::Ret(Some(e)) };
            vec![Stmt { kind, span: espan }]
        } else {
            self.block()?
        };
        Ok(Func { name, params, ret, body, span })
    }

    fn ty(&mut self) -> PResult<Type> {
        let span = self.span();
        let (name, _) = self.ident("a type")?;
        match name.as_str() {
            "int" => Ok(Type::Int),
            "float" => Ok(Type::Float),
            "bool" => Ok(Type::Bool),
            "str" => Ok(Type::Str),
            _ => Err(Diag::new("E0102", format!("unknown type `{name}`"), span)
                .hint("the types are: int, float, bool, str")),
        }
    }

    // ---- statements ------------------------------------------------------

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect(Tok::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::RBrace => {
                    self.bump();
                    return Ok(stmts);
                }
                Tok::Eof | Tok::Fn => return Err(self.unexpected("`}`")),
                _ => match self.stmt() {
                    Ok(s) => stmts.push(s),
                    Err(d) => {
                        self.errs.push(d);
                        self.sync_stmt();
                    }
                },
            }
        }
    }

    fn sync_stmt(&mut self) {
        let mut depth = 0usize;
        loop {
            match self.peek() {
                Tok::Eof | Tok::Fn => return,
                Tok::LBrace => depth += 1,
                Tok::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                Tok::Newline if depth == 0 => {
                    self.bump();
                    return;
                }
                _ => {}
            }
            self.bump();
        }
    }

    fn end_stmt(&mut self) -> PResult<()> {
        match self.peek() {
            Tok::Newline => {
                self.bump();
                Ok(())
            }
            Tok::RBrace | Tok::Eof => Ok(()),
            _ => Err(self.unexpected("end of line").hint("put each statement on its own line")),
        }
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Let | Tok::Var => {
                let mutable = self.bump().tok == Tok::Var;
                let (name, _) = self.ident("a variable name")?;
                let ty = if self.at(&Tok::Colon) {
                    self.bump();
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect(Tok::Assign, "`=`")
                    .map_err(|d| d.hint(format!("variables must be initialized: `let {name} = ...`")))?;
                let value = self.expr()?;
                self.end_stmt()?;
                StmtKind::Let { name, mutable, ty, value }
            }
            Tok::If => return self.if_stmt(),
            Tok::While => {
                self.bump();
                let cond = self.expr()?;
                let body = self.block()?;
                StmtKind::While { cond, body }
            }
            Tok::For => {
                self.bump();
                let (var, _) = self.ident("a loop variable")?;
                self.expect(Tok::In, "`in`")?;
                let start = self.expr()?;
                self.expect(Tok::DotDot, "`..`")
                    .map_err(|d| d.hint("loops look like `for i in 0..10 { }`"))?;
                let end = self.expr()?;
                let body = self.block()?;
                StmtKind::For { var, start, end, body }
            }
            Tok::Ret => {
                self.bump();
                let value = if matches!(self.peek(), Tok::Newline | Tok::RBrace | Tok::Eof) {
                    None
                } else {
                    Some(self.expr()?)
                };
                self.end_stmt()?;
                StmtKind::Ret(value)
            }
            Tok::Ident(name) if *self.peek_at(1) == Tok::Assign => {
                self.bump();
                self.bump();
                let value = self.expr()?;
                self.end_stmt()?;
                StmtKind::Assign { name, value }
            }
            Tok::Ident(name) if matches!(self.peek_at(1), Tok::OpAssign(_)) => {
                self.bump();
                let Tok::OpAssign(op) = self.bump().tok else { unreachable!() };
                let rhs = self.expr()?;
                self.end_stmt()?;
                let var = Expr::new(ExprKind::Var(name.clone()), span);
                let value = Expr::new(ExprKind::Binary(op, Box::new(var), Box::new(rhs)), span);
                StmtKind::Assign { name, value }
            }
            _ => {
                let e = self.expr()?;
                self.end_stmt()?;
                StmtKind::Expr(e)
            }
        };
        Ok(Stmt { kind, span })
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let span = self.expect(Tok::If, "`if`")?;
        let cond = self.expr()?;
        let then = self.block()?;
        // `else` may sit on the line after the closing `}`.
        let save = self.pos;
        self.skip_newlines();
        let els = if self.at(&Tok::Else) {
            self.bump();
            if self.at(&Tok::If) {
                Some(vec![self.if_stmt()?])
            } else {
                Some(self.block()?)
            }
        } else {
            self.pos = save;
            None
        };
        Ok(Stmt { kind: StmtKind::If { cond, then, els }, span })
    }

    // ---- expressions -----------------------------------------------------

    fn expr(&mut self) -> PResult<Expr> {
        self.binary(1)
    }

    fn binop(t: &Tok) -> Option<(BinOp, u8)> {
        Some(match t {
            Tok::Or => (BinOp::Or, 1),
            Tok::And => (BinOp::And, 2),
            Tok::Eq => (BinOp::Eq, 3),
            Tok::Ne => (BinOp::Ne, 3),
            Tok::Lt => (BinOp::Lt, 4),
            Tok::Le => (BinOp::Le, 4),
            Tok::Gt => (BinOp::Gt, 4),
            Tok::Ge => (BinOp::Ge, 4),
            Tok::Plus => (BinOp::Add, 5),
            Tok::Minus => (BinOp::Sub, 5),
            Tok::Star => (BinOp::Mul, 6),
            Tok::Slash => (BinOp::Div, 6),
            Tok::Percent => (BinOp::Mod, 6),
            _ => return None,
        })
    }

    fn binary(&mut self, min_prec: u8) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        while let Some((op, prec)) = Self::binop(self.peek()) {
            if prec < min_prec {
                break;
            }
            let span = self.bump().span;
            self.skip_newlines();
            let rhs = self.binary(prec + 1)?;
            lhs = Expr::new(ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), span);
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Not => UnOp::Not,
            _ => return self.primary(),
        };
        self.bump();
        let inner = self.unary()?;
        Ok(Expr::new(ExprKind::Unary(op, Box::new(inner)), span))
    }

    fn primary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                ExprKind::Int(n)
            }
            Tok::Float(f) => {
                self.bump();
                ExprKind::Float(f)
            }
            Tok::Str(s) => {
                self.bump();
                ExprKind::Str(s)
            }
            Tok::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            Tok::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            Tok::Ident(name) => {
                self.bump();
                if self.at(&Tok::LParen) {
                    self.bump();
                    let mut args = Vec::new();
                    while !self.at(&Tok::RParen) {
                        args.push(self.expr()?);
                        if !self.at(&Tok::RParen) {
                            self.expect(Tok::Comma, "`,` or `)`")?;
                        }
                    }
                    self.bump();
                    ExprKind::Call(name, args)
                } else {
                    ExprKind::Var(name)
                }
            }
            Tok::LParen => {
                self.bump();
                let e = self.expr()?;
                self.expect(Tok::RParen, "`)`")?;
                return Ok(e);
            }
            _ => return Err(self.unexpected("an expression")),
        };
        Ok(Expr::new(kind, span))
    }
}
