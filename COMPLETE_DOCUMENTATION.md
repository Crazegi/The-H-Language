COMPLETE DOCUMENTATION — H Language Toolkit
===========================================

Purpose
-------
This single file documents the H language project in this repository. It explains architecture, every major module and feature, how things work (high-level), why design choices were made, and how to use the toolchain. It's written for readers with minimal prior knowledge of compilers or Rust.

Quick Start
-----------
- Build and run tests:

```powershell
cargo test
```

- Compile a sample program to bytecode and run on VM:

```powershell
cargo run --bin hl-lex -- --compile examples/cycle_contracts.hl --out out.hbc.txt
cargo run --bin hl-lex -- --vm examples/cycle_contracts.hl
```

Project Overview
----------------
H is a small hybrid language for deterministic, low-level-friendly programs (hardware pulses, small controllers). The toolchain implements:
- Lexer (tokenizes source text into tokens)
- Parser (builds AST from tokens)
- Semantic analyzer (checks types/bindings/contracts)
- Compiler (converts AST to bytecode, enforces cycle contracts)
- Virtual Machine (executes bytecode)
- Native backend (emits Rust-coded runtime + linkable object/executable)
- Packaging (`.hbcp`) for distribution without native compile steps
- VS Code language assets in `vscode-h-language/` (grammar/snippets)

Top-level files
---------------
- [src/lexer.rs](src/lexer.rs) — lexical analysis and indentation/YAML handling
- [src/token.rs](src/token.rs) — token kinds and token utilities
- [src/parser.rs](src/parser.rs) — parser to produce AST
- [src/ast.rs](src/ast.rs) — AST node definitions (Expr, Stmt, Function, Program)
- [src/semantic.rs](src/semantic.rs) — semantic checks and validations
- [src/compiler.rs](src/compiler.rs) — bytecode emitter, optimizations, cycle contracts
- [src/bytecode.rs](src/bytecode.rs) — bytecode format and Instruction set
- [src/vm.rs](src/vm.rs) — VM runtime for bytecode execution
- [src/native.rs](src/native.rs) — native code generation (Rust -> object/exe)
- [src/package.rs](src/package.rs) — package serialization (.hbcp)
- [src/main.rs](src/main.rs) — CLI entrypoint and flags
- [examples/] — real-world examples including cycle contract showcase
- [tests/] — test coverage for parser, compiler, VM, package, native

Design Principles and Rationale
--------------------------------
- Readable input: YAML-like structure and indentation-based blocks make it approachable to new users and clearly express structured data like `section` and `print:` blocks.
- Low-level control: assembly-style instructions (`mov`, `add`, etc.) and memory operands (e.g. `[port_a]`) expose hardware-like behavior.
- Determinism via Cycle Contracts: `contract` / `execute` blocks allow compile-time enforcement that a short sequence uses exactly (or within) specified cycle budgets — essential for hardware timing guarantees.
- Two runtime paths: native backend for tight integration with host runtimes, and a packaged bytecode runtime for distribution and CI without invoking rustc.
- Conservative optimizations: constant folding, peephole cleanup, and multiple opt levels to allow safe reasoning and predictable cycle accounting.

Module-by-module Guide
----------------------
Each section below explains purpose, key types/functions, behavior, and "why this matters".

**Lexer: `src/lexer.rs`**
- Purpose: turn source text into a stream of `Token`s.
- Key behaviors:
  - Normalizes CRLF line endings and tracks line/column for diagnostics.
  - Emits `Indent` / `Dedent` tokens so parser can use indentation-based blocks (similar to Python).
  - Recognizes YAML-like keys (used by `print:` and `contract:` blocks) and emits `YamlKey` tokens.
  - Recognizes numeric literals including hex `0x..`, strings, identifiers, keywords, mnemonics (assembly ops), registers like `r1`.
  - Rejects tabs for indentation (error), rejects brace characters as illegal (project standardized to colon+indent syntax).
- Why it matters: Accurate lexing is crucial to provide tight error messages and to support structured blocks and contract keys robustly.

**Tokens: `src/token.rs`**
- `TokenKind` enumerates token types such as `Indent`, `Dedent`, `KeywordFn`, `Colon`, punctuation, operators, and `Mnemonic`.
- `Token` holds kind, lexeme, and `Span` (line/column).
- The project intentionally removed brace tokens to enforce a single block style.

