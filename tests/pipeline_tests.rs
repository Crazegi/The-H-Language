use hl_lexer::{analyze, analyze_with_warnings, parse_source, parse_source_from_path, run_program};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_path(file_name: &str) -> std::path::PathBuf {
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .expect("clock before unix epoch")
    .as_nanos();
  std::env::temp_dir().join(format!("hl_desktop_{}_{}", nanos, file_name))
}

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
    assert!(err.message.contains("Cannot assign to immutable binding"));
}

#[test]
fn semantic_rejects_assign_to_const() {
    let src = r#"section .text:
  fn main():
    const LIMIT = 5
    LIMIT = 7
    return LIMIT
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("assigning to const should fail");
    assert!(err.message.contains("immutable binding"));
}

#[test]
fn semantic_rejects_non_literal_const_expression() {
    let src = r#"section .text:
  fn main():
    const LIMIT = 1 + 2
    return LIMIT
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("non-literal const should fail");
    assert!(err.message.contains("literal values"));
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
fn parses_and_runs_for_loop_without_unused_index_warning() {
    let src = r#"section .text:
  fn main():
    own sum = 0
    for i in 5:
      add sum, i
    return sum
"#;

    let program = parse_source(src).expect("parse should pass");
    let warnings = analyze_with_warnings(&program).expect("semantic analysis should pass");
    assert!(!warnings
        .iter()
        .any(|w| w.message.contains("Variable `i` declared but never used")));
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "10");
}

#[test]
fn parses_and_runs_for_loop_over_array_items() {
    let src = r#"section .text:
  fn main():
    own arr = array_new()
    arr = array_push(arr, "a")
    arr = array_push(arr, "b")
    arr = array_push(arr, "c")

    own total = 0
    for item in arr:
      own n = len(item)
      add total, n
    return total
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "3");
}

