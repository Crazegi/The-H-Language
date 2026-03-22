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
- Functions: `fn name(a, b):` with indentation-based blocks
- Ownership/borrowing: `own` and `ref`
- Hardware port ownership: `own [port_a]`, `ref [port_a]`, and `ref alias = &[port_a]`
- Interrupt handlers: `interrupt fn handler():`
- Yield windows for interrupt grants: `yield [port_a] to handler:`
- Typed declarations in block syntax: `int`, `string`, `bool`
- Assembly instructions: `mov`, `add`, `sub`, `mul`, `div`, `mod`, `cmp`
- Memory-mapped destination style: `mov [port_a], r1`
- Expressions: arithmetic, comparison, bitwise (`&`, `|`, `<<`, `>>`), call expressions
- Tri-state logic: `true`, `false`, `maybe`, `and`, `or`, `xor`, `not`
- Builtins:
  - math: `abs`, `sqrt`, `pow`, `min`, `max`, `clamp`
  - string: `len`, `upper`, `lower`, `contains`
  - logic: `phase`, `collapse`
  - hardware: `sleep_until(interrupt)`
- Control flow: `if/else`, `while`, `repeat`, `return`
- Structured output: YAML-style `print:` blocks

## Hardware Port Ownership (Long Note)

### Why this exists

In normal memory safety discussions, the big risk is two references writing the same RAM location.
In embedded systems, a different class of failure is often more dangerous in practice:

- Two unrelated code paths drive the same physical pin or bus.
- Timing overlaps happen under load, interrupts, or integration changes.
- Hardware sees conflicting levels/frames and behaves unpredictably.

Examples of real failures this can cause:

- I2C arbitration corruption when two drivers push to the same line ownership domain.
- GPIO glitching when one routine toggles while another assumes stable output.
- Radio transaction corruption when TX control is triggered concurrently.

H now models this as a compile-time ownership rule for memory-mapped ports.

### The model in one sentence

If code writes `mov [port_x], ...`, that function must hold explicit ownership/borrow rights
for `port_x`, or compilation fails.

### Syntax

Ownership declaration:

```text
own [port_a]
```

Borrow declaration:

```text
ref [port_a]
```

Alias borrow form:

```text
ref tx = &[radio_tx]
```

Port write:

```text
mov [port_a], r1
```

### What is enforced

1. Local write permission:
- A function cannot write to `[port]` unless it has `own [port]` or `ref [port]` in scope
  (directly or via `ref alias = &[port]`).

2. Global owner collision prevention:
- If two different functions claim `own [same_port]`, semantic analysis fails.
- This blocks accidental multi-owner architecture drift as the codebase grows.

3. Compile-time rejection with explicit diagnostics:
- Missing ownership gives a direct compile/semantic error.
- Duplicate owners across functions give a dedicated ownership-collision error.

### Practical usage pattern

A good baseline pattern for embedded modules:

1. Assign one owner function per physical endpoint.
2. Let helper routines use `ref [port]` if they must touch that endpoint.
3. Keep all `mov [port], ...` operations inside owned/borrowed contexts.

This naturally creates a hardware access map in code, without separate spreadsheets.

### Example: valid pattern

```text
section .text:
  fn radio_send(byte):
    own [radio_tx]
    own r1 = byte
    mov [radio_tx], r1
    return r1
```

### Example: rejected pattern (no ownership)

```text
section .text:
  fn bad():
    own r1 = 1
    mov [port_a], r1
    return r1
```

Reason: write to `[port_a]` without `own [port_a]` or `ref [port_a]`.

### Example: rejected pattern (two owners)

```text
section .text:
  fn a():
    own [port_a]
    own r1 = 1
    mov [port_a], r1
    return r1

  fn b():
    own [port_a]
    own r2 = 2
    mov [port_a], r2
    return r2
```

Reason: ownership collision, both `a` and `b` claim exclusive ownership.

### Relationship to contracts

Port ownership and cycle/energy contracts are complementary:

- Ownership answers: "Who is allowed to drive this hardware endpoint?"
- Cycle contract answers: "How long does this critical block run?"
- Energy contract answers: "How much energy does this block consume?"

Together they provide a stronger embedded safety envelope:
- structural safety (ownership),
- temporal safety (cycles),
- power safety (energy).

### Current scope and future direction

Current scope is intentionally strict and simple:

- ownership is function-level and compile-time checked,
- collisions are prevented early,
- runtime behavior remains unchanged (these are semantic guarantees).

Natural next expansions:

- explicit ownership transfer semantics between functions,
- scoped ownership blocks,
- capability-like port tokens for larger system decomposition.

## Interrupt-Safe Ownership Yields

The ownership model now includes an interrupt-aware grant path.
This solves the common embedded conflict where a normal function owns a hardware port,
but an interrupt handler must touch the same endpoint safely.

### Syntax

Interrupt function declaration:

```text
interrupt fn emergency_interrupt():
  own r1 = 1
  mov [port_a], r1
```

Yield window declaration from the owner function:

```text
yield [port_a] to emergency_interrupt:
  mov [port_a], r1
```

### Compile-time rules

1. Interrupt handler shape:
- `interrupt fn` cannot declare parameters.

