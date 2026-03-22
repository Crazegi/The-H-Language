# H Lexer (Phase 1)

This crate contains a production-ready lexer prototype for the H language (`.hl`) written in Rust.

## Implemented in Phase 1

- Indentation-sensitive lexing with `Indent` / `Dedent` / `Newline`
- Keywords: `section`, `fn`, `own`, `ref`, `print`
- Assembly mnemonics: `add`, `mov`, `cmp`, `sub`, `mul`, `div`, `jmp`, `jne`, `je`, `call`, `ret`
- Registers: `r1`, `r2`, ...
- Literals: decimal numbers and double-quoted strings with escapes
- YAML-like print block keys tokenized as `YamlKey`
- Error handling with line/column for illegal characters and malformed input

## Quick Start

```powershell
cargo test
cargo run --bin hl-lex
```

To lex a specific file:

```powershell
cargo run --bin hl-lex -- examples/sample.hl
```
