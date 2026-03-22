use hl_lexer::{analyze, compile_program, parse_source, run_bytecode};

const PROGRAM: &str = r#"section .data:
  threshold: 10

section .text:
  fn bump(v):
    own r1 = v
    add r1, 2
    return r1

  fn main():
    own r1 = 3
    own r2 = bump(r1)
    while r2 < threshold:
      add r2, 1
    if r2 == threshold:
      print:
        event: "hit"
        value: r2
    return r2
"#;

#[test]
fn compiles_and_runs_program_in_vm() {
    let program = parse_source(PROGRAM).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "10");
}

#[test]
fn compile_rejects_non_literal_data_value() {
    let src = r#"section .data:
  bad: 1 + 2

section .text:
  fn main():
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let err = compile_program(&program).expect_err("compile should reject non-literal data");
    assert!(err.message.contains("literal constants"));
}

#[test]
fn vm_supports_repeat_and_exotic_logic() {
    let src = r#"section .data:
  title: "phase"

section .text:
  fn main() {
    int total = 1;
    repeat 4 {
      add total, 1;
    }

    own a = phase(true, maybe);
    own b = true xor false;
    if collapse((b and not false) or a) {
      return total;
    }
    return 0;
  }
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "5");
}