2. Ownership discipline:
- `interrupt fn` cannot declare `own [port]` directly.
- A normal owner function can grant access with `yield [port] to handler:`.

3. Grant target validation:
- The yield target must be an `interrupt fn`.
- A function can only yield a port it actually owns.

4. Effective access in handlers:
- Interrupt handler writes to `[port]` are valid only when that port was granted through a yield.
- Without a grant, writes are rejected with the same ownership diagnostics.

### Example: valid

```text
section .text:
  interrupt fn emergency_interrupt():
    own r1 = 7
    mov [port_a], r1
    return r1

  fn main():
    own [port_a]
    own r1 = 1
    yield [port_a] to emergency_interrupt:
      mov [port_a], r1
    return r1
```

### Example: rejected (wrong target)

```text
section .text:
  fn helper():
    return 0

  fn main():
    own [port_a]
    yield [port_a] to helper:
      mov [port_a], 1
    return 0
```

Reason: yield target is not declared as `interrupt fn`.

### Current implementation scope

Current implementation is compile-time capability validation.
It does not yet emit runtime preemption locks/flags automatically.
That keeps behavior deterministic and simple while still blocking unsafe ownership patterns.

## Cycle Contracts

Contract shape:

```text
contract:
  cycles: 16
  energy_nj: 45
  on_underflow: "pad_nop"
  on_overflow: "compile_error"
execute:
  mov [port_a], r1
  add r1, r2
  mov [port_a], r2
```

Rules:
- `execute` supports deterministic statements: assembly instructions, `own`/assignment, `if`, and `repeat`.
- `if` conditions and `repeat` counts inside `execute` must be compile-time constants.
- Function calls are rejected inside `execute`.
- Memory-mapped writes require hardware ownership (`own [port]` or `ref [port]`).
- Underflow can be padded with inserted `nop` instructions.
- Overflow can be rejected as a compile-time error.
- If `energy_nj` is set, compile fails when measured execute energy exceeds budget.

## Phase 4: Cycle Profiles And Reports

Available profiles:
- `generic`
- `avr-like`
- `cortex-m0-like`

External profile files are also supported via `--cycle-profile-file`.
Profiles can inherit from built-ins (or other custom profiles), override only selected keys,
and set unknown-cost behavior.

Energy budgets use profile table `energy_nj` (same key naming as `costs`, e.g. `instr.mov`).

Profiles can also attach traceability metadata per key:
- `sources.<key>` (where the number came from, e.g. TRM section)
- `confidence.<key>` (e.g. `high`, `medium`, `low`)
- `worst_case_cycles.<key>` (optional worst-case bound)

Example profile file (`examples/cycle_profiles.toml`):

```toml
[profiles.cortex-m4-like]
extends = "generic"
unknown_policy = "strict"

[profiles.cortex-m4-like.costs]
"instr.mul" = 4
"expr.mul" = 4

[profiles.cortex-m4-like.energy_nj]
"instr.mov" = 6
"instr.mul" = 10

[profiles.cortex-m4-like.sources]
"instr.mul" = "ARM TRM rev C"

[profiles.cortex-m4-like.confidence]
"instr.mul" = "high"

[profiles.cortex-m4-like.worst_case_cycles]
"instr.mul" = 6

[profiles.esp32-safe]
extends = "generic"
unknown_policy = "conservative"
conservative_fallback = 3

[profiles.esp32-safe.costs]
"instr.div" = 10
"instr.mod" = 10
```

Profile impact:
- Different targets can assign different cycle costs for the same instruction.
- The same contract can pass in one profile and fail in another.

Contract report output includes:
- function name
- contract index
- selected profile
- declared cycles
- measured cycles
- declared energy budget (when set)
- measured energy (when set)
- padded nop count
- final cycle count
- underflow and overflow policies

Additional cycle profile flags:
- `--cycle-profile-file <path>` loads profiles from TOML.
- `--cycle-profile <name>` selects built-in or file-defined profile name.
- `--unknown-cycle-cost strict|conservative` overrides selected profile policy.
- `--unknown-cycle-cost-fallback <n>` overrides conservative fallback cycles.

Game changer: Profile Doctor (`--profile-doctor`)
- Audits which cycle keys your source actually needs in `contract/execute` blocks.
- Lists missing keys before compile-time overflow/unknown-cost failures.
- Prints metadata (source/confidence/worst-case) per key when available.

Example:

```powershell
cargo run --bin hl-lex -- --profile-doctor examples/cycle_contracts.hl --cycle-profile-file examples/cycle_profiles.toml --cycle-profile esp32-safe --out profile_doctor.txt
```

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

### Rewrite Phase: Rust-Toolchain-Independent Packaging

This workflow avoids the rustc native backend path:
1. Compile source to bytecode package (`.hbcp`).
2. Run the package directly with the VM runtime.

Create package:

```powershell
cargo run --bin hl-lex -- --pack examples/cycle_contracts.hl --opt-level 3 --cycle-profile generic --contract-report pkg_report.txt --out cycle.hbcp
```

Run package:

```powershell
cargo run --bin hl-lex -- --run-package cycle.hbcp
```

Why this exists:
- no rustc invocation in the compile/distribution path
- fast deployment artifact for CI and test benches
- deterministic bytecode payload that can be transported across environments

### Tests

```powershell
cargo test
```
