mod fixtures;

use fixtures::SYNTAX_EXAMPLE;
use hl_lexer::{Lexer, TokenKind};

#[test]
fn lexes_reference_program_with_indent_and_yaml_tokens() {
    let mut lexer = Lexer::new(SYNTAX_EXAMPLE);
    let tokens = lexer.tokenize().expect("tokenization should succeed");

    assert!(!tokens.is_empty());
    assert_eq!(tokens.last().map(|t| &t.kind), Some(&TokenKind::Eof));

    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.iter().any(|k| *k == TokenKind::KeywordSection));
    assert!(kinds.iter().any(|k| *k == TokenKind::KeywordFn));
    assert!(kinds.iter().any(|k| *k == TokenKind::KeywordOwn));
    assert!(kinds.iter().any(|k| *k == TokenKind::KeywordRef));
    assert!(kinds.iter().any(|k| *k == TokenKind::KeywordPrint));

    assert!(kinds.iter().any(|k| *k == TokenKind::Mnemonic));
    assert!(kinds.iter().any(|k| *k == TokenKind::Register));
    assert!(kinds.iter().any(|k| *k == TokenKind::YamlKey));

    let indent_count = kinds.iter().filter(|k| **k == TokenKind::Indent).count();
    let dedent_count = kinds.iter().filter(|k| **k == TokenKind::Dedent).count();
    assert_eq!(indent_count, dedent_count, "indent/dedent should balance");
}

#[test]
fn rejects_illegal_character() {
    let src = "section .data:\n  value: @bad\n";
    let mut lexer = Lexer::new(src);
    let err = lexer.tokenize().expect_err("should fail on illegal character");
    assert!(err.message.contains("Illegal character"));
}

#[test]
fn rejects_tab_indentation() {
    let src = "section .data:\n\tname: \"x\"\n";
    let mut lexer = Lexer::new(src);
    let err = lexer.tokenize().expect_err("tabs in indentation must fail");
    assert!(err.message.contains("Tabs are not allowed"));
}

#[test]
fn rejects_unterminated_string() {
    let src = "section .data:\n  name: \"abc\n";
    let mut lexer = Lexer::new(src);
    let err = lexer.tokenize().expect_err("unterminated string must fail");
    assert!(err.message.contains("Unterminated string literal"));
}