#[test]
fn parses_and_runs_format_builtin() {
    let src = r#"section .text:
  fn main():
    own name = "port_a"
    own reading = 42
    own status = "HIGH"
    own msg = format("sensor={} reading={} status={}", name, reading, status)
    if msg == "sensor=port_a reading=42 status=HIGH":
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_const_declaration() {
    let src = r#"section .text:
  fn main():
    const SENSOR_ID = "port_a"
    const LIMIT = 10
    own msg = format("{}:{}", SENSOR_ID, LIMIT)
    if msg == "port_a:10":
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
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
fn parses_and_runs_core_stdlib_math_collections_string_convert() {
    let src = r#"section .text:
  fn main():
    own arr = array_new()
    arr = array_push(arr, "alpha")
    arr = array_push(arr, "beta")
    own second = array_get(arr, 1)

    own q = queue_new()
    q = queue_push(q, "A")
    q = queue_push(q, "B")
    own first_q = queue_peek(q)
    q = queue_pop(q)

    own ring = ring_new(2)
    ring = ring_push(ring, "one")
    ring = ring_push(ring, "two")
    ring = ring_push(ring, "three")

    own pieces = split("x|y|z", "|")
    own rebuilt = join(pieces, "-")

    own lg = log2(8)
    own s = sin(30)
    own c = cos(60)
    own t = tan(45)
    own b = to_bool("on")
    own fp = to_float("2.500")
    own fp_s = to_float_string(fp)

    if second == "beta" and first_q == "A" and queue_len(q) == 1 and ring_peek(ring) == "two" and ring_len(ring) == 2 and rebuilt == "x-y-z" and lg == 3 and s >= 499 and s <= 501 and c >= 499 and c <= 501 and t >= 999 and t <= 1001 and b and fp == 2500 and fp_s == "2.500":
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_embedded_hardware_library_builtins() {
    let src = r#"section .text:
  fn main():
    own pin = gpio_claim("[port_a]")
    own pin_cfg = gpio_mode(pin, "out")
    own pin_ok = gpio_write(pin, 1)

    own uart = uart_new("uart1", 9600)
    own sent = uart_write(uart, "HI")

    own spi = spi_new("spi1", 2000000, 1)
    own echo = spi_transfer(spi, "55")

    own i2c = i2c_new("i2c1", 100000)
    own readback = i2c_read(i2c, 0x20, 3)

    own timer = timer_new("t1", 1000)
    own started = timer_start(timer, 16)

    own wd = watchdog_new("wd1", 100)
    own fed = watchdog_feed(wd)

    own dma = dma_new("ch1")
    own moved = dma_transfer(dma, "s", "d", 8)

    if contains(pin_cfg, "mode=out") and pin_ok and sent == 2 and echo == "55" and readback == "20 20 20" and contains(started, "cycles=16") and fed and moved:
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_extended_math_and_bit_builtins() {
    let src = r#"section .text:
  fn main():
    own fx = to_float("3.500")
    own r = round(fx)
    own t = trunc(fx)
    own f = frac(fx)
    own sn = snap(33, 16)

    own lg10 = log10(to_float("10.000"))
    own nln = ln(to_float("1.000"))
    own ex = exp(to_float("0.000"))

    own a1 = asin(0)
    own a2 = acos(0)
    own a3 = atan(1000)
    own a4 = atan2(1, 0)

    own g = gcd(18, 12)
    own l = lcm(18, 12)
    own p = is_prime(29)
    own np2 = next_pow2(1000)

    own pc = popcount(0xFF)
    own lz = leading_zeros(1)
    own tz = trailing_zeros(16)

    if r == 4 and t == 3 and f == 500 and sn == 32 and lg10 == 1000 and nln == 0 and ex == 1000 and a1 == 0 and a2 == 90000 and a3 >= 44999 and a3 <= 45001 and a4 == 90000 and g == 6 and l == 36 and p and np2 == 1024 and pc == 8 and lz == 63 and tz == 4:
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_imported_namespaced_builtins() {
    let src = r#"section .text:
  import math

  fn main():
    own value = math.snap(15, 8)
    own theta = math.atan2(1, 0)
    if value == 16 and theta == 90000:
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_multifile_hl_import() {
    let root = unique_temp_path("multifile_import");
    std::fs::create_dir_all(&root).expect("should create import fixture dir");

    let main_path = root.join("main.hl");
    let helper_path = root.join("helper.hl");

    std::fs::write(
      &helper_path,
      r#"section .text:
  fn helper_value():
    return 7
"#,
    )
    .expect("should write helper module");

    std::fs::write(
      &main_path,
      r#"section .text:
  import "./helper.hl"

  fn main():
    return helper_value()
"#,
    )
    .expect("should write main module");

    let program = parse_source_from_path(&main_path).expect("path-aware parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "7");
}

#[test]
fn parse_rejects_multifile_import_cycles() {
    let root = unique_temp_path("multifile_cycle");
    std::fs::create_dir_all(&root).expect("should create cycle fixture dir");

    let a_path = root.join("a.hl");
    let b_path = root.join("b.hl");

    std::fs::write(
      &a_path,
      r#"section .text:
  import "./b.hl"

  fn main():
    return 1
"#,
    )
    .expect("should write module a");

    std::fs::write(
      &b_path,
      r#"section .text:
  import "./a.hl"

  fn helper():
    return 2
"#,
    )
    .expect("should write module b");

    let err = parse_source_from_path(&a_path).expect_err("import cycle should fail parse");
    assert!(err.message.contains("Import cycle detected"));
}

  #[test]
  fn parses_and_runs_directory_imports() {
    let root = unique_temp_path("multifile_dir_import");
    let pkg_dir = root.join("pkg");
    std::fs::create_dir_all(&pkg_dir).expect("should create package dir");

    let main_path = root.join("main.hl");
    let helper_a = pkg_dir.join("a.hl");
    let helper_b = pkg_dir.join("b.hl");

    std::fs::write(
      &helper_a,
      r#"section .text:
  fn helper_a():
    return 2
"#,
    )
    .expect("should write helper a");

    std::fs::write(
      &helper_b,
      r#"section .text:
  fn helper_b():
    return 5
"#,
    )
    .expect("should write helper b");

    std::fs::write(
      &main_path,
      r#"section .text:
  import "./pkg"

  fn main():
    own sum = helper_a()
    add sum, helper_b()
    return sum
"#,
    )
    .expect("should write main module");

    let program = parse_source_from_path(&main_path).expect("directory import should parse");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "7");
  }

  #[test]
  fn parses_and_runs_glob_imports() {
    let root = unique_temp_path("multifile_glob_import");
    let lib_dir = root.join("lib");
    std::fs::create_dir_all(&lib_dir).expect("should create lib dir");

    let main_path = root.join("main.hl");
    let h1 = lib_dir.join("one.hl");
    let h2 = lib_dir.join("two.hl");

    std::fs::write(
      &h1,
      r#"section .text:
  fn one():
    return 10
"#,
    )
    .expect("should write one");

    std::fs::write(
      &h2,
      r#"section .text:
  fn two():
    return 32
"#,
    )
    .expect("should write two");

    std::fs::write(
      &main_path,
      r#"section .text:
  import "./lib/*.hl"

  fn main():
    own total = one()
    add total, two()
    return total
"#,
    )
    .expect("should write main");

    let program = parse_source_from_path(&main_path).expect("glob import should parse");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "42");
  }

  #[test]
  fn parses_and_runs_recursive_directory_imports() {
    let root = unique_temp_path("multifile_recursive_dir_import");
    let pkg_dir = root.join("pkg");
    let nested_dir = pkg_dir.join("nested");
    std::fs::create_dir_all(&nested_dir).expect("should create nested package dir");

    let main_path = root.join("main.hl");
    let root_mod = pkg_dir.join("root.hl");
    let nested_mod = nested_dir.join("value.hl");

    std::fs::write(
      &root_mod,
      r#"section .text:
  fn root_value():
    return 6
"#,
    )
    .expect("should write root module");

    std::fs::write(
      &nested_mod,
      r#"section .text:
  fn nested_value():
    return 36
"#,
    )
    .expect("should write nested module");

    std::fs::write(
      &main_path,
      r#"section .text:
  import "./pkg"

  fn main():
    own total = root_value()
    add total, nested_value()
    return total
"#,
    )
    .expect("should write main module");

    let program = parse_source_from_path(&main_path)
      .expect("recursive directory import should parse");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "42");
  }

  #[test]
  fn multifile_imports_deduplicate_overlapping_targets() {
    let root = unique_temp_path("multifile_overlap_dedupe");
    let lib_dir = root.join("lib");
    std::fs::create_dir_all(&lib_dir).expect("should create lib dir");

    let main_path = root.join("main.hl");
    let helper_path = lib_dir.join("helper.hl");

    std::fs::write(
      &helper_path,
      r#"section .text:
  fn helper_value():
    return 42
"#,
    )
    .expect("should write helper module");

    std::fs::write(
      &main_path,
      r#"section .text:
  import "./lib"
  import "./lib/helper.hl"

  fn main():
    return helper_value()
"#,
    )
    .expect("should write main module");

    let program = parse_source_from_path(&main_path)
      .expect("overlapping directory/file import should parse");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "42");
  }

#[test]
fn semantic_rejects_namespaced_builtin_without_import() {
    let src = r#"section .text:
  fn main():
    own value = math.snap(15, 8)
    return value
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("missing import should fail semantic analysis");
    assert!(err.message.contains("not imported"));
}

#[test]
fn semantic_rejects_break_outside_loop() {
    let src = r#"section .text:
  fn main():
    break
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("break outside loop should fail semantic analysis");
    assert!(err.message.contains("inside loop"));
}

#[test]
fn semantic_rejects_continue_outside_loop() {
    let src = r#"section .text:
  fn main():
    continue
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("continue outside loop should fail semantic analysis");
    assert!(err.message.contains("inside loop"));
}

#[test]
fn parses_and_runs_break_continue_in_loops() {
    let src = r#"section .text:
  fn main():
    own i = 0
    own sum = 0
    while i < 10:
      add i, 1
      if i == 3:
        continue
      if i == 8:
        break
      add sum, i
    return sum
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "25");
}

#[test]
fn semantic_errors_include_line_column_and_caret_snippet() {
    let src = r#"section .text:
  fn main():
    own value = math.snap(15, 8)
    return value
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("missing import should fail semantic analysis");
    let rendered = err.to_string();

    assert_eq!(err.line, 3);
    assert_eq!(err.column, 17);
    assert!(rendered.contains("at 3:17"));
    assert!(rendered.contains("own value = math.snap(15, 8)"));
    assert!(rendered.contains("^"));
}

#[test]
fn semantic_rejects_format_placeholder_arg_mismatch() {
    let src = r#"section .text:
  fn main():
    own x = format("a={} b={}", 1)
    return x
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("format placeholder mismatch should fail semantic analysis");
    assert!(err.message.contains("placeholder"));
}

#[test]
fn semantic_warnings_report_unused_symbols_and_ports_with_locations() {
    let src = r#"section .text:
  fn main():
    own temp = 42
    own [port_a]
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    let warnings = analyze_with_warnings(&program).expect("semantic analysis should pass");

    assert!(warnings
        .iter()
        .any(|w| w.message.contains("Variable `temp` declared but never used") && w.line == 3));
    assert!(warnings
        .iter()
        .any(|w| w.message.contains("Hardware port `port_a` is owned but never written to") && w.line == 4));
    assert!(warnings.iter().all(|w| w.to_string().contains("warning:")));
}

#[test]
fn semantic_allows_declaration_level_unused_suppression() {
    let src = r#"section .text:
  fn main():
    unused own intentionally_unused = 42
    own still_warns = 7
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    let warnings = analyze_with_warnings(&program).expect("semantic analysis should pass");

    assert!(!warnings
        .iter()
        .any(|w| w.message.contains("Variable `intentionally_unused` declared but never used")));
    assert!(warnings
        .iter()
        .any(|w| w.message.contains("Variable `still_warns` declared but never used")));
}

#[test]
fn parse_rejects_unused_modifier_on_non_declaration() {
    let src = r#"section .text:
  fn main():
    unused return 0
"#;

    let err = parse_source(src).expect_err("unused modifier on return should fail parse");
    assert!(err.message.contains("can only be applied to variable declarations"));
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

#[test]
fn semantic_allows_interrupt_port_access_when_yielded() {
    let src = r#"section .text:
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
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("yielded interrupt access should pass semantic analysis");
}

#[test]
fn semantic_rejects_interrupt_port_access_without_yield() {
    let src = r#"section .text:
  interrupt fn emergency_interrupt():
    own r1 = 7
    mov [port_a], r1
    return r1

  fn main():
    own [port_a]
    own r1 = 1
    mov [port_a], r1
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("interrupt write without yield should fail");
    assert!(err.message.contains("requires hardware ownership"));
}

#[test]
fn semantic_rejects_yield_to_non_interrupt_function() {
    let src = r#"section .text:
  fn helper():
    return 0

  fn main():
    own [port_a]
    yield [port_a] to helper:
      mov [port_a], 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("yield to non-interrupt should fail");
    assert!(err.message.contains("must be an `interrupt fn`"));
}

#[test]
fn semantic_rejects_interrupt_function_parameters() {
    let src = r#"section .text:
  interrupt fn emergency_interrupt(code):
    return code

  fn main():
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("interrupt functions with params should fail");
    assert!(err.message.contains("cannot declare parameters"));
}

#[test]
fn parses_and_runs_bitwise_and_sleep_until_builtin() {
    let src = r#"section .text:
  fn main():
    own woke = sleep_until("emergency_interrupt")
    own reg = ((1 << 5) | 3) & 0x1F
    if woke:
      return reg >> 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_desktop_file_and_utility_builtins() {
    let path = unique_temp_path("desktop_utils_pipeline.txt");
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
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_desktop_starter_api_scaffold_builtins() {
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
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_scripting_library_builtins() {
    let script_dir = unique_temp_path("script_lib_pipeline");
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
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_extended_scripting_and_string_builtins() {
    let root = unique_temp_path("script_ext_pipeline");
    let root_h = root.to_string_lossy().replace('\\', "/");

    let src = format!(
        r#"section .text:
  fn main():
    own root = "{}"
    own made_root = script_mkdir_all(root)
    own sub_dir = script_path_join(root, "sub")
    own made_sub = script_mkdir(sub_dir)
    own file = script_path_join(sub_dir, "data.txt")
    own wrote = write_text(file, "alpha\nbeta\n")

    own static_lines = split_lines("first\nsecond")
    own line_count = 0
    for line in static_lines:
      if len(trim(line)) > 0:
        add line_count, 1

    own captured = script_run_capture("echo line1")
    own captured_lines = script_run_capture_lines("echo line1")

    own starts = starts_with("foobar", "foo")
    own ends = ends_with("foobar", "bar")
    own left = pad_left("x", 3)
    own right = pad_right("x", 3)
    own rep = repeat_str("ab", 3)
    own idx = index_of("abcdef", "cd")

    own listed = script_list_dir(sub_dir)
    own listed_ok = contains(listed, "data.txt")

    own copied_path = script_path_join(sub_dir, "copy.txt")
    own copied = script_copy(file, copied_path)
    own moved_path = script_path_join(sub_dir, "moved.txt")
    own moved = script_move(copied_path, moved_path)

    own is_file_ok = script_is_file(moved_path)
    own is_dir_ok = script_is_dir(sub_dir)
    own exists_ok = script_exists(moved_path)

    own env_ok = script_env_set("H_TEST_SCRIPT_EXT", "ok")
    own env_val = env("H_TEST_SCRIPT_EXT")

    own pipe_code = script_pipe("echo piped", "sort")
    own removed = script_delete(root)
    own exists_after = script_exists(root)

    if made_root and made_sub and wrote and line_count == 2 and contains(captured, "line1") and iter_len(captured_lines) >= 1 and array_get(captured_lines, 0) == "line1" and starts and ends and left == "  x" and right == "x  " and rep == "ababab" and idx == 2 and listed_ok and copied and moved and is_file_ok and is_dir_ok and exists_ok and env_ok and env_val == "ok" and pipe_code == 0 and removed and (not exists_after):
      return 1
    return 0
"#,
        root_h
    );

    let program = parse_source(&src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn parses_and_runs_struct_construction_and_field_access() {
    let src = r#"section .text:
  struct SensorReading:
    value: int
    timestamp: int
    status: string

  fn main():
    own reading = SensorReading(42, now_ms(), "ok")
    own v = reading.value
    own s = reading.status
    if v == 42 and s == "ok":
      return 1
    return 0
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic analysis should pass");
    let result = run_program(&program).expect("runtime should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn semantic_rejects_unknown_struct_field_access() {
    let src = r#"section .text:
  struct SensorReading:
    value: int

  fn main():
    own reading = SensorReading(42)
    return reading.missing
"#;

    let program = parse_source(src).expect("parse should pass");
    let err = analyze(&program).expect_err("unknown field access should fail semantic analysis");
    assert!(err.message.contains("has no field"));
}
