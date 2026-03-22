use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ast::{
    BinaryOp, ContractPolicy, CycleContract, Expr, Function, Instruction, Program, Stmt, UnaryOp,
};
use crate::error::LexerError;
use crate::lexer::Lexer;
use crate::token::{Token, TokenKind};

#[derive(Debug, Clone)]
pub struct ParseError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl ParseError {
    fn new(token: &Token, message: impl Into<String>) -> Self {
        Self {
            line: token.span.line,
            column: token.span.column,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at {}:{}", self.message, self.line, self.column)
    }
}

impl std::error::Error for ParseError {}

impl From<LexerError> for ParseError {
    fn from(value: LexerError) -> Self {
        Self {
            line: value.line,
            column: value.column,
            message: value.message,
        }
    }
}

pub struct Parser {
    source: String,
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn from_source(source: &str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize()?;
        Ok(Self {
            source: source.to_string(),
            tokens,
            pos: 0,
        })
    }

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut data = BTreeMap::new();
        let mut imports = Vec::new();
        let mut import_spans = Vec::new();
        let mut functions = Vec::new();

        self.skip_newlines();
        while !self.is_at_end() {
            self.expect(TokenKind::KeywordSection, "Expected `section`")?;
            self.expect(TokenKind::Dot, "Expected `.` after `section`")?;
            let section_name = self.expect_identifier_like("Expected section name")?.lexeme;
            self.expect(TokenKind::Colon, "Expected `:` after section name")?;
            self.expect(TokenKind::Newline, "Expected newline after section header")?;
            self.expect(TokenKind::Indent, "Expected indented block after section header")?;

            match section_name.as_str() {
                "data" => {
                    while !self.check(TokenKind::Dedent) && !self.is_at_end() {
                        self.skip_newlines();
                        if self.check(TokenKind::Dedent) {
                            break;
                        }
                        let key = self.expect_identifier_like("Expected data key")?.lexeme;
                        self.expect(TokenKind::Colon, "Expected `:` after data key")?;
                        let value = self.parse_expression()?;
                        self.consume_newline_if_present();
                        data.insert(key, value);
                    }
                }
                "text" => {
                    while !self.check(TokenKind::Dedent) && !self.is_at_end() {
                        self.skip_newlines();
                        if self.check(TokenKind::Dedent) {
                            break;
                        }
                        if self.match_kind(TokenKind::KeywordImport) {
                            let (module, span) = self.parse_import_stmt()?;
                            imports.push(module);
                            import_spans.push(span);
                        } else {
                            functions.push(self.parse_function()?);
                        }
                    }
                }
                _ => {
                    return Err(ParseError::new(
                        self.peek(),
                        format!("Unknown section `.{}'", section_name),
                    ));
                }
            }

            self.expect(TokenKind::Dedent, "Expected section block end")?;
            self.skip_newlines();
        }

        Ok(Program {
            data,
            imports,
            import_spans,
            functions,
            source: self.source.clone(),
        })
    }

