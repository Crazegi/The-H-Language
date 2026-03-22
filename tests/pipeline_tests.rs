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
