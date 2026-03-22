use std::collections::VecDeque;

use crate::error::LexerError;
use crate::token::{Token, TokenKind};

const MNEMONICS: &[&str] = &[
    "add", "mov", "cmp", "sub", "mul", "div", "mod", "jmp", "jne", "je", "call", "ret",
];

#[derive(Debug, Clone)]
pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    column: usize,
    at_line_start: bool,
    line_has_content: bool,
    indent_stack: Vec<usize>,
    pending: VecDeque<Token>,
    reached_eof: bool,
    pending_yaml_block: bool,
    in_yaml_block: bool,
    yaml_indent: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
        Self {
            chars: normalized.chars().collect(),
            pos: 0,
            line: 1,
            column: 1,
            at_line_start: true,
            line_has_content: false,
            indent_stack: vec![0],
            pending: VecDeque::new(),
            reached_eof: false,
            pending_yaml_block: false,
            in_yaml_block: false,
            yaml_indent: 0,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();
        loop {
            let t = self.next_token()?;
            let end = t.kind == TokenKind::Eof;
            tokens.push(t);
            if end {
                break;
            }
        }
        Ok(tokens)
    }

    pub fn next_token(&mut self) -> Result<Token, LexerError> {
        if let Some(tok) = self.pending.pop_front() {
            return Ok(tok);
        }

        if self.reached_eof {
            return Ok(Token::new(TokenKind::Eof, "", self.line, self.column));
        }

        loop {
            if self.at_line_start {
                self.process_line_start()?;
                if let Some(tok) = self.pending.pop_front() {
                    return Ok(tok);
                }
            }

            let ch = match self.current() {
                Some(ch) => ch,
                None => {
                    let eof_line = self.line;
                    let eof_col = self.column;
                    while self.indent_stack.len() > 1 {
                        self.indent_stack.pop();
                        self.pending
                            .push_back(Token::new(TokenKind::Dedent, "", eof_line, eof_col));
                    }
                    self.pending
                        .push_back(Token::new(TokenKind::Eof, "", eof_line, eof_col));
                    self.reached_eof = true;
                    return Ok(self.pending.pop_front().expect("pending EOF token"));
                }
            };

            if ch == ' ' || ch == '\t' {
                self.advance();
                continue;
            }

            if ch == '/' && self.peek_char() == Some('/') {
                self.consume_comment();
                continue;
            }

            if ch == '\n' {
                let line = self.line;
                let col = self.column;
                self.advance();
                self.at_line_start = true;
                self.line_has_content = false;
                return Ok(Token::new(TokenKind::Newline, "\\n", line, col));
            }

            let token = match ch {
                ':' => {
                    let tok = Token::new(TokenKind::Colon, ":", self.line, self.column);
                    self.advance();
                    tok
                }
                ',' => {
                    let tok = Token::new(TokenKind::Comma, ",", self.line, self.column);
                    self.advance();
                    tok
                }
                '.' => {
                    let tok = Token::new(TokenKind::Dot, ".", self.line, self.column);
                    self.advance();
                    tok
                }
                '=' => self.scan_equal_like(),
                '!' => self.scan_bang_like()?,
                '+' => {
                    let tok = Token::new(TokenKind::Plus, "+", self.line, self.column);
                    self.advance();
                    tok
                }
                '-' => {
                    let tok = Token::new(TokenKind::Minus, "-", self.line, self.column);
                    self.advance();
                    tok
                }
                '*' => {
                    let tok = Token::new(TokenKind::Star, "*", self.line, self.column);
                    self.advance();
                    tok
                }
                '/' => {
                    let tok = Token::new(TokenKind::Slash, "/", self.line, self.column);
                    self.advance();
                    tok
                }
                '%' => {
                    let tok = Token::new(TokenKind::Percent, "%", self.line, self.column);
                    self.advance();
                    tok
                }
                '|' => {
                    let tok = Token::new(TokenKind::Pipe, "|", self.line, self.column);
                    self.advance();
                    tok
                }
                '<' => self.scan_lt_like(),
                '>' => self.scan_gt_like(),
                '&' => {
                    let tok = Token::new(TokenKind::Ampersand, "&", self.line, self.column);
                    self.advance();
                    tok
                }
                '(' => {
                    let tok = Token::new(TokenKind::LParen, "(", self.line, self.column);
                    self.advance();
                    tok
                }
                ')' => {
                    let tok = Token::new(TokenKind::RParen, ")", self.line, self.column);
                    self.advance();
                    tok
                }
                '[' => {
                    let tok = Token::new(TokenKind::LBracket, "[", self.line, self.column);
                    self.advance();
                    tok
                }
                ']' => {
                    let tok = Token::new(TokenKind::RBracket, "]", self.line, self.column);
                    self.advance();
                    tok
                }
                ';' => {
                    let tok = Token::new(TokenKind::Semicolon, ";", self.line, self.column);
                    self.advance();
                    tok
                }
                '"' => self.scan_string()?,
                c if c.is_ascii_digit() => self.scan_number()?,
                c if is_ident_start(c) => self.scan_word(),
                other => {
                    return Err(LexerError::new(
                        self.line,
                        self.column,
                        format!("Illegal character '{}'", other),
                    ));
                }
            };

            self.line_has_content = true;
            return Ok(token);
        }
    }

    pub fn peek_token(&self) -> Result<Token, LexerError> {
        let mut cloned = self.clone();
        cloned.next_token()
    }

    fn process_line_start(&mut self) -> Result<(), LexerError> {
        self.at_line_start = false;

        let start_pos = self.pos;
        let mut spaces = 0usize;

        while let Some(ch) = self.current() {
            match ch {
                ' ' => {
                    spaces += 1;
                    self.advance();
                }
                '\t' => {
                    return Err(LexerError::new(
                        self.line,
                        self.column,
                        "Tabs are not allowed for indentation; use spaces only",
                    ));
                }
                _ => break,
            }
        }

        match self.current() {
            Some('\n') => {
                self.pos = start_pos;
                self.column = 1;
                return Ok(());
            }
            Some('/') if self.peek_char() == Some('/') => {
                self.pos = start_pos;
                self.column = 1;
                return Ok(());
            }
            None => return Ok(()),
            _ => {}
        }

        let current_indent = *self.indent_stack.last().expect("indent stack non-empty");

        if self.in_yaml_block && spaces < self.yaml_indent {
            self.in_yaml_block = false;
            self.yaml_indent = 0;
        }

        if spaces > current_indent {
            self.indent_stack.push(spaces);
            self.pending
                .push_back(Token::new(TokenKind::Indent, "", self.line, 1));
            if self.pending_yaml_block {
                self.in_yaml_block = true;
                self.yaml_indent = spaces;
                self.pending_yaml_block = false;
            }
        } else if spaces < current_indent {
            while let Some(last) = self.indent_stack.last().copied() {
                if last <= spaces {
                    break;
                }
                self.indent_stack.pop();
                self.pending
                    .push_back(Token::new(TokenKind::Dedent, "", self.line, 1));
            }

            if *self.indent_stack.last().expect("indent stack non-empty") != spaces {
                return Err(LexerError::new(
                    self.line,
                    1,
                    format!("Inconsistent indentation: {} spaces does not match any outer block", spaces),
                ));
            }
        } else if self.pending_yaml_block {
            return Err(LexerError::new(
                self.line,
                1,
                "Expected an indented YAML block after `print:`",
            ));
        }

        self.line_has_content = false;
        Ok(())
    }

    fn scan_word(&mut self) -> Token {
        let line = self.line;
        let col = self.column;
        let mut s = String::new();
        while let Some(ch) = self.current() {
            if is_ident_continue(ch) {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }

        let lower = s.to_ascii_lowercase();

        let mut kind = match lower.as_str() {
            "section" => TokenKind::KeywordSection,
            "fn" => TokenKind::KeywordFn,
            "import" | "textimport" => TokenKind::KeywordImport,
            "interrupt" => TokenKind::KeywordInterrupt,
            "own" => TokenKind::KeywordOwn,
            "ref" => TokenKind::KeywordRef,
            "yield" => TokenKind::KeywordYield,
            "to" => TokenKind::KeywordTo,
            "print" => TokenKind::KeywordPrint,
            "contract" => TokenKind::KeywordContract,
            "execute" => TokenKind::KeywordExecute,
            "if" => TokenKind::KeywordIf,
            "else" => TokenKind::KeywordElse,
            "while" => TokenKind::KeywordWhile,
            "repeat" => TokenKind::KeywordRepeat,
            "for" => TokenKind::KeywordFor,
            "in" => TokenKind::KeywordIn,
            "return" => TokenKind::KeywordReturn,
            "true" => TokenKind::KeywordTrue,
            "false" => TokenKind::KeywordFalse,
            "maybe" => TokenKind::KeywordMaybe,
            "and" => TokenKind::KeywordAnd,
            "or" => TokenKind::KeywordOr,
            "xor" => TokenKind::KeywordXor,
            "not" => TokenKind::KeywordNot,
            "int" => TokenKind::KeywordInt,
            "string" => TokenKind::KeywordString,
            "bool" => TokenKind::KeywordBool,
            "const" => TokenKind::KeywordConst,
            "unused" => TokenKind::KeywordUnused,
            _ if is_register(&s) => TokenKind::Register,
            _ if MNEMONICS.contains(&lower.as_str()) => TokenKind::Mnemonic,
            _ => TokenKind::Identifier,
        };

        if self.in_yaml_block && !self.line_has_content && matches!(kind, TokenKind::Identifier) {
            let mut i = self.pos;
            while let Some(ch) = self.chars.get(i) {
                if *ch == ' ' {
                    i += 1;
                    continue;
                }
                if *ch == ':' {
                    kind = TokenKind::YamlKey;
                }
                break;
            }
        }

        let token = Token::new(kind, s, line, col);

        if token.kind == TokenKind::KeywordPrint || token.kind == TokenKind::KeywordContract {
            let mut i = self.pos;
            while let Some(ch) = self.chars.get(i) {
                if *ch == ' ' {
                    i += 1;
                    continue;
                }
                if *ch == ':' {
                    self.pending_yaml_block = true;
                }
                break;
            }
        }

        token
    }

    fn scan_number(&mut self) -> Result<Token, LexerError> {
        let line = self.line;
        let col = self.column;

        if self.current() == Some('0') && matches!(self.peek_char(), Some('x') | Some('X')) {
            let mut s = String::from("0");
            self.advance();
            s.push(self.current().expect("hex prefix must exist"));
            self.advance();

            let mut digits = 0usize;
            while let Some(ch) = self.current() {
                if ch.is_ascii_hexdigit() {
                    s.push(ch);
                    self.advance();
                    digits += 1;
                } else {
                    break;
                }
            }

            if digits == 0 {
                return Err(LexerError::new(
                    line,
                    col,
                    "Invalid hexadecimal literal; expected digits after 0x",
                ));
            }

            return Ok(Token::new(TokenKind::Number, s, line, col));
        }

        let mut s = String::new();
        while let Some(ch) = self.current() {
            if ch.is_ascii_digit() {
                s.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        Ok(Token::new(TokenKind::Number, s, line, col))
    }

    fn scan_string(&mut self) -> Result<Token, LexerError> {
        let line = self.line;
        let col = self.column;
        let mut out = String::new();
        self.advance();

        while let Some(ch) = self.current() {
            match ch {
                '"' => {
                    self.advance();
                    return Ok(Token::new(TokenKind::String, out, line, col));
                }
                '\\' => {
                    self.advance();
                    let escaped = self.current().ok_or_else(|| {
                        LexerError::new(line, col, "Unterminated escape sequence in string")
                    })?;
                    let value = match escaped {
                        'n' => '\n',
                        't' => '\t',
                        '"' => '"',
                        '\\' => '\\',
                        other => {
                            return Err(LexerError::new(
                                self.line,
                                self.column,
                                format!("Unsupported escape sequence \\{}", other),
                            ));
                        }
                    };
                    out.push(value);
                    self.advance();
                }
                '\n' => {
                    return Err(LexerError::new(line, col, "Unterminated string literal"));
                }
                c => {
                    out.push(c);
                    self.advance();
                }
            }
        }

        Err(LexerError::new(line, col, "Unterminated string literal"))
    }

    fn consume_comment(&mut self) {
        while let Some(ch) = self.current() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn scan_equal_like(&mut self) -> Token {
        let line = self.line;
        let col = self.column;
        self.advance();
        if self.current() == Some('=') {
            self.advance();
            Token::new(TokenKind::EqEq, "==", line, col)
        } else {
            Token::new(TokenKind::Assign, "=", line, col)
        }
    }

    fn scan_bang_like(&mut self) -> Result<Token, LexerError> {
        let line = self.line;
        let col = self.column;
        self.advance();
        if self.current() == Some('=') {
            self.advance();
            Ok(Token::new(TokenKind::NotEq, "!=", line, col))
        } else {
            Err(LexerError::new(
                line,
                col,
                "Unexpected '!' (did you mean '!=')",
            ))
        }
    }

    fn scan_lt_like(&mut self) -> Token {
        let line = self.line;
        let col = self.column;
        self.advance();
        if self.current() == Some('<') {
            self.advance();
            Token::new(TokenKind::ShiftLeft, "<<", line, col)
        } else if self.current() == Some('=') {
            self.advance();
            Token::new(TokenKind::Lte, "<=", line, col)
        } else {
            Token::new(TokenKind::Lt, "<", line, col)
        }
    }

    fn scan_gt_like(&mut self) -> Token {
        let line = self.line;
        let col = self.column;
        self.advance();
        if self.current() == Some('>') {
            self.advance();
            Token::new(TokenKind::ShiftRight, ">>", line, col)
        } else if self.current() == Some('=') {
            self.advance();
            Token::new(TokenKind::Gte, ">=", line, col)
        } else {
            Token::new(TokenKind::Gt, ">", line, col)
        }
    }

    fn current(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_char(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn advance(&mut self) {
        if let Some(ch) = self.current() {
            self.pos += 1;
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_continue(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_register(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some('r') | Some('R') => chars.all(|c| c.is_ascii_digit()) && s.len() > 1,
        _ => false,
    }
}
