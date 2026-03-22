# The H Language Prototype

This repository now contains a complete end-to-end prototype for H (`.hl`) in Rust:

- Lexer (indentation-sensitive, YAML-style blocks)
- AST and recursive descent parser
- Semantic analyzer (ownership/reference checks + symbol/function checks)
- Evaluator/runtime

## Implemented Language Features

- Sections: `section .data:` and `section .text:`
- Functions with parameters: `fn name(a, b):`
- Ownership and borrowing:
	- `own r1 = 45`
	- `ref alias = &r1`
- Assembly-flavored instructions:
	- `mov`, `add`, `sub`, `mul`, `div`, `mod`, `cmp`
- Expressions:
	- arithmetic: `+ - * / %`
	- comparisons: `== != < <= > >=`
	- function calls inside expressions
- Control flow:
	- `if/else`
	- `while`
	- `return`
- Structured native print blocks:
	- `print:` followed by YAML-style key-value lines

## CLI Modes

Token stream:

```powershell
cargo run --bin hl-lex -- --tokens examples/advanced.hl
```

AST dump:

```powershell
cargo run --bin hl-lex -- --ast examples/advanced.hl
```

Run program:

```powershell
cargo run --bin hl-lex -- examples/advanced.hl
```

Run tests:

```powershell
cargo test
```