    fn parse_import_stmt(&mut self) -> Result<(String, crate::token::Span), ParseError> {
        let span = self.previous().span;
        let module = if self.match_kind(TokenKind::String) {
            self.previous().lexeme.clone()
        } else {
            self.expect_identifier_like("Expected module name or path string after `import`")?
                .lexeme
        };
        self.consume_stmt_terminator();
        Ok((module, span))
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let is_interrupt = self.match_kind(TokenKind::KeywordInterrupt);
        self.expect(TokenKind::KeywordFn, "Expected `fn`")?;
        let fn_tok = self.previous().clone();
        let name = self.expect_identifier_like("Expected function name")?.lexeme;
        self.expect(TokenKind::LParen, "Expected `(` after function name")?;

        let mut params = Vec::new();
        if !self.check(TokenKind::RParen) {
            loop {
                let p = self.expect_identifier_like("Expected function parameter")?.lexeme;
                params.push(p);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
        }

        self.expect(TokenKind::RParen, "Expected `)` after function params")?;
        let body = self.parse_block("function body")?;
        Ok(Function {
            name,
            span: fn_tok.span,
            is_interrupt,
            params,
            body,
        })
    }

    fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        let suppress_unused_warning = self.match_kind(TokenKind::KeywordUnused);

        if self.match_kind(TokenKind::KeywordConst) {
            let span = self.previous().span;
            let name = self
                .expect_identifier_like("Expected identifier after `const`")?
                .lexeme;
            self.expect(TokenKind::Assign, "Expected `=` in const declaration")?;
            let expr = self.parse_expression()?;
            self.consume_stmt_terminator();
            return Ok(Stmt::ConstDecl {
                name,
                expr,
                suppress_unused_warning,
                span,
            });
        }

        if self.check(TokenKind::KeywordInt)
            || self.check(TokenKind::KeywordString)
            || self.check(TokenKind::KeywordBool)
        {
            let decl_tok = self.advance().clone();
            let name = self.expect_identifier_like("Expected identifier after type")?.lexeme;
            self.expect(TokenKind::Assign, "Expected `=` in typed declaration")?;
            let expr = self.parse_expression()?;
            self.consume_stmt_terminator();
            return Ok(Stmt::OwnDecl {
                name,
                expr,
                suppress_unused_warning,
                span: decl_tok.span,
            });
        }

        if self.match_kind(TokenKind::KeywordOwn) {
            let span = self.previous().span;
            if self.match_kind(TokenKind::LBracket) {
                if suppress_unused_warning {
                    return Err(ParseError::new(
                        self.previous(),
                        "`unused` can only be applied to variable declarations",
                    ));
                }
                let port = self
                    .expect_identifier_like("Expected port identifier after `own [`")?
                    .lexeme;
                self.expect(TokenKind::RBracket, "Expected `]` after port identifier")?;
                self.consume_stmt_terminator();
                return Ok(Stmt::PortOwn { port, span });
            }

            let name = self.expect_identifier_like("Expected identifier after `own`")?.lexeme;
            self.expect(TokenKind::Assign, "Expected `=` in own declaration")?;
            let expr = self.parse_expression()?;
            self.consume_stmt_terminator();
            return Ok(Stmt::OwnDecl {
                name,
                expr,
                suppress_unused_warning,
                span,
            });
        }

        if self.match_kind(TokenKind::KeywordRef) {
            let span = self.previous().span;
            if self.match_kind(TokenKind::LBracket) {
                if suppress_unused_warning {
                    return Err(ParseError::new(
                        self.previous(),
                        "`unused` can only be applied to variable declarations",
                    ));
                }
                let port = self
                    .expect_identifier_like("Expected port identifier after `ref [`")?
                    .lexeme;
                self.expect(TokenKind::RBracket, "Expected `]` after port identifier")?;
                self.consume_stmt_terminator();
                return Ok(Stmt::PortRef { port, span });
            }

            let name = self.expect_identifier_like("Expected identifier after `ref`")?.lexeme;
            self.expect(TokenKind::Assign, "Expected `=` in ref declaration")?;
            self.expect(TokenKind::Ampersand, "Expected `&` in ref declaration")?;
            let target = if self.match_kind(TokenKind::LBracket) {
                let port = self
                    .expect_identifier_like("Expected referenced port identifier")?
                    .lexeme;
                self.expect(TokenKind::RBracket, "Expected `]` after referenced port")?;
                format!("[{}]", port)
            } else {
                self.expect_identifier_like("Expected referenced identifier")?.lexeme
            };
            self.consume_stmt_terminator();
            return Ok(Stmt::RefDecl {
                name,
                target,
                suppress_unused_warning,
                span,
            });
        }

        if suppress_unused_warning {
            return Err(ParseError::new(
                self.peek(),
                "`unused` can only be applied to variable declarations",
            ));
        }

        if self.match_kind(TokenKind::KeywordIf) {
            let span = self.previous().span;
            let condition = self.parse_expression()?;
            let then_body = self.parse_block("if body")?;
            self.skip_newlines();

            let mut else_body = Vec::new();
            if self.match_kind(TokenKind::KeywordElse) {
                else_body = self.parse_block("else body")?;
            }

            return Ok(Stmt::If {
                condition,
                then_body,
                else_body,
                span,
            });
        }

        if self.match_kind(TokenKind::KeywordWhile) {
            let span = self.previous().span;
            let condition = self.parse_expression()?;
            let body = self.parse_block("while body")?;
            return Ok(Stmt::While {
                condition,
                body,
                span,
            });
        }

        if self.match_kind(TokenKind::KeywordRepeat) {
            let span = self.previous().span;
            let times = self.parse_expression()?;
            let body = self.parse_block("repeat body")?;
            return Ok(Stmt::Repeat { times, body, span });
        }

        if self.match_kind(TokenKind::KeywordFor) {
            let span = self.previous().span;
            let name = self
                .expect_identifier_like("Expected loop variable after `for`")?
                .lexeme;
            self.expect(TokenKind::KeywordIn, "Expected `in` in for loop")?;
            let iterable = self.parse_expression()?;
            let body = self.parse_block("for body")?;
            return Ok(Stmt::For {
                name,
                iterable,
                body,
                span,
            });
        }

        if self.match_kind(TokenKind::KeywordYield) {
            let span = self.previous().span;
            self.expect(TokenKind::LBracket, "Expected `[` after `yield`")?;
            let port = self
                .expect_identifier_like("Expected hardware port after `yield [`")?
                .lexeme;
            self.expect(TokenKind::RBracket, "Expected `]` after yield port")?;
            self.expect(TokenKind::KeywordTo, "Expected `to` in yield statement")?;
            let handler = self
                .expect_identifier_like("Expected interrupt handler function name after `to`")?
                .lexeme;
            let body = self.parse_block("yield body")?;
            return Ok(Stmt::YieldPort {
                port,
                handler,
                body,
                span,
            });
        }

        if self.match_kind(TokenKind::KeywordContract) {
            let spec = self.parse_cycle_contract_spec()?;
            self.skip_newlines();
            self.expect(
                TokenKind::KeywordExecute,
                "Expected `execute` block after `contract`",
            )?;
            let span = self.previous().span;
            let body = self.parse_block("contract execute body")?;
            return Ok(Stmt::CycleContract { spec, body, span });
        }

        if self.match_kind(TokenKind::KeywordPrint) {
            let span = self.previous().span;
            self.expect(TokenKind::Colon, "Expected `:` after `print`")?;
            self.expect(TokenKind::Newline, "Expected newline after `print:`")?;
            self.expect(TokenKind::Indent, "Expected indented print block")?;
            let mut fields = Vec::new();
            while !self.check(TokenKind::Dedent) && !self.is_at_end() {
                self.skip_newlines();
                if self.check(TokenKind::Dedent) {
                    break;
                }
                let key_tok = self.advance().clone();
                if key_tok.kind != TokenKind::YamlKey && key_tok.kind != TokenKind::Identifier {
                    return Err(ParseError::new(&key_tok, "Expected print field key"));
                }
                self.expect(TokenKind::Colon, "Expected `:` after print field key")?;
                let expr = self.parse_expression()?;
                self.consume_newline_if_present();
                fields.push((key_tok.lexeme, expr));
            }
            self.expect(TokenKind::Dedent, "Expected end of print block")?;
            return Ok(Stmt::PrintBlock { fields, span });
        }

        if self.match_kind(TokenKind::KeywordReturn) {
            let span = self.previous().span;
            if self.check(TokenKind::Newline) {
                self.advance();
                return Ok(Stmt::Return { expr: None, span });
            }
            let expr = self.parse_expression()?;
            self.consume_stmt_terminator();
            return Ok(Stmt::Return {
                expr: Some(expr),
                span,
            });
        }

        if self.check(TokenKind::Mnemonic) {
            let op_tok = self.advance().clone();
            let op = match op_tok.lexeme.to_ascii_lowercase().as_str() {
                "mov" => Instruction::Mov,
                "add" => Instruction::Add,
                "sub" => Instruction::Sub,
                "mul" => Instruction::Mul,
                "div" => Instruction::Div,
                "mod" => Instruction::Mod,
                "cmp" => Instruction::Cmp,
                _ => {
                    return Err(ParseError::new(
                        &op_tok,
                        format!("Unsupported instruction `{}`", op_tok.lexeme),
                    ));
                }
            };
            let target = self.parse_instruction_target()?;
            self.expect(TokenKind::Comma, "Expected `,` after instruction target")?;
            let rhs = self.parse_expression()?;
            self.consume_stmt_terminator();
            return Ok(Stmt::Instruction {
                op,
                target,
                rhs,
                span: op_tok.span,
            });
        }

        if self.check(TokenKind::Identifier) || self.check(TokenKind::Register) {
            let start_tok = self.advance().clone();
            let name = start_tok.lexeme;
            let start_span = start_tok.span;
            let qualified = self.parse_qualified_name(name)?;
            if self.match_kind(TokenKind::Assign) {
                if qualified.contains('.') {
                    return Err(ParseError::new(
                        self.peek(),
                        "Qualified names cannot be assignment targets",
                    ));
                }
                let expr = self.parse_expression()?;
                self.consume_stmt_terminator();
                return Ok(Stmt::Assign {
                    name: qualified,
                    expr,
                    span: start_span,
                });
            }
            if self.match_kind(TokenKind::LParen) {
                let args = self.parse_call_args()?;
                self.consume_stmt_terminator();
                let call_expr = Expr::Call {
                    name: qualified,
                    args,
                    span: start_span,
                };
                let span = call_expr.span();
                return Ok(Stmt::Expr {
                    expr: call_expr,
                    span,
                });
            }
            if qualified.contains('.') {
                return Err(ParseError::new(
                    self.peek(),
                    "Qualified names must be used as function calls",
                ));
            }
            return Err(ParseError::new(
                self.peek(),
                "Expected assignment (`=`) or function call",
            ));
        }

        let expr = self.parse_expression()?;
        self.consume_stmt_terminator();
        let span = expr.span();
        Ok(Stmt::Expr { expr, span })
    }