**Parser: `src/parser.rs`**
- Purpose: take the token stream and produce an AST (`Program`) with `data` and `text` sections.
- High-level flow:
  - `parse_program()` orchestrates parsing of named sections (`.data`, `.text`).
  - `parse_function()` parses `fn name(params):` and uses `parse_block` to gather statements.
  - Statements include typed declarations, `own`/`ref`, `if/else`, `while`, `repeat`, `contract`, `print:` blocks, assembly instruction statements, returns, and function calls.
  - `parse_block()` only supports `:` followed by indentation-based block. Brace blocks were removed to avoid syntax ambiguity.
  - Expression parsing follows standard precedence: or/xor/and → equality → comparison → term → factor → unary → primary.
- Important: parser produces useful `ParseError`s with line/column for diagnostics.
- Why this matters: a simple, consistent syntax is both easier to parse and easier for users to learn; avoiding mixed syntaxes prevents confusing edge cases.

**AST: `src/ast.rs`**
- Core node types:
  - `Program { data: BTreeMap<String, Expr>, functions: Vec<Function> }`
  - `Function { name, params, body }`
  - `Stmt` variants: `OwnDecl`, `RefDecl`, `If`, `While`, `Repeat`, `CycleContract` (contract spec + body), `PrintBlock`, `Instruction`, `Assign`, `Expr`, `Return`.
  - `Expr` variants for numbers, strings, bools, variables, calls, unary/binary ops, and `Maybe` tri-state.
- The AST is intentionally small and readable to aid analysis and straightforward bytecode generation.

