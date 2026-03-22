# The H Language Prototype

This repository now contains a complete end-to-end prototype for H (`.hl`) in Rust:

- Lexer (indentation-sensitive, YAML-style blocks)
- AST and recursive descent parser
- Semantic analyzer (ownership/reference checks + symbol/function checks)
- Interpreter runtime
- Compiler backend (AST -> bytecode)
- Bytecode VM

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

Interpret source program:

```powershell
cargo run --bin hl-lex -- examples/advanced.hl
```

Compile to bytecode listing:

```powershell
cargo run --bin hl-lex -- --compile examples/advanced.hl --out out.hbc.txt
```

Compile H source into a native executable binary:

```powershell
cargo run --bin hl-lex -- --native examples/advanced.hl --out advanced.exe
```

Run compiled bytecode in VM:

```powershell
cargo run --bin hl-lex -- --vm examples/advanced.hl
```

Run tests:

```powershell
cargo test
```
