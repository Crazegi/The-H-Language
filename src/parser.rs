use std::collections::BTreeMap;

use crate::ast::{BinaryOp, Expr, Function, Instruction, Program, Stmt, UnaryOp};
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
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn from_source(source: &str) -> Result<Self, ParseError> {
        let mut lexer = Lexer::new(source);
        let tokens = lexer.tokenize()?;
        Ok(Self { tokens, pos: 0 })
    }

    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let mut data = BTreeMap::new();
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
                        functions.push(self.parse_function()?);
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

        Ok(Program { data, functions })
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        self.expect(TokenKind::KeywordFn, "Expected `fn`")?;
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
        self.expect(TokenKind::Colon, "Expected `:` after function signature")?;
        self.expect(TokenKind::Newline, "Expected newline after function header")?;
        self.expect(TokenKind::Indent, "Expected indented function body")?;

        let mut body = Vec::new();
        while !self.check(TokenKind::Dedent) && !self.is_at_end() {
            self.skip_newlines();
            if self.check(TokenKind::Dedent) {
                break;
            }
            body.push(self.parse_statement()?);
        }

        self.expect(TokenKind::Dedent, "Expected end of function body")?;
        Ok(Function { name, params, body })
    }

    fn parse_statement(&mut self) -> Result<Stmt, ParseError> {
        if self.match_kind(TokenKind::KeywordOwn) {
            let name = self.expect_identifier_like("Expected identifier after `own`")?.lexeme;
            self.expect(TokenKind::Assign, "Expected `=` in own declaration")?;
            let expr = self.parse_expression()?;
            self.consume_newline_if_present();
            return Ok(Stmt::OwnDecl { name, expr });
        }

        if self.match_kind(TokenKind::KeywordRef) {
            let name = self.expect_identifier_like("Expected identifier after `ref`")?.lexeme;
            self.expect(TokenKind::Assign, "Expected `=` in ref declaration")?;
            self.expect(TokenKind::Ampersand, "Expected `&` in ref declaration")?;
            let target = self.expect_identifier_like("Expected referenced identifier")?.lexeme;
            self.consume_newline_if_present();
            return Ok(Stmt::RefDecl { name, target });
        }

        if self.match_kind(TokenKind::KeywordIf) {
            let condition = self.parse_expression()?;
            self.expect(TokenKind::Colon, "Expected `:` after if condition")?;
            self.expect(TokenKind::Newline, "Expected newline after if header")?;
            self.expect(TokenKind::Indent, "Expected indented if body")?;
            let mut then_body = Vec::new();
            while !self.check(TokenKind::Dedent) && !self.is_at_end() {
                self.skip_newlines();
                if self.check(TokenKind::Dedent) {
                    break;
                }
                then_body.push(self.parse_statement()?);
            }
            self.expect(TokenKind::Dedent, "Expected end of if body")?;

            let mut else_body = Vec::new();
            if self.match_kind(TokenKind::KeywordElse) {
                self.expect(TokenKind::Colon, "Expected `:` after else")?;
                self.expect(TokenKind::Newline, "Expected newline after else")?;
                self.expect(TokenKind::Indent, "Expected indented else body")?;
                while !self.check(TokenKind::Dedent) && !self.is_at_end() {
                    self.skip_newlines();
                    if self.check(TokenKind::Dedent) {
                        break;
                    }
                    else_body.push(self.parse_statement()?);
                }
                self.expect(TokenKind::Dedent, "Expected end of else body")?;
            }

            return Ok(Stmt::If {
                condition,
                then_body,
                else_body,
            });
        }

        if self.match_kind(TokenKind::KeywordWhile) {
            let condition = self.parse_expression()?;
            self.expect(TokenKind::Colon, "Expected `:` after while condition")?;
            self.expect(TokenKind::Newline, "Expected newline after while")?;
            self.expect(TokenKind::Indent, "Expected indented while body")?;
            let mut body = Vec::new();
            while !self.check(TokenKind::Dedent) && !self.is_at_end() {
                self.skip_newlines();
                if self.check(TokenKind::Dedent) {
                    break;
                }
                body.push(self.parse_statement()?);
            }
            self.expect(TokenKind::Dedent, "Expected end of while body")?;
            return Ok(Stmt::While { condition, body });
        }

        if self.match_kind(TokenKind::KeywordPrint) {
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
            return Ok(Stmt::PrintBlock(fields));
        }

        if self.match_kind(TokenKind::KeywordReturn) {
            if self.check(TokenKind::Newline) {
                self.advance();
                return Ok(Stmt::Return(None));
            }
            let expr = self.parse_expression()?;
            self.consume_newline_if_present();
            return Ok(Stmt::Return(Some(expr)));
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
            let target = self.expect_identifier_like("Expected instruction target")?.lexeme;
            self.expect(TokenKind::Comma, "Expected `,` after instruction target")?;
            let rhs = self.parse_expression()?;
            self.consume_newline_if_present();
            return Ok(Stmt::Instruction { op, target, rhs });
        }

        if self.check(TokenKind::Identifier) || self.check(TokenKind::Register) {
            let name = self.advance().lexeme.clone();
            if self.match_kind(TokenKind::Assign) {
                let expr = self.parse_expression()?;
                self.consume_newline_if_present();
                return Ok(Stmt::Assign { name, expr });
            }
            if self.match_kind(TokenKind::LParen) {
                let args = self.parse_call_args()?;
                self.consume_newline_if_present();
                return Ok(Stmt::Expr(Expr::Call { name, args }));
            }
            return Err(ParseError::new(
                self.peek(),
                "Expected assignment (`=`) or function call",
            ));
        }

        let expr = self.parse_expression()?;
        self.consume_newline_if_present();
        Ok(Stmt::Expr(expr))
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
        self.parse_equality()
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
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_term()?;
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
            let right = self.parse_term()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
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
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
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
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.match_kind(TokenKind::Minus) {
            let rhs = self.parse_unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                rhs: Box::new(rhs),
            });
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let tok = self.advance().clone();
        match tok.kind {
            TokenKind::Number => {
                let value = tok
                    .lexeme
                    .parse::<i64>()
                    .map_err(|_| ParseError::new(&tok, "Invalid integer literal"))?;
                Ok(Expr::Number(value))
            }
            TokenKind::String => Ok(Expr::String(tok.lexeme)),
            TokenKind::KeywordTrue => Ok(Expr::Bool(true)),
            TokenKind::KeywordFalse => Ok(Expr::Bool(false)),
            TokenKind::Identifier | TokenKind::Register => {
                let name = tok.lexeme;
                if self.match_kind(TokenKind::LParen) {
                    let args = self.parse_call_args()?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Var(name))
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
