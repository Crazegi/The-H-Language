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
- Java-style blocks are also supported: `fn name(a, b) { ... }`
- Ownership and borrowing:
	- `own r1 = 45`
	- `ref alias = &r1`
- Java-style typed declarations in function bodies:
	- `int x = 10;`
	- `string name = "engine";`
	- `bool ok = true;`
- Assembly-flavored instructions:
	- `mov`, `add`, `sub`, `mul`, `div`, `mod`, `cmp`
	- memory-mapped operand form for `mov`: `mov [port_a], r1`
- Expressions:
	- arithmetic: `+ - * / %`
	- comparisons: `== != < <= > >=`
	- logic (including tri-state support): `and or xor not`
	- tri-state literal: `maybe`
	- function calls inside expressions
- Builtin math/functions:
	- `abs(x)`, `sqrt(x)`, `pow(base, exp)`
	- `min(a, b)`, `max(a, b)`, `clamp(v, lo, hi)`
	- string helpers: `len(s)`, `upper(s)`, `lower(s)`, `contains(s, part)`
	- exotic logic helpers: `phase(a, b)`, `collapse(v)`
- Control flow:
	- `if/else`
	- `while`
	- `repeat n` (counted loop)
	- `return`
- Cycle Contracts (deterministic execute blocks):
	- `contract:` metadata with `cycles`, `on_underflow`, `on_overflow`
	- `execute:` block for cycle-counted instructions
	- compile-time overflow errors and underflow `nop` padding
	- cycle profiles: `generic`, `avr-like`, `cortex-m0-like`
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

Compile with a cycle profile and emit a cycle contract report artifact:

```powershell
cargo run --bin hl-lex -- --compile examples/advanced.hl --cycle-profile avr-like --contract-report contract_report.txt --out out.hbc.txt
```

Compile H source with object + link pipeline (`.obj` then `.exe`):

```powershell
cargo run --bin hl-lex -- --native examples/advanced.hl --out advanced.exe
```

This mode now emits:
- Object file: `advanced.obj`
- Final executable: `advanced.exe`

Run compiled bytecode in VM:

```powershell
cargo run --bin hl-lex -- --vm examples/advanced.hl --cycle-profile cortex-m0-like
```

Run tests:

```powershell
cargo test
```
