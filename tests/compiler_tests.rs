use hl_lexer::{
  analyze, compile_program, compile_program_with_options, parse_source,
  render_contract_report_text, run_bytecode, CompileOptions, CycleProfile, Instruction,
  OptimizationLevel,
};

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

#[test]
fn cycle_contract_underflow_pads_with_nop() {
    let src = r#"section .text:
  fn main():
    own r1 = 1
    own r2 = 2
    contract:
      cycles: 5
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      add r1, r2
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");

    let main = bytecode
        .functions
        .get("main")
        .expect("main function should exist");
    let nop_count = main
        .code
        .iter()
        .filter(|ins| matches!(ins, Instruction::Nop))
        .count();
    assert_eq!(nop_count, 3);
}

#[test]
fn cycle_contract_overflow_reports_compile_error() {
    let src = r#"section .text:
  fn main():
    own r1 = 4
    own r2 = 3
    contract:
      cycles: 1
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      mul r1, r2
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let err = compile_program(&program).expect_err("compile should fail on overflow");
    assert!(err.message.contains("Cycle contract overflow"));
}

#[test]
fn cycle_profile_changes_contract_budget_behavior() {
    let src = r#"section .text:
  fn main():
    own r1 = 20
    own r2 = 4
    contract:
      cycles: 3
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      mul r1, r2
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");

    let generic = compile_program_with_options(
        &program,
        CompileOptions {
            cycle_profile: CycleProfile::Generic,
        ..Default::default()
        },
    )
    .expect("generic profile should compile");
    let generic_nops = generic
        .bytecode
        .functions
        .get("main")
        .expect("main should exist")
        .code
        .iter()
        .filter(|ins| matches!(ins, Instruction::Nop))
        .count();
    assert_eq!(generic_nops, 0);

    let avr_err = compile_program_with_options(
        &program,
        CompileOptions {
            cycle_profile: CycleProfile::AvrLike,
        ..Default::default()
        },
    )
    .expect_err("avr-like profile should overflow");
    assert!(avr_err.message.contains("overflow"));
}

#[test]
fn contract_report_contains_profile_and_padding() {
    let src = r#"section .text:
  fn main():
    own r1 = 1
    own r2 = 2
    contract:
      cycles: 5
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      add r1, r2
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let compiled = compile_program_with_options(
        &program,
        CompileOptions {
            cycle_profile: CycleProfile::Generic,
        ..Default::default()
        },
    )
    .expect("compile should pass");

    let text = render_contract_report_text(&compiled.contract_reports);
    assert!(text.contains("profile=generic"));
    assert!(text.contains("padded_nops=3"));
}

#[test]
fn optimizer_folds_constant_expressions() {
    let src = r#"section .text:
  fn main():
    own r1 = (2 + 3) * 4
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");

    let unoptimized = compile_program_with_options(
        &program,
        CompileOptions {
            cycle_profile: CycleProfile::Generic,
            opt_level: OptimizationLevel::O0,
            const_folding: true,
            peephole: true,
            fast_math: false,
            strict_cycle_contracts: true,
        },
    )
    .expect("compile should pass");

    let optimized = compile_program_with_options(
        &program,
        CompileOptions {
            cycle_profile: CycleProfile::Generic,
            opt_level: OptimizationLevel::O3,
            const_folding: true,
            peephole: true,
            fast_math: false,
            strict_cycle_contracts: true,
        },
    )
    .expect("compile should pass");

    let unoptimized_main = unoptimized
        .bytecode
        .functions
        .get("main")
        .expect("main should exist");
    let optimized_main = optimized
        .bytecode
        .functions
        .get("main")
        .expect("main should exist");

    let optimized_has_folded_push = optimized_main
        .code
        .iter()
        .any(|ins| matches!(ins, Instruction::PushInt(20)));
    assert!(optimized_has_folded_push);
    assert!(optimized_main.code.len() < unoptimized_main.code.len());
}

#[test]
fn relaxed_contract_mode_allows_compile_error_policies() {
    let src = r#"section .text:
  fn main():
    own r1 = 9
    own r2 = 3
    contract:
      cycles: 1
      on_underflow: "compile_error"
      on_overflow: "compile_error"
    execute:
      mul r1, r2
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");

    let strict_err = compile_program_with_options(
        &program,
        CompileOptions {
            cycle_profile: CycleProfile::Generic,
            opt_level: OptimizationLevel::O2,
            const_folding: true,
            peephole: true,
            fast_math: false,
            strict_cycle_contracts: true,
        },
    )
    .expect_err("strict mode should fail");
    assert!(strict_err.message.contains("overflow"));

    let relaxed_ok = compile_program_with_options(
        &program,
        CompileOptions {
            cycle_profile: CycleProfile::Generic,
            opt_level: OptimizationLevel::O2,
            const_folding: true,
            peephole: true,
            fast_math: false,
            strict_cycle_contracts: false,
        },
    );
    assert!(relaxed_ok.is_ok());
}
