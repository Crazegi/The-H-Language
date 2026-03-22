use hl_lexer::{analyze, analyze_with_warnings, parse_source, run_program};
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
