use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use hl_lexer::{
  analyze, compile_program, compile_program_with_options, load_cycle_profiles_from_file,
  diagnose_cycle_profile_coverage, parse_source, parse_source_from_path,
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
fn vm_executes_for_loops() {
    let src = r#"section .text:
  fn main():
    own acc = 0
    for i in 4:
      add acc, i
    return acc
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "6");
}

#[test]
fn vm_executes_for_loops_over_array_items() {
    let src = r#"section .text:
  fn main():
    own arr = array_new()
    arr = array_push(arr, "x")
    arr = array_push(arr, "y")
    arr = array_push(arr, "z")

    own total = 0
    for item in arr:
      own n = len(item)
      add total, n
    return total
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "3");
}

#[test]
fn vm_executes_format_builtin() {
    let src = r#"section .text:
  fn main():
    own sensor = "port_b"
    own reading = 7
    own msg = format("sensor={} reading={}", sensor, reading)
    if msg == "sensor=port_b reading=7":
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
fn vm_executes_multifile_hl_imports() {
    let root = unique_profile_path("vm_multifile_import");
    std::fs::create_dir_all(&root).expect("should create import fixture dir");

    let main_path = root.join("main.hl");
    let helper_path = root.join("helper.hl");

    std::fs::write(
      &helper_path,
      r#"section .text:
  fn helper_value():
    return 11
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
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "11");
}

#[test]
fn vm_executes_const_declarations() {
    let src = r#"section .text:
  fn main():
    const SENSOR = "port_c"
    const LIMIT = 3
    own msg = format("{}:{}", SENSOR, LIMIT)
    if msg == "port_c:3":
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
fn vm_executes_extended_scripting_and_string_builtins() {
    let root = unique_profile_path("script_ext_vm");
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
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn vm_executes_struct_construction_and_field_access() {
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
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "1");
}

#[test]
fn vm_executes_break_continue_in_loops() {
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
    analyze(&program).expect("semantic pass should pass");
    let bytecode = compile_program(&program).expect("compile should pass");
    let result = run_bytecode(&bytecode).expect("vm run should pass");
    assert_eq!(result.render(), "25");
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
fn vm_executes_embedded_hardware_library_builtins() {
    let src = r#"section .text:
  fn main():
    own pin = gpio_claim("[port_a]")
    own pin_cfg = gpio_mode(pin, "out")
    own wrote = gpio_write(pin, 1)
    own pin_state = gpio_read(pin)

    own uart = uart_new("uart0", 115200)
    own sent = uart_write(uart, "PING")
    own recv = uart_read(uart)

    own spi = spi_new("spi0", 1000000, 0)
    own echo = spi_transfer(spi, "AB")

    own i2c = i2c_new("i2c0", 400000)
    own i2c_ok = i2c_write(i2c, 0x40, "AA")
    own i2c_bytes = i2c_read(i2c, 0x40, 2)

    own timer = timer_new("t0", 1000)
    own timer_arm = timer_start(timer, 64)
    own elapsed = timer_elapsed(timer)

    own wd = watchdog_new("wd0", 250)
    own fed = watchdog_feed(wd)

    own dma = dma_new("ch0")
    own copied = dma_transfer(dma, "src", "dst", 128)

    if contains(pin_cfg, "mode=out") and wrote and (pin_state == 0 or pin_state == 1) and sent == 4 and recv == "uart_rx_stub" and echo == "AB" and i2c_ok and i2c_bytes == "40 40" and contains(timer_arm, "cycles=64") and elapsed >= 0 and fed and copied:
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
fn vm_executes_extended_math_and_bit_builtins() {
    let src = r#"section .text:
  fn main():
    own fx = to_float("12.750")
    own r = round(fx)
    own t = trunc(fx)
    own f = frac(fx)
    own snapped = snap(14, 8)

    own lg10 = log10(to_float("1000.000"))
    own nln = ln(to_float("2.718"))
    own ex = exp(to_float("1.000"))

    own a1 = asin(500)
    own a2 = acos(500)
    own a3 = atan(1000)
    own a4 = atan2(1, 1)

    own g = gcd(84, 30)
    own l = lcm(84, 30)
    own p = is_prime(97)
    own np2 = next_pow2(513)

    own pc = popcount(0xF0F0)
    own lz = leading_zeros(1)
    own tz = trailing_zeros(8)
    own br = bit_reverse(1)

    if r == 13 and t == 12 and f == 750 and snapped == 16 and lg10 == 3000 and nln >= 999 and nln <= 1001 and ex >= 2717 and ex <= 2719 and a1 >= 29999 and a1 <= 30001 and a2 >= 59999 and a2 <= 60001 and a3 >= 44999 and a3 <= 45001 and a4 >= 44999 and a4 <= 45001 and g == 6 and l == 420 and p and np2 == 1024 and pc == 8 and lz == 63 and tz == 3 and br < 0:
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
fn vm_executes_imported_namespaced_builtins() {
    let src = r#"section .text:
  import math
  import gpio

  fn main():
    own pin = gpio.claim("[port_a]")
    own _cfg = gpio.mode(pin, "out")
    own ok = gpio.write(pin, 1)
    own angle = math.atan2(1, 1)
    own rounded = math.round(to_float("2.600"))

    if ok and angle >= 44999 and angle <= 45001 and rounded == 3:
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
    assert_eq!(err.line, 10);
    assert_eq!(err.column, 5);
    let rendered = err.to_string();
    assert!(rendered.contains("at 10:5"));
    assert!(rendered.contains("execute:"));
    assert!(rendered.contains("^"));
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
