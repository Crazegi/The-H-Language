use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use hl_lexer::{
  analyze, compile_program, compile_program_with_options, load_cycle_profiles_from_file,
  diagnose_cycle_profile_coverage, parse_source,
  render_contract_report_text, run_bytecode, CompileOptions, CycleProfile, Instruction,
  OptimizationLevel, UnknownCycleCostPolicy,
};

fn unique_profile_path(file_name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("hl_profiles_{}_{}", nanos, file_name))
}

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
  fn main():
    int total = 1
    repeat 4:
      add total, 1

    own a = phase(true, maybe)
    own b = true xor false
    if collapse((b and not false) or a):
      return total
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "5");
}

#[test]
fn vm_executes_bitwise_and_sleep_until_builtin() {
    let src = r#"section .text:
  fn main():
    own woke = sleep_until("irq_ready")
    own reg = ((1 << 5) | 3) & 0x1F
    if woke:
      return reg >> 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn vm_executes_desktop_file_and_utility_builtins() {
    let path = unique_profile_path("desktop_utils_vm.txt");
    let path_h = path.to_string_lossy().replace('\\', "/");

    let src = format!(
        r#"section .text:
  fn main():
    own path = "{}"
    own wrote = write_text(path, "hello")
    own appended = append_text(path, "_world")
    own text = read_text(path)
    own exists_before = exists(path)
    own deleted = delete_file(path)
    own exists_after = exists(path)
    own n = to_int(trim(" 42 "))
    own s = to_string(n)
    own replaced = replace(s, "2", "7")
    own now = now_ms()
    own fixed = rand_int(5, 5)
    sleep_ms(0)
    if wrote and appended and exists_before and deleted and (not exists_after) and text == "hello_world" and replaced == "47" and now >= 0 and fixed == 5:
      return 1
    return 0
"#,
        path_h
    );

    let program = parse_source(&src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn vm_executes_desktop_starter_api_scaffold_builtins() {
    let src = r#"section .text:
  fn main():
    own payload = http_get("https://example.com/api")
    own status = json_parse(payload, "status")
    own url = json_parse(payload, "url")
    own picked = menu("Main", "Open|Settings|Exit")
    own loop_result = window_loop("DemoApp", 2)

    if status == 200 and contains(url, "example.com") and picked == "Open" and contains(loop_result, "DemoApp"):
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn vm_executes_scripting_library_builtins() {
    let script_dir = unique_profile_path("script_lib_vm");
    std::fs::create_dir_all(&script_dir).expect("should create temp script dir");
    let script_dir_h = script_dir.to_string_lossy().replace('\\', "/");

    let src = format!(
        r#"section .text:
  fn main():
    own before = script_cwd()
    own moved = script_chdir("{}")
    own now = script_cwd()
    own joined = script_path_join(now, "tool.txt")
    own dir = script_dirname(joined)
    own base = script_basename(joined)
    own out = script_run_capture("echo script_ok")
    own code = script_run("echo script_run")
    own count = script_args_count()
    own back = script_chdir(before)

    if moved and back and now == "{}" and dir == "{}" and base == "tool.txt" and contains(out, "script_ok") and code == 0 and count >= 1:
      return 1
    return 0
"#,
        script_dir_h, script_dir_h, script_dir_h
    );

    let program = parse_source(&src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn vm_executes_core_stdlib_math_collections_string_convert() {
    let src = r#"section .text:
  fn main():
    own arr = array_new()
    arr = array_push(arr, "red")
    arr = array_push(arr, "blue")
    own arr_n = array_len(arr)
    own second = array_get(arr, 1)
    own joined = join(arr, ",")

    own parts = split("a,b,c", ",")
    own parts_n = array_len(parts)

    own q = queue_new()
    q = queue_push(q, "job1")
    q = queue_push(q, "job2")
    own head = queue_peek(q)
    q = queue_pop(q)
    own q_n = queue_len(q)

    own ring = ring_new(2)
    ring = ring_push(ring, "x")
    ring = ring_push(ring, "y")
    ring = ring_push(ring, "z")
    own ring_head = ring_peek(ring)
    own ring_n = ring_len(ring)

    own lg = log2(16)
    own s = sin(30)
    own c = cos(60)
    own t = tan(45)
    own f = floor(7)
    own ce = ceil(7)

    own b = to_bool("true")
    own fp = to_float("12.345")
    own fp_s = to_float_string(fp)

    if arr_n == 2 and second == "blue" and joined == "red,blue" and parts_n == 3 and head == "job1" and q_n == 1 and ring_head == "y" and ring_n == 2 and lg == 4 and s >= 499 and s <= 501 and c >= 499 and c <= 501 and t >= 999 and t <= 1001 and f == 7 and ce == 7 and b and fp == 12345 and fp_s == "12.345":
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn cycle_contract_underflow_pads_with_nop() {
    let src = r#"section .text:
  fn main():
    own r1 = 1
    own r2 = 2
    own [port_a]
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
    own [port_a]
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
    own [port_a]
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
          cycle_profile_override: None,
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
          cycle_profile_override: None,
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
    own [port_a]
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
          cycle_profile_override: None,
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
          cycle_profile_override: None,
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
          cycle_profile_override: None,
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
    own [port_a]
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
          cycle_profile_override: None,
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
          cycle_profile_override: None,
            opt_level: OptimizationLevel::O2,
            const_folding: true,
            peephole: true,
            fast_math: false,
            strict_cycle_contracts: false,
        },
    );
    assert!(relaxed_ok.is_ok());
}

#[test]
fn cycle_contract_allows_deterministic_high_level_subset() {
    let src = r#"section .text:
  fn main():
    contract:
      cycles: 7
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      own x = 1 + 2
      if true:
        add x, 1
      else:
        add x, 99
      repeat 2:
        sub x, 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let compiled = compile_program(&program).expect("compile should pass");

    let main = compiled
        .functions
        .get("main")
        .expect("main function should exist");
    let nop_count = main
        .code
        .iter()
        .filter(|ins| matches!(ins, Instruction::Nop))
        .count();
    assert_eq!(nop_count, 0);
}

#[test]
fn cycle_contract_rejects_non_constant_if_condition() {
    let src = r#"section .text:
  fn main():
    own gate = maybe
    own [port_a]
    contract:
      cycles: 4
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      if gate:
        mov [port_a], 1
      else:
        mov [port_a], 0
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let err = compile_program(&program).expect_err("compile should fail on dynamic execute if");
    assert!(err
      .message
      .contains("if` condition must be compile-time constant")
      || err.message.contains("condition must be compile-time constant"));
}

#[test]
fn cycle_contract_rejects_non_constant_repeat_count() {
    let src = r#"section .text:
  fn main():
    own n = 2
    own [port_a]
    contract:
      cycles: 4
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      repeat n:
        mov [port_a], 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let err = compile_program(&program).expect_err("compile should fail on dynamic execute repeat");
    assert!(err
      .message
      .contains("repeat` count must be compile-time constant")
      || err.message.contains("count must be compile-time constant"));
}

  #[test]
  fn external_profile_inheritance_overrides_mul_cost() {
    let profile_path = unique_profile_path("inherit.toml");
    let profile_text = r#"[profiles.cortex-m4-like]
  extends = "generic"

  [profiles.cortex-m4-like.costs]
  "instr.mul" = 4
  "expr.mul" = 4
  "#;
    fs::write(&profile_path, profile_text).expect("profile file write should succeed");

    let src = r#"section .text:
  fn main():
    own r1 = 20
    own r2 = 4
    own [port_a]
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

    let profiles = load_cycle_profiles_from_file(&profile_path)
      .expect("external profiles should load successfully");
    let selected = profiles
      .get("cortex-m4-like")
      .expect("custom profile should exist")
      .clone();

    let err = compile_program_with_options(
      &program,
      CompileOptions {
        cycle_profile: CycleProfile::Generic,
        cycle_profile_override: Some(selected),
        ..Default::default()
      },
    )
    .expect_err("custom profile should overflow with mul cost override");
    assert!(err.message.contains("overflow"));

    let _ = fs::remove_file(profile_path);
  }

  #[test]
  fn strict_unknown_cost_policy_rejects_missing_keys() {
    let profile_path = unique_profile_path("strict.toml");
    let profile_text = r#"[profiles.minimal-strict]
  unknown_policy = "strict"

  [profiles.minimal-strict.costs]
  "instr.mov" = 1
  "#;
    fs::write(&profile_path, profile_text).expect("profile file write should succeed");

    let src = r#"section .text:
  fn main():
    own r1 = 1
    own r2 = 2
    own [port_a]
    contract:
      cycles: 4
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      add r1, r2
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");

    let profiles = load_cycle_profiles_from_file(&profile_path)
      .expect("external profiles should load successfully");
    let selected = profiles
      .get("minimal-strict")
      .expect("custom profile should exist")
      .clone();

    let err = compile_program_with_options(
      &program,
      CompileOptions {
        cycle_profile: CycleProfile::Generic,
        cycle_profile_override: Some(selected),
        ..Default::default()
      },
    )
    .expect_err("strict mode should reject unknown cycle keys");
    assert!(err.message.contains("Unknown cycle cost key"));

    let _ = fs::remove_file(profile_path);
  }

  #[test]
  fn conservative_unknown_cost_policy_uses_fallback() {
    let src = r#"section .text:
  fn main():
    own r1 = 1
    own r2 = 2
    own [port_a]
    contract:
      cycles: 4
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      add r1, r2
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");

    let mut selected = hl_lexer::CycleCostProfile {
      name: "conservative-minimal".to_string(),
      costs: std::collections::HashMap::new(),
      metadata: std::collections::HashMap::new(),
      unknown_policy: UnknownCycleCostPolicy::Conservative,
      conservative_fallback: 2,
    };
    selected.costs.insert("instr.mov".to_string(), 1);

    let compiled = compile_program_with_options(
      &program,
      CompileOptions {
        cycle_profile: CycleProfile::Generic,
        cycle_profile_override: Some(selected),
        ..Default::default()
      },
    )
    .expect("conservative unknown mode should compile");

    let text = render_contract_report_text(&compiled.contract_reports);
    assert!(text.contains("profile=conservative-minimal"));
  }

  #[test]
  fn profile_loader_parses_metadata_fields() {
    let profile_path = unique_profile_path("metadata.toml");
    let profile_text = r#"[profiles.arm-m4]
  extends = "generic"

  [profiles.arm-m4.costs]
  "instr.mul" = 4

  [profiles.arm-m4.sources]
  "instr.mul" = "ARM TRM rev C"

  [profiles.arm-m4.confidence]
  "instr.mul" = "high"

  [profiles.arm-m4.worst_case_cycles]
  "instr.mul" = 6
  "#;
    fs::write(&profile_path, profile_text).expect("profile file write should succeed");

    let profiles = load_cycle_profiles_from_file(&profile_path)
      .expect("profile load should succeed");
    let profile = profiles.get("arm-m4").expect("profile should exist");
    let meta = profile
      .metadata
      .get("instr.mul")
      .expect("metadata for instr.mul should exist");

    assert_eq!(meta.source.as_deref(), Some("ARM TRM rev C"));
    assert_eq!(meta.confidence.as_deref(), Some("high"));
    assert_eq!(meta.worst_case_cycles, Some(6));

    let _ = fs::remove_file(profile_path);
  }

  #[test]
  fn profile_doctor_reports_missing_keys() {
    let src = r#"section .text:
  fn main():
    own r1 = 2
    own r2 = 3
    own [port_a]
    contract:
      cycles: 6
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      own x = r1 + r2
      mul x, r2
      mov [port_a], x
    return x
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic should pass");

    let mut profile = hl_lexer::CycleCostProfile {
      name: "doctor-test".to_string(),
      costs: std::collections::HashMap::new(),
      metadata: std::collections::HashMap::new(),
      unknown_policy: UnknownCycleCostPolicy::Strict,
      conservative_fallback: 1,
    };
    profile.costs.insert("instr.mov".to_string(), 1);

    let report = diagnose_cycle_profile_coverage(
      &program,
      &CompileOptions {
        cycle_profile: CycleProfile::Generic,
        cycle_profile_override: Some(profile),
        ..Default::default()
      },
    );

    assert!(report.required_keys.iter().any(|k| k == "instr.mul"));
    assert!(report.required_keys.iter().any(|k| k == "expr.binary.default"));
    assert!(report.missing_keys.iter().any(|k| k == "instr.mul"));
    assert!(report.missing_keys.iter().any(|k| k == "stmt.store"));
  }

#[test]
fn energy_contract_overflow_reports_compile_error() {
    let profile_path = unique_profile_path("energy_overflow.toml");
    let profile_text = r#"[profiles.radio-heavy]
extends = "generic"

[profiles.radio-heavy.energy_nj]
"instr.mov" = 60
"#;
    fs::write(&profile_path, profile_text).expect("profile file write should succeed");

    let src = r#"section .text:
  fn main():
    own r1 = 1
    own [radio_tx]
    contract:
      cycles: 20
      energy_nj: 20
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      mov [radio_tx], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");

    let profiles = load_cycle_profiles_from_file(&profile_path)
      .expect("external profiles should load successfully");
    let selected = profiles
      .get("radio-heavy")
      .expect("custom profile should exist")
      .clone();

    let err = compile_program_with_options(
      &program,
      CompileOptions {
        cycle_profile: CycleProfile::Generic,
        cycle_profile_override: Some(selected),
        ..Default::default()
      },
    )
    .expect_err("energy budget overflow should fail compile");
    assert!(err.message.contains("Energy contract overflow"));

    let _ = fs::remove_file(profile_path);
}

#[test]
fn contract_report_contains_energy_fields_when_present() {
    let src = r#"section .text:
  fn main():
    own r1 = 1
    own [port_a]
    contract:
      cycles: 8
      energy_nj: 20
      on_underflow: "pad_nop"
      on_overflow: "compile_error"
    execute:
      mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
  let compiled = compile_program_with_options(
    &program,
    CompileOptions {
      cycle_profile: CycleProfile::Generic,
      cycle_profile_override: None,
      ..Default::default()
    },
  )
  .expect("compile should pass");

    let text = render_contract_report_text(&compiled.contract_reports);
    assert!(text.contains("declared_energy_nj=20"));
    assert!(text.contains("measured_energy_nj="));
}
