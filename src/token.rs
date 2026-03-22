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
    KeywordOwn,
    KeywordRef,
    KeywordPrint,
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
    Ampersand,
    LParen,
    RParen,
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
            KeywordOwn => "KEYWORD_OWN",
            KeywordRef => "KEYWORD_REF",
            KeywordPrint => "KEYWORD_PRINT",
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
            Ampersand => "AMPERSAND",
            LParen => "LPAREN",
            RParen => "RPAREN",
        };
        write!(f, "{}", label)
    }
}
