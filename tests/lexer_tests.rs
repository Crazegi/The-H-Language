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
fn rejects_brace_characters() {
    let src = "section .text:\n  fn main() {\n    return 0\n  }\n";
    let mut lexer = Lexer::new(src);
    let err = lexer.tokenize().expect_err("brace characters should fail lexing");
    assert!(err.message.contains("Illegal character '{'"));
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

#[test]
fn lexes_control_flow_operators() {
    let src = "section .text:\n  fn main():\n    own r1 = 2 + 3 * 4\n    if r1 >= 10:\n      return true\n    else:\n      return false\n";
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenization should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.contains(&TokenKind::KeywordIf));
    assert!(kinds.contains(&TokenKind::KeywordElse));
    assert!(kinds.contains(&TokenKind::KeywordReturn));
    assert!(kinds.contains(&TokenKind::KeywordTrue));
    assert!(kinds.contains(&TokenKind::KeywordFalse));
    assert!(kinds.contains(&TokenKind::Plus));
    assert!(kinds.contains(&TokenKind::Star));
    assert!(kinds.contains(&TokenKind::Gte));
}

#[test]
fn lexes_typed_declarations_and_semicolons() {
    let src = "section .text:\n  fn main():\n    int x = 3;\n    string s = \"ok\";\n    bool b = true;\n";
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenization should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.contains(&TokenKind::Colon));
    assert!(kinds.contains(&TokenKind::Semicolon));
    assert!(kinds.contains(&TokenKind::KeywordInt));
    assert!(kinds.contains(&TokenKind::KeywordString));
    assert!(kinds.contains(&TokenKind::KeywordBool));
}

#[test]
fn lexes_unused_modifier_keyword() {
    let src = "section .text:\n  fn main():\n    unused own temp = 1\n";
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenization should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.contains(&TokenKind::KeywordUnused));
    assert!(kinds.contains(&TokenKind::KeywordOwn));
}

#[test]
fn lexes_struct_keyword() {
    let src = "section .text:\n  struct Sensor:\n    value: int\n";
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenization should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.contains(&TokenKind::KeywordStruct));
}

#[test]
fn lexes_cycle_contract_block_and_memory_operands() {
    let src = "section .text:\n  fn hardware_pulse():\n    own r1 = 0x01\n    own r2 = 0x00\n\n    contract:\n      cycles: 16\n      on_underflow: \"pad_nop\"\n      on_overflow: \"compile_error\"\n    execute:\n      mov [port_a], r1\n      add r1, r2\n      mov [port_a], r2\n";

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenization should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.contains(&TokenKind::KeywordContract));
    assert!(kinds.contains(&TokenKind::KeywordExecute));
    assert!(kinds.contains(&TokenKind::LBracket));
    assert!(kinds.contains(&TokenKind::RBracket));
    assert!(kinds.contains(&TokenKind::YamlKey));

    assert!(tokens
        .iter()
        .any(|t| t.kind == TokenKind::Number && t.lexeme == "0x01"));
    assert!(tokens
        .iter()
        .any(|t| t.kind == TokenKind::Number && t.lexeme == "0x00"));
}

#[test]
fn lexes_interrupt_and_yield_keywords() {
    let src = "section .text:\n  interrupt fn emergency_interrupt():\n    mov [port_a], 1\n\n  fn main():\n    own [port_a]\n    yield [port_a] to emergency_interrupt:\n      mov [port_a], 2\n";

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenization should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.contains(&TokenKind::KeywordInterrupt));
    assert!(kinds.contains(&TokenKind::KeywordYield));
    assert!(kinds.contains(&TokenKind::KeywordTo));
}

#[test]
fn lexes_bitwise_operators_and_shifts() {
    let src = "section .text:\n  fn main():\n    own r1 = (1 << 5) | 3\n    own r2 = r1 & 0x1F\n    own r3 = r2 >> 1\n    return r3\n";

    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().expect("tokenization should succeed");
    let kinds: Vec<TokenKind> = tokens.iter().map(|t| t.kind.clone()).collect();

    assert!(kinds.contains(&TokenKind::Pipe));
    assert!(kinds.contains(&TokenKind::Ampersand));
    assert!(kinds.contains(&TokenKind::ShiftLeft));
    assert!(kinds.contains(&TokenKind::ShiftRight));
}
