use hl_lexer::{analyze, parse_source, run_program};

const PROGRAM: &str = r#"section .data:
  sensor_name: "Engine"

section .text:
  fn bump(v):
    own r1 = v
    add r1, 3
    return r1

  fn main():
    own r1 = 10
    own r2 = bump(r1)
    if r2 > 10:
      print:
        event: "ok"
        reading: r2
        sensor: sensor_name
    return r2
"#;

#[test]
fn parses_semantics_and_runs() {
    let program = parse_source(PROGRAM).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "13");
}

#[test]
fn semantic_rejects_assign_to_ref() {
    let src = r#"section .data:
  x: 1

section .text:
  fn main():
    own r1 = 5
    ref alias = &r1
    alias = 9
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("assigning to ref should fail");
    assert!(err.message.contains("Cannot assign to reference binding"));
}

#[test]
fn parses_colon_syntax_and_math_builtins() {
    let src = r#"section .data:
  cap: 100

section .text:
  fn main():
    int x = -9
    int y = abs(x)
    int z = pow(y, 2)
    int root = sqrt(z)
    int low = min(root, 4)
    int high = max(low, 7)
    int bounded = clamp(high, 0, cap)
    if bounded >= 7:
      print:
        event: "java_style"
        reading: bounded
    return bounded
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "7");
}

#[test]
fn runs_repeat_and_tristate_logic_features() {
    let src = r#"section .data:
  seed: "Pulse"

section .text:
  fn main():
    int n = 0
    repeat 3:
      add n, 2

    string loud = upper(seed)
    bool has_u = contains(loud, "U")
    own gate = phase(has_u, maybe)
    own final = collapse((true xor false) and (not false or gate))

    if final:
      return n
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "6");
}

#[test]
fn parses_and_runs_cycle_contract_execute_block() {
    let src = r#"section .text:
  fn hardware_pulse():
    own r1 = 0x01
    own r2 = 0x00
    own [port_a]

    contract:
      cycles: 16
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      mov [port_a], r1
      add r1, r2
      mov [port_a], r2

    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn rejects_invalid_cycle_contract_policy() {
    let src = r#"section .text:
  fn main():
    contract:
      cycles: 8
      on_underflow: "pad_nop"
      on_overflow: "drop"
    execute:
      own r1 = 1
    return 0
"#;

    let err = parse_source(src).expect_err("invalid policy should fail parse");
    assert!(err.message.contains("Invalid contract policy"));
}

#[test]
fn rejects_brace_block_syntax() {
    let src = r#"section .text:
  fn main() {
    return 0
  }
"#;

    let err = parse_source(src).expect_err("brace block syntax should fail parse");
    assert!(err.message.contains("Illegal character '{'"));
}

#[test]
fn semantic_rejects_while_inside_execute_block() {
    let src = r#"section .text:
  fn main():
    contract:
      cycles: 8
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      while true:
        mov [port_a], 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("while in execute should fail semantic analysis");
    assert!(err
        .message
        .contains("supports deterministic statements only"));
}

#[test]
fn semantic_rejects_port_write_without_ownership() {
    let src = r#"section .text:
  fn main():
    own r1 = 1
    mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("port write without ownership should fail");
    assert!(err.message.contains("requires hardware ownership"));
}

#[test]
fn semantic_rejects_duplicate_port_owners_across_functions() {
    let src = r#"section .text:
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
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("duplicate port ownership should fail");
    assert!(err.message.contains("ownership collision"));
}
