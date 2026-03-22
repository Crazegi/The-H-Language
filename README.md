# The H Language Prototype

[![Rust](https://img.shields.io/badge/Rust-1.74%2B-000000?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux%20%7C%20macOS-2ea44f)](#)
[![Lexer](https://img.shields.io/badge/pipeline-phase%201%20lexer-0ea5e9)](#)
[![Parser](https://img.shields.io/badge/pipeline-phase%202%20parser-2563eb)](#)
[![Cycle Contracts](https://img.shields.io/badge/flagship-cycle%20contracts-f59e0b)](#cycle-contracts)
[![Cycle Profiles](https://img.shields.io/badge/profiles-generic%20%7C%20avr--like%20%7C%20cortex--m0--like-8b5cf6)](#phase-4-cycle-profiles-and-reports)
[![Native](https://img.shields.io/badge/native-object%20%2B%20link-10b981)](#native-compile)
[![VM](https://img.shields.io/badge/runtime-bytecode%20vm-0f766e)](#run-on-vm)
[![Tests](https://img.shields.io/badge/tests-passing-success)](#)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

H is a hybrid language for deterministic systems work:
- YAML-style structure for clarity.
- Assembly-flavored instructions for low-level intent.
- High-level control flow and expressions for ergonomics.
- Compile-time Cycle Contracts for hard timing guarantees.

## Why These Features Exist (Real-World View)

1. Deterministic I/O pulses for devices:
`contract/execute` lets you guarantee exact timing windows when toggling ports.

2. Embedded safety checks:
`on_overflow: "compile_error"` prevents shipping code that violates a cycle budget.

3. Practical tuning across targets:
cycle profiles model different hardware timing behaviors without rewriting source code.

4. Human-readable diagnostics:
YAML-style report artifacts can be generated and archived in build pipelines.

5. Runtime resilience:
tri-state logic (`maybe`) helps represent uncertain sensor state without unsafe coercions.

## Implemented Features

- Sections: `section .data:` and `section .text:`
- Functions: `fn name(a, b):` and `fn name(a, b) { ... }`
- Ownership/borrowing: `own` and `ref`
- Typed declarations in block syntax: `int`, `string`, `bool`
- Assembly instructions: `mov`, `add`, `sub`, `mul`, `div`, `mod`, `cmp`
- Memory-mapped destination style: `mov [port_a], r1`
- Expressions: arithmetic, comparison, call expressions
- Tri-state logic: `true`, `false`, `maybe`, `and`, `or`, `xor`, `not`
- Builtins:
  - math: `abs`, `sqrt`, `pow`, `min`, `max`, `clamp`
  - string: `len`, `upper`, `lower`, `contains`
  - logic: `phase`, `collapse`
- Control flow: `if/else`, `while`, `repeat`, `return`
- Structured output: YAML-style `print:` blocks

## Cycle Contracts

Contract shape:

```text
contract:
  cycles: 16
  on_underflow: "pad_nop"
  on_overflow: "compile_error"
execute:
  mov [port_a], r1
  add r1, r2
  mov [port_a], r2
```

Rules:
- `execute` currently accepts instruction statements only.
- Underflow can be padded with inserted `nop` instructions.
- Overflow can be rejected as a compile-time error.

## Phase 4: Cycle Profiles And Reports

Available profiles:
- `generic`
- `avr-like`
- `cortex-m0-like`

Profile impact:
- Different targets can assign different cycle costs for the same instruction.
- The same contract can pass in one profile and fail in another.

Contract report output includes:
- function name
- contract index
- selected profile
- declared cycles
- measured cycles
- padded nop count
- final cycle count
- underflow and overflow policies

## Example Programs

- Basic language example: `examples/sample.hl`
- Rich feature example: `examples/advanced.hl`
- Dedicated Cycle Contracts showcase: `examples/cycle_contracts.hl`

Use the contracts showcase to compare profiles on the same source:

```powershell
cargo run --bin hl-lex -- --compile examples/cycle_contracts.hl --cycle-profile generic --contract-report cycle_generic.txt --out cycle_generic.hbc.txt
```

```powershell
cargo run --bin hl-lex -- --compile examples/cycle_contracts.hl --cycle-profile avr-like --contract-report cycle_avr.txt --out cycle_avr.hbc.txt
```

```powershell
cargo run --bin hl-lex -- --compile examples/cycle_contracts.hl --cycle-profile cortex-m0-like --contract-report cycle_m0.txt --out cycle_m0.hbc.txt
```

## CLI Workflows

### Tokenize

```powershell
cargo run --bin hl-lex -- --tokens examples/advanced.hl
```

### Parse To AST

```powershell
cargo run --bin hl-lex -- --ast examples/advanced.hl
```

### Interpret

```powershell
cargo run --bin hl-lex -- examples/advanced.hl
```

### Compile To Bytecode Listing

```powershell
cargo run --bin hl-lex -- --compile examples/advanced.hl --out out.hbc.txt
```

### Compile With Profile And Report

```powershell
cargo run --bin hl-lex -- --compile examples/advanced.hl --cycle-profile avr-like --contract-report contract_report.txt --out out.hbc.txt
```

### Performance And Tuning Options

The compiler now supports speed and behavior tuning flags:

- `--opt-level 0|1|2|3`
  - `0`: minimal optimization
  - `1`: enables constant folding
  - `2`: constant folding + peephole cleanup (default)
  - `3`: aggressive enabled optimizations
- `--no-const-fold` disables compile-time expression folding
- `--no-peephole` disables bytecode peephole cleanup
- `--fast-math` relaxes strict constant math handling
- `--relaxed-contracts` disables strict compile-error enforcement for contract overflow/underflow policies

Example: high-speed compile with aggressive options and profile report

```powershell
cargo run --bin hl-lex -- --compile examples/cycle_contracts.hl --opt-level 3 --cycle-profile cortex-m0-like --fast-math --contract-report fast_report.txt --out fast.hbc.txt
```

### Run On VM

```powershell
cargo run --bin hl-lex -- --vm examples/advanced.hl --cycle-profile cortex-m0-like
```

### Native Compile

```powershell
cargo run --bin hl-lex -- --native examples/advanced.hl --cycle-profile generic --out advanced.exe
```

Native mode emits:
- object file (`.obj` on Windows, `.o` elsewhere)
- linked executable

### Tests

```powershell
cargo test
```