    fn parse_block(&mut self, context: &str) -> Result<Vec<Stmt>, ParseError> {
        if self.match_kind(TokenKind::Colon) {
            self.expect(TokenKind::Newline, "Expected newline after `:`")?;
            self.expect(TokenKind::Indent, "Expected indented block")?;
            let mut body = Vec::new();
            while !self.check(TokenKind::Dedent) && !self.is_at_end() {
                self.skip_newlines();
                if self.check(TokenKind::Dedent) {
                    break;
                }
                body.push(self.parse_statement()?);
            }
            self.expect(TokenKind::Dedent, "Expected end of indented block")?;
            return Ok(body);
        }

        Err(ParseError::new(
            self.peek(),
            format!("Expected ':' for {}", context),
        ))
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, ParseError> {
        let mut args = Vec::new();
        if !self.check(TokenKind::RParen) {
            loop {
                args.push(self.parse_expression()?);
                if !self.match_kind(TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen, "Expected `)` after call args")?;
        Ok(args)
    }

    fn parse_expression(&mut self) -> Result<Expr, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_xor()?;
        while self.match_kind(TokenKind::KeywordOr) {
            let right = self.parse_xor()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_xor(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_and()?;
        while self.match_kind(TokenKind::KeywordXor) {
            let right = self.parse_and()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Xor,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_bit_or()?;
        while self.match_kind(TokenKind::KeywordAnd) {
            let right = self.parse_bit_or()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_bit_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_bit_and()?;
        while self.match_kind(TokenKind::Pipe) {
            let right = self.parse_bit_and()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitOr,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_bit_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_equality()?;
        while self.match_kind(TokenKind::Ampersand) {
            let right = self.parse_equality()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::BitAnd,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_comparison()?;
        loop {
            let op = if self.match_kind(TokenKind::EqEq) {
                Some(BinaryOp::Eq)
            } else if self.match_kind(TokenKind::NotEq) {
                Some(BinaryOp::Ne)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_comparison()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_shift()?;
        loop {
            let op = if self.match_kind(TokenKind::Lt) {
                Some(BinaryOp::Lt)
            } else if self.match_kind(TokenKind::Lte) {
                Some(BinaryOp::Lte)
            } else if self.match_kind(TokenKind::Gt) {
                Some(BinaryOp::Gt)
            } else if self.match_kind(TokenKind::Gte) {
                Some(BinaryOp::Gte)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_shift()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_shift(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_term()?;
        loop {
            let op = if self.match_kind(TokenKind::ShiftLeft) {
                Some(BinaryOp::Shl)
            } else if self.match_kind(TokenKind::ShiftRight) {
                Some(BinaryOp::Shr)
            } else {
                None
            };

            let Some(op) = op else {
                break;
            };

            let right = self.parse_term()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_factor()?;
        loop {
            let op = if self.match_kind(TokenKind::Plus) {
                Some(BinaryOp::Add)
            } else if self.match_kind(TokenKind::Minus) {
                Some(BinaryOp::Sub)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_factor()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_unary()?;
        loop {
            let op = if self.match_kind(TokenKind::Star) {
                Some(BinaryOp::Mul)
            } else if self.match_kind(TokenKind::Slash) {
                Some(BinaryOp::Div)
            } else if self.match_kind(TokenKind::Percent) {
                Some(BinaryOp::Mod)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_unary()?;
            let span = expr.span();
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
                span,
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.match_kind(TokenKind::KeywordNot) {
            let span = self.previous().span;
            let rhs = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                rhs: Box::new(rhs),
                span,
            });
        }

        if self.match_kind(TokenKind::Minus) {
            let span = self.previous().span;
            let rhs = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                rhs: Box::new(rhs),
                span,
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.advance().clone();
        match tok.kind {
            TokenKind::Number => {
                let value = parse_int_literal(&tok.lexeme)
                    .ok_or_else(|| ParseError::new(&tok, "Invalid integer literal"))?;
                Ok(Expr::Number(value, tok.span))
            }
            TokenKind::String => Ok(Expr::String(tok.lexeme, tok.span)),
            TokenKind::KeywordTrue => Ok(Expr::Bool(true, tok.span)),
            TokenKind::KeywordFalse => Ok(Expr::Bool(false, tok.span)),
            TokenKind::KeywordMaybe => Ok(Expr::Maybe(tok.span)),
            TokenKind::Identifier | TokenKind::Register => {
                let name = self.parse_qualified_name(tok.lexeme)?;
                if self.match_kind(TokenKind::LParen) {
                    let args = self.parse_call_args()?;
                    Ok(Expr::Call {
                        name,
                        args,
                        span: tok.span,
                    })
                } else if name.contains('.') {
                    Err(ParseError::new(
                        self.peek(),
                        "Qualified names must be used as function calls",
                    ))
                } else {
                    Ok(Expr::Var(name, tok.span))
                }
            }
            TokenKind::LParen => {
                let expr = self.parse_expression()?;
                self.expect(TokenKind::RParen, "Expected `)`")?;
                Ok(expr)
            }
            _ => Err(ParseError::new(&tok, "Unexpected token in expression")),
        }
    }

    fn expect(&mut self, kind: TokenKind, message: &str) -> Result<Token, ParseError> {
        if self.check(kind.clone()) {
            Ok(self.advance().clone())
        } else {
            Err(ParseError::new(self.peek(), message))
        }
    }

    fn expect_identifier_like(&mut self, message: &str) -> Result<Token, ParseError> {
        if self.check(TokenKind::Identifier) || self.check(TokenKind::Register) {
            Ok(self.advance().clone())
        } else {
            Err(ParseError::new(self.peek(), message))
        }
    }

    fn parse_qualified_name(&mut self, base: String) -> Result<String, ParseError> {
        let mut name = base;
        while self.match_kind(TokenKind::Dot) {
            let seg = self
                .expect_identifier_like("Expected identifier after `.`")?
                .lexeme;
            name.push('.');
            name.push_str(&seg);
        }
        Ok(name)
    }

    fn parse_instruction_target(&mut self) -> Result<String, ParseError> {
        if self.match_kind(TokenKind::LBracket) {
            let name = self
                .expect_identifier_like("Expected memory target name inside `[` `]`")?
                .lexeme;
            self.expect(TokenKind::RBracket, "Expected `]` after memory target")?;
            return Ok(format!("[{}]", name));
        }

        Ok(self
            .expect_identifier_like("Expected instruction target")?
            .lexeme)
    }

    fn parse_cycle_contract_spec(&mut self) -> Result<CycleContract, ParseError> {
        self.expect(TokenKind::Colon, "Expected `:` after `contract`")?;
        self.expect(TokenKind::Newline, "Expected newline after `contract:`")?;
        self.expect(TokenKind::Indent, "Expected indented contract block")?;

        let mut cycles: Option<u64> = None;
        let mut energy_nj: Option<u64> = None;
        let mut on_underflow: Option<ContractPolicy> = None;
        let mut on_overflow: Option<ContractPolicy> = None;

        while !self.check(TokenKind::Dedent) && !self.is_at_end() {
            self.skip_newlines();
            if self.check(TokenKind::Dedent) {
                break;
            }

            let key_tok = self.advance().clone();
            if key_tok.kind != TokenKind::YamlKey && key_tok.kind != TokenKind::Identifier {
                return Err(ParseError::new(&key_tok, "Expected cycle contract key"));
            }
            let key = key_tok.lexeme;
            self.expect(TokenKind::Colon, "Expected `:` after contract key")?;

            match key.as_str() {
                "cycles" => {
                    let expr = self.parse_expression()?;
                    let Expr::Number(value, _) = expr else {
                        return Err(ParseError::new(
                            self.peek(),
                            "Contract `cycles` must be an integer literal",
                        ));
                    };
                    if value <= 0 {
                        return Err(ParseError::new(
                            self.peek(),
                            "Contract `cycles` must be > 0",
                        ));
                    }
                    cycles = Some(value as u64);
                }
                "on_underflow" => {
                    let policy = self.parse_contract_policy("on_underflow")?;
                    on_underflow = Some(policy);
                }
                "on_overflow" => {
                    let policy = self.parse_contract_policy("on_overflow")?;
                    on_overflow = Some(policy);
                }
                "energy_nj" => {
                    let expr = self.parse_expression()?;
                    let Expr::Number(value, _) = expr else {
                        return Err(ParseError::new(
                            self.peek(),
                            "Contract `energy_nj` must be an integer literal",
                        ));
                    };
                    if value < 0 {
                        return Err(ParseError::new(
                            self.peek(),
                            "Contract `energy_nj` must be >= 0",
                        ));
                    }
                    energy_nj = Some(value as u64);
                }
                _ => {
                    return Err(ParseError::new(
                        self.peek(),
                        format!("Unknown contract key `{}`", key),
                    ));
                }
            }

            self.consume_newline_if_present();
        }

        self.expect(TokenKind::Dedent, "Expected end of contract block")?;

        Ok(CycleContract {
            cycles: cycles.ok_or_else(|| {
                ParseError::new(self.peek(), "Contract requires `cycles` key")
            })?,
            energy_nj,
            on_underflow: on_underflow.ok_or_else(|| {
                ParseError::new(self.peek(), "Contract requires `on_underflow` key")
            })?,
            on_overflow: on_overflow.ok_or_else(|| {
                ParseError::new(self.peek(), "Contract requires `on_overflow` key")
            })?,
        })
    }

    fn parse_contract_policy(&mut self, key: &str) -> Result<ContractPolicy, ParseError> {
        let tok = self.advance().clone();
        let TokenKind::String = tok.kind else {
            return Err(ParseError::new(
                &tok,
                format!("Contract `{}` must be a string literal", key),
            ));
        };

        match tok.lexeme.as_str() {
            "pad_nop" => Ok(ContractPolicy::PadNop),
            "compile_error" => Ok(ContractPolicy::CompileError),
            other => Err(ParseError::new(
                &tok,
                format!(
                    "Invalid contract policy `{}` for `{}`; expected `pad_nop` or `compile_error`",
                    other, key
                ),
            )),
        }
    }

    fn match_kind(&mut self, kind: TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, kind: TokenKind) -> bool {
        !self.is_at_end() && self.peek().kind == kind
    }

    fn consume_newline_if_present(&mut self) {
        if self.check(TokenKind::Newline) {
            self.advance();
        }
    }

    fn consume_stmt_terminator(&mut self) {
        if self.check(TokenKind::Semicolon) {
            self.advance();
        }
        self.consume_newline_if_present();
    }

    fn skip_newlines(&mut self) {
        while self.check(TokenKind::Newline) {
            self.advance();
        }
    }

    fn is_at_end(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or_else(|| {
            self.tokens
                .last()
                .expect("token stream should always contain EOF")
        })
    }

    fn previous(&self) -> &Token {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .unwrap_or_else(|| self.tokens.first().expect("token stream should not be empty"))
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.pos += 1;
        }
        self.tokens
            .get(self.pos.saturating_sub(1))
            .unwrap_or_else(|| self.tokens.last().expect("token stream should contain EOF"))
    }
}

pub fn parse_source(source: &str) -> Result<Program, ParseError> {
    let mut parser = Parser::from_source(source)?;
    parser.parse_program()
}

pub fn parse_source_from_path(path: &Path) -> Result<Program, ParseError> {
    let root = canonicalize_existing(path)?;
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    parse_source_from_path_inner(&root, &mut visited, &mut stack)
}

fn parse_source_from_path_inner(
    path: &Path,
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
) -> Result<Program, ParseError> {
    if stack.iter().any(|p| p == path) {
        let mut chain = stack
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        chain.push(path.to_string_lossy().to_string());
        return Err(ParseError {
            line: 1,
            column: 1,
            message: format!("Import cycle detected: {}", chain.join(" -> ")),
        });
    }

    if visited.contains(path) {
        return Ok(Program {
            data: BTreeMap::new(),
            imports: Vec::new(),
            import_spans: Vec::new(),
            functions: Vec::new(),
            source: String::new(),
        });
    }

    visited.insert(path.to_path_buf());
    stack.push(path.to_path_buf());

    let source = fs::read_to_string(path).map_err(|e| ParseError {
        line: 1,
        column: 1,
        message: format!("Failed to read `{}`: {}", path.display(), e),
    })?;
    let mut local = parse_source(&source)?;

    let mut merged = Program {
        data: BTreeMap::new(),
        imports: Vec::new(),
        import_spans: Vec::new(),
        functions: Vec::new(),
        source: local.source.clone(),
    };

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for (idx, import) in local.imports.iter().enumerate() {
        let span = local
            .import_spans
            .get(idx)
            .copied()
            .unwrap_or(crate::token::Span { line: 1, column: 1 });
        if is_file_import_spec(import) {
            let child = resolve_import_path(parent, import)?;
            let child_program = parse_source_from_path_inner(&child, visited, stack)?;
            merge_programs(&mut merged, child_program)?;
        } else if !merged.imports.iter().any(|m| m == import) {
            merged.imports.push(import.clone());
            merged.import_spans.push(span);
        }
    }

    // Path imports are resolved recursively here, so only canonicalized module imports
    // should remain in the merged program import list.
    local.imports.clear();
    local.import_spans.clear();

    merge_programs(&mut merged, std::mem::replace(&mut local, Program {
        data: BTreeMap::new(),
        imports: Vec::new(),
        import_spans: Vec::new(),
        functions: Vec::new(),
        source: String::new(),
    }))?;

    stack.pop();
    Ok(merged)
}

fn merge_programs(target: &mut Program, mut incoming: Program) -> Result<(), ParseError> {
    for (name, expr) in incoming.data {
        if target.data.contains_key(&name) {
            return Err(ParseError {
                line: 1,
                column: 1,
                message: format!("Duplicate data symbol `{}` across imported modules", name),
            });
        }
        target.data.insert(name, expr);
    }

    for module in incoming.imports.drain(..) {
        if !target.imports.iter().any(|m| m == &module) {
            target.imports.push(module);
            target.import_spans.push(crate::token::Span { line: 1, column: 1 });
        }
    }

    for function in incoming.functions {
        if target.functions.iter().any(|f| f.name == function.name) {
            return Err(ParseError {
                line: function.span.line,
                column: function.span.column,
                message: format!("Duplicate function `{}` across imported modules", function.name),
            });
        }
        target.functions.push(function);
    }

    Ok(())
}

fn is_file_import_spec(spec: &str) -> bool {
    spec.ends_with(".hl") || spec.contains('/') || spec.contains('\\') || spec.starts_with('.')
}

fn resolve_import_path(base_dir: &Path, spec: &str) -> Result<PathBuf, ParseError> {
    let raw = Path::new(spec);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base_dir.join(raw)
    };
    canonicalize_existing(&joined)
}

fn canonicalize_existing(path: &Path) -> Result<PathBuf, ParseError> {
    fs::canonicalize(path).map_err(|e| ParseError {
        line: 1,
        column: 1,
        message: format!("Failed to resolve path `{}`: {}", path.display(), e),
    })
}

fn parse_int_literal(lexeme: &str) -> Option<i64> {
    if let Some(hex) = lexeme
        .strip_prefix("0x")
        .or_else(|| lexeme.strip_prefix("0X"))
    {
        i64::from_str_radix(hex, 16).ok()
    } else {
        lexeme.parse::<i64>().ok()
    }
}