**Semantic Analyzer: `src/semantic.rs`**
- Purpose: perform type checks, scope/binding validation, and additional constraints that the parser cannot encode.
- Checks include:
  - Validity of `data` section values (must be literals)
  - Ensures `ref` bindings are used correctly (can't assign to a `ref`), and ownership rules for `own`
  - Contract constraints: `execute` must contain only assembly/mnemonic-like statements (prevent arbitrary high-level code in `execute` blocks)
- Why this matters: catching mistakes early and ensuring contracts remain analyzable at compile-time.

**Compiler: `src/compiler.rs`**
- Purpose: convert AST into `BytecodeProgram` (see `src/bytecode.rs`) and enforce cycle contracts at compile-time.
- Key features:
  - Instruction selection: maps AST instructions and high-level constructs into bytecode instructions.
  - Constant folding and peephole optimizations per `opt_level` configuration.
  - Cycle accounting: when encountering a `CycleContract`, the compiler computes cycle costs for the `execute` body according to the selected `CycleProfile` and applies `on_underflow`/`on_overflow` policies:
    - `pad_nop` inserts `Nop` instructions to reach declared cycles when underflowing.
    - `compile_error` emits a compile-time error for overflow or other violations.
  - Options include `opt_level` (O0..O3), `const_folding`, `peephole`, `fast_math`, and `strict_cycle_contracts`.
- Output: a `BytecodeProgram` which contains function bytecode listings and optional contract reports.
- Why this matters: the compiler is where deterministic guarantees are enforced and where performance is tuned.

**Bytecode: `src/bytecode.rs`**
- Contains the `Instruction` enum used by the VM (e.g., `PushInt`, `Load`, `Store`, `Add`, `Mul`, `Mov`, `Nop`, etc.).
- Bytecode format is kept simple and serializable for packaging and VM execution.
- `Nop` is used by contract padding.

**Virtual Machine: `src/vm.rs`**
- Purpose: execute compiled bytecode deterministically.
- Features:
  - Stack/register model consistent with emitted bytecode.
  - Deterministic instruction semantics suitable for cycle accounting tests.
  - Minimal runtime to keep the VM predictable and easy to reason about.

**Native Backend: `src/native.rs`**
- Purpose: emit Rust source that reconstructs program data and functions, allowing native compilation into a host binary.
- Use-case: when a developer needs a native executable instead of the bytecode+VM path.
- Why both paths: native builds may yield tighter integration/performance; vm+package path enables easier CI & distribution.

**Packaging: `src/package.rs`**
- Purpose: serialize the `BytecodeProgram` and metadata into a `.hbcp` file.
- Use-case: produces a portable artifact for running on systems without the full Rust toolchain.

**CLI: `src/main.rs`**
- Entrypoint and flags include:
  - `--tokens` (show tokens), `--ast` (print AST), `--compile` (emit bytecode), `--vm` (run in VM), `--native` (build native executable), `--pack`/`--run-package` (pack/run `.hbcp`).
  - Cycle profile selection `--cycle-profile generic|avr-like|cortex-m0-like` and `--contract-report` path.
  - Optimization and behavior flags: `--opt-level`, `--no-const-fold`, `--no-peephole`, `--fast-math`, `--relaxed-contracts`.
- CLI collects flags, loads files, and routes through parsing → analysis → compile/run paths.

Cycle Contracts (Detailed)
--------------------------
Cycle contracts are a flagship feature; they let you declare a short, timing-sensitive window and guarantee how many cycles it will take.

Example:

```
contract:
  cycles: 16
  on_underflow: "pad_nop"
  on_overflow: "compile_error"
execute:
  mov [port_a], r1
  add r1, r2
  mov [port_a], r2
```

How it works internally:
- The compiler assigns a cycle cost to each bytecode instruction according to a `CycleProfile`.
- When compiling a contract, it sums instruction costs.
  - If sum < declared cycles and `on_underflow` == `pad_nop`, the compiler inserts `Nop` instructions to reach declared cycles.
  - If sum > declared cycles and `on_overflow` == `compile_error`, the compiler fails with a clear diagnostic describing measured vs declared cycles.
- Profiles model target hardware (e.g., `avr-like` may have higher costs for certain ops).

Why this is valuable:
- Real-time embedded code often requires deterministic, provable timing for I/O pulses.
- Contracts allow these guarantees without runtime timing checks; violations are caught at compile time.

Testing and Quality
-------------------
- The repository includes unit and integration tests under [tests/](tests/).
- Tests exercise parsing, semantics, compile-time contract behavior, optimization effects, packaging roundtrips, and native backend flow.
- Running `cargo test` runs all validations and should yield green on a stable state.

Examples
--------
- [examples/sample.hl](examples/sample.hl) — minimal example showing sections and a simple function.
- [examples/advanced.hl](examples/advanced.hl) — demonstrates advanced constructs and builtins.
- [examples/cycle_contracts.hl](examples/cycle_contracts.hl) — showcases cycle contracts and is used in CLI workflow docs.

Common Workflows
----------------
- Edit `.hl` source
- `cargo run --bin hl-lex -- --compile file.hl --out out.hbc.txt` — compile to bytecode
- `cargo run --bin hl-lex -- --vm file.hl` — run on VM
- `cargo run --bin hl-lex -- --pack file.hl --out pkg.hbcp` — produce package
- `cargo run --bin hl-lex -- --run-package pkg.hbcp` — run package directly

Configuration and Optimization
------------------------------
- `--opt-level` controls optimizer aggressiveness (0..3).
- `--no-const-fold`/`--no-peephole` let you disable optimizations for easier cycle reasoning.
- `--fast-math` relaxes strict math checks (useful for performance experiments).
- `--relaxed-contracts` disables strict compile-time contract errors (useful for prototyping).

Development Notes for Contributors
----------------------------------
- Follow code style in existing source files; keep changes minimal and targeted.
- If you change instruction semantics or add instructions, update cycle profiles and contract tests.
- When adding syntax features, prefer explicit grammar changes and add regression tests under `tests/`.

Why This Project Design Is Good (Plain Language)
-------------------------------------------------
- Predictable: indentation + small AST keeps the language easy to reason about.
- Deterministic: cycle contracts and simple VM semantics aid deterministic embedded workflows.
- Dual path: VM packages are great for CI and shipping; native outputs support maximum performance.
- Small surface area: fewer tokens and a single block style reduce user confusion and implementation complexity.

Glossary
--------
- AST: Abstract Syntax Tree — a tree representation of program structure.
- Bytecode: a low-level, portable instruction set executed by the VM.
- Cycle Profile: mapping of instruction → cycle cost used for timing analysis.
- Contract: a compile-time timing budget for a short sequence of instructions.

Appendix: Notable Functions / Files (Quick Index)
-------------------------------------------------
- `Parser::from_source`, `parse_program`, `parse_function`, `parse_block` — parsing entry points in [src/parser.rs](src/parser.rs).
- `Lexer::tokenize`, `next_token` — tokenization logic in [src/lexer.rs](src/lexer.rs).
- `analyze` — main semantic check entry in [src/semantic.rs](src/semantic.rs).
- `compile_program`, `compile_program_with_options` — compiler entrypoints in [src/compiler.rs](src/compiler.rs).
- `run_bytecode` — VM execution entry in [src/vm.rs](src/vm.rs).
- `write_package` / `read_package` — packaging functions in [src/package.rs](src/package.rs).

If you want a function-by-function docstring expansion (every public function fully documented), I can generate that next, producing either inline doc comments in each file or a separate reference manual. Which would you prefer?