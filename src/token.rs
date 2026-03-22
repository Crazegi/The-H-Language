use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Indent,
    Dedent,
    Newline,
    Eof,
    KeywordSection,
    KeywordFn,
    KeywordInterrupt,
    KeywordOwn,
    KeywordRef,
    KeywordYield,
    KeywordTo,
    KeywordPrint,
    KeywordContract,
    KeywordExecute,
    KeywordIf,
    KeywordElse,
    KeywordWhile,
    KeywordRepeat,
    KeywordReturn,
    KeywordTrue,
    KeywordFalse,
    KeywordMaybe,
    KeywordAnd,
    KeywordOr,
    KeywordXor,
    KeywordNot,
    KeywordInt,
    KeywordString,
    KeywordBool,
    Mnemonic,
    Identifier,
    Register,
    Number,
    String,
    YamlKey,
    Colon,
    Comma,
    Dot,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Ampersand,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Semicolon,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub lexeme: String,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, lexeme: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            kind,
            lexeme: lexeme.into(),
            span: Span { line, column },
        }
    }
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TokenKind::*;
        let label = match self {
            Indent => "INDENT",
            Dedent => "DEDENT",
            Newline => "NEWLINE",
            Eof => "EOF",
            KeywordSection => "KEYWORD_SECTION",
            KeywordFn => "KEYWORD_FN",
            KeywordInterrupt => "KEYWORD_INTERRUPT",
            KeywordOwn => "KEYWORD_OWN",
            KeywordRef => "KEYWORD_REF",
            KeywordYield => "KEYWORD_YIELD",
            KeywordTo => "KEYWORD_TO",
            KeywordPrint => "KEYWORD_PRINT",
            KeywordContract => "KEYWORD_CONTRACT",
            KeywordExecute => "KEYWORD_EXECUTE",
            KeywordIf => "KEYWORD_IF",
            KeywordElse => "KEYWORD_ELSE",
            KeywordWhile => "KEYWORD_WHILE",
            KeywordRepeat => "KEYWORD_REPEAT",
            KeywordReturn => "KEYWORD_RETURN",
            KeywordTrue => "KEYWORD_TRUE",
            KeywordFalse => "KEYWORD_FALSE",
            KeywordMaybe => "KEYWORD_MAYBE",
            KeywordAnd => "KEYWORD_AND",
            KeywordOr => "KEYWORD_OR",
            KeywordXor => "KEYWORD_XOR",
            KeywordNot => "KEYWORD_NOT",
            KeywordInt => "KEYWORD_INT",
            KeywordString => "KEYWORD_STRING",
            KeywordBool => "KEYWORD_BOOL",
            Mnemonic => "MNEMONIC",
            Identifier => "IDENTIFIER",
            Register => "REGISTER",
            Number => "NUMBER",
            String => "STRING",
            YamlKey => "YAML_KEY",
            Colon => "COLON",
            Comma => "COMMA",
            Dot => "DOT",
            Assign => "ASSIGN",
            Plus => "PLUS",
            Minus => "MINUS",
            Star => "STAR",
            Slash => "SLASH",
            Percent => "PERCENT",
            EqEq => "EQEQ",
            NotEq => "NOTEQ",
            Lt => "LT",
            Lte => "LTE",
            Gt => "GT",
            Gte => "GTE",
            Ampersand => "AMPERSAND",
            LParen => "LPAREN",
            RParen => "RPAREN",
            LBracket => "LBRACKET",
            RBracket => "RBRACKET",
            Semicolon => "SEMICOLON",
        };
        write!(f, "{}", label)
    }
}
