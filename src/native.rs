use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bytecode::{BytecodeProgram, Instruction};
use crate::compiler::{compile_program_with_options, CompileOptions};
use crate::evaluator::Value;
use crate::parser::parse_source;
use crate::semantic::analyze_with_warnings;

#[derive(Debug, Clone)]
pub struct NativeCompileError {
    pub message: String,
}

impl NativeCompileError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for NativeCompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for NativeCompileError {}

#[derive(Debug, Clone)]
pub struct NativeBuildArtifacts {
    pub rust_runtime_path: PathBuf,
    pub object_path: PathBuf,
    pub link_stub_path: PathBuf,
    pub executable_path: PathBuf,
}

pub fn compile_h_to_native_binary(source: &str, output_path: &Path) -> Result<PathBuf, NativeCompileError> {
    let artifacts = compile_h_to_native_artifacts(source, output_path)?;
    Ok(artifacts.executable_path)
}

pub fn compile_h_to_native_binary_with_options(
    source: &str,
    output_path: &Path,
    options: CompileOptions,
) -> Result<PathBuf, NativeCompileError> {
    let artifacts = compile_h_to_native_artifacts_with_options(source, output_path, options)?;
    Ok(artifacts.executable_path)
}

pub fn compile_h_to_native_artifacts(
    source: &str,
    output_path: &Path,
) -> Result<NativeBuildArtifacts, NativeCompileError> {
    compile_h_to_native_artifacts_with_options(source, output_path, CompileOptions::default())
}

pub fn compile_h_to_native_artifacts_with_options(
    source: &str,
    output_path: &Path,
    options: CompileOptions,
) -> Result<NativeBuildArtifacts, NativeCompileError> {
    let program = parse_source(source).map_err(|e| NativeCompileError::new(format!("Parse error: {}", e)))?;
    analyze_with_warnings(&program)
        .map_err(|e| NativeCompileError::new(format!("Semantic error: {}", e)))?;
    let bytecode = compile_program_with_options(&program, options)
        .map_err(|e| NativeCompileError::new(format!("Compile error: {}", e)))?;

    let rust_runtime_source = generate_rust_runtime_module(&bytecode.bytecode);
    let rust_runtime_file = output_path.with_extension("runtime.rs");
    std::fs::write(&rust_runtime_file, rust_runtime_source)
        .map_err(|e| NativeCompileError::new(format!("Failed to write generated source: {}", e)))?;

    let object_path = output_path.with_extension(object_extension());

    let rustc = find_rustc()?;
    let obj_output = Command::new(&rustc)
        .arg(&rust_runtime_file)
        .arg("--crate-name")
        .arg("hl_compiled_obj")
        .arg("--crate-type")
        .arg("lib")
        .arg("--emit=obj")
        .arg("-O")
        .arg("-o")
        .arg(&object_path)
        .output()
        .map_err(|e| NativeCompileError::new(format!("Failed to run rustc for object build: {}", e)))?;

    if !obj_output.status.success() {
        let stderr = String::from_utf8_lossy(&obj_output.stderr);
        let stdout = String::from_utf8_lossy(&obj_output.stdout);
        return Err(NativeCompileError::new(format!(
            "rustc object stage failed\nstdout:\n{}\nstderr:\n{}",
            stdout, stderr
        )));
    }

    let link_stub_source = generate_link_stub_source();
    let link_stub_file = output_path.with_extension("link.rs");
    std::fs::write(&link_stub_file, link_stub_source)
        .map_err(|e| NativeCompileError::new(format!("Failed to write linker stub: {}", e)))?;

    let link_output = Command::new(&rustc)
        .arg(&link_stub_file)
        .arg("--crate-name")
        .arg("hl_compiled_link")
        .arg("-C")
        .arg(format!("link-arg={}", object_path.display()))
        .arg("-O")
        .arg("-o")
        .arg(output_path)
        .output()
        .map_err(|e| NativeCompileError::new(format!("Failed to run rustc linker stage: {}", e)))?;

    if !link_output.status.success() {
        let stderr = String::from_utf8_lossy(&link_output.stderr);
        let stdout = String::from_utf8_lossy(&link_output.stdout);
        return Err(NativeCompileError::new(format!(
            "rustc link stage failed\nstdout:\n{}\nstderr:\n{}",
            stdout, stderr
        )));
    }

    Ok(NativeBuildArtifacts {
        rust_runtime_path: rust_runtime_file,
        object_path,
        link_stub_path: link_stub_file,
        executable_path: output_path.to_path_buf(),
    })
}

fn object_extension() -> &'static str {
    if cfg!(windows) {
        "obj"
    } else {
        "o"
    }
}

fn find_rustc() -> Result<PathBuf, NativeCompileError> {
    if let Ok(explicit) = std::env::var("RUSTC") {
        let pb = PathBuf::from(explicit);
        if pb.exists() {
            return Ok(pb);
        }
    }

    if let Ok(home) = std::env::var("USERPROFILE") {
        let candidate = PathBuf::from(home).join(".cargo").join("bin").join("rustc.exe");
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Ok(PathBuf::from("rustc"))
}

fn generate_rust_runtime_module(program: &BytecodeProgram) -> String {
    let mut out = String::new();
    out.push_str("use std::cmp::Ordering;\n");
    out.push_str("use std::collections::HashMap;\n\n");
    out.push_str("use std::fs::{self, OpenOptions};\n");
    out.push_str("use std::io::{self, Write};\n");
    out.push_str("use std::path::{Path, PathBuf};\n");
    out.push_str("use std::process::Command;\n");
    out.push_str("use std::thread;\n");
    out.push_str("use std::time::{Duration, SystemTime, UNIX_EPOCH};\n\n");

    out.push_str("#[derive(Clone, Debug)]\n");
    out.push_str("enum Value { Int(i64), Str(String), Bool(bool), Maybe, Ref(String), Unit }\n\n");

    out.push_str("#[derive(Clone, Debug)]\n");
    out.push_str("enum Instruction {\n");
    out.push_str("  PushInt(i64), PushStr(String), PushBool(bool), PushMaybe, PushUnit,\n");
    out.push_str("  LoadVar(String), DefineVar(String), StoreVar(String), StoreOrDefine(String),\n");
    out.push_str("  DeclareRef { name: String, target: String },\n");
    out.push_str("  Add, Sub, Mul, Div, Mod, Eq, Ne, Lt, Lte, Gt, Gte, And, Or, Xor, BitAnd, BitOr, Shl, Shr, Neg, Not, Cmp3,\n");
    out.push_str("  Jump(usize), JumpIfFalse(usize),\n");
    out.push_str("  Call(String, usize),\n");
    out.push_str("  PrintBegin, PrintField(String), PrintEnd, Nop, Pop, Return,\n");
    out.push_str("}\n\n");

    out.push_str("#[derive(Clone)]\n");
    out.push_str("struct Function { params: Vec<String>, code: Vec<Instruction> }\n\n");

    out.push_str("fn render(v: &Value) -> String {\n");
    out.push_str("  match v {\n");
    out.push_str("    Value::Int(n) => n.to_string(),\n");
    out.push_str("    Value::Str(s) => format!(\"\\\"{}\\\"\", s),\n");
    out.push_str("    Value::Bool(b) => b.to_string(),\n");
    out.push_str("    Value::Maybe => \"maybe\".to_string(),\n");
    out.push_str("    Value::Ref(name) => format!(\"&{}\", name),\n");
    out.push_str("    Value::Unit => \"unit\".to_string(),\n");
    out.push_str("  }\n}\n\n");

    out.push_str("fn pop(stack: &mut Vec<Value>) -> Result<Value, String> { stack.pop().ok_or_else(|| \"stack underflow\".to_string()) }\n\n");

    out.push_str("fn as_int(v: Value) -> Result<i64, String> { match v { Value::Int(n) => Ok(n), _ => Err(\"expected int\".to_string()) } }\n");
    out.push_str("fn as_bool(v: Value) -> Result<bool, String> { match v { Value::Bool(b) => Ok(b), Value::Int(n) => Ok(n != 0), Value::Maybe => Ok(false), _ => Err(\"expected bool-compatible value\".to_string()) } }\n\n");

    out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
    out.push_str("enum Logic3 { False, Maybe, True }\n\n");
    out.push_str("fn to_logic(v: &Value) -> Result<Logic3, String> {\n");
    out.push_str("  match v {\n");
    out.push_str("    Value::Bool(true) => Ok(Logic3::True),\n");
    out.push_str("    Value::Bool(false) => Ok(Logic3::False),\n");
    out.push_str("    Value::Int(n) => Ok(if *n == 0 { Logic3::False } else { Logic3::True }),\n");
    out.push_str("    Value::Maybe => Ok(Logic3::Maybe),\n");
    out.push_str("    _ => Err(\"expected logical-compatible value\".to_string()),\n");
    out.push_str("  }\n}\n\n");
    out.push_str("fn from_logic(v: Logic3) -> Value {\n");
    out.push_str("  match v { Logic3::True => Value::Bool(true), Logic3::False => Value::Bool(false), Logic3::Maybe => Value::Maybe }\n}\n\n");
    out.push_str("fn logic_and(a: Logic3, b: Logic3) -> Logic3 {\n");
    out.push_str("  match (a, b) { (Logic3::False, _) | (_, Logic3::False) => Logic3::False, (Logic3::True, x) | (x, Logic3::True) => x, (Logic3::Maybe, Logic3::Maybe) => Logic3::Maybe }\n}\n\n");
    out.push_str("fn logic_or(a: Logic3, b: Logic3) -> Logic3 {\n");
    out.push_str("  match (a, b) { (Logic3::True, _) | (_, Logic3::True) => Logic3::True, (Logic3::False, x) | (x, Logic3::False) => x, (Logic3::Maybe, Logic3::Maybe) => Logic3::Maybe }\n}\n\n");
    out.push_str("fn logic_xor(a: Logic3, b: Logic3) -> Logic3 {\n");
    out.push_str("  match (a, b) { (Logic3::Maybe, _) | (_, Logic3::Maybe) => Logic3::Maybe, (Logic3::True, Logic3::True) | (Logic3::False, Logic3::False) => Logic3::False, _ => Logic3::True }\n}\n\n");
    out.push_str("fn logic_phase(a: Logic3, b: Logic3) -> Logic3 { if a == b { a } else { Logic3::Maybe } }\n\n");

    out.push_str("fn builtin_int_arg(args: &[Value], idx: usize) -> Result<i64, String> {\n");
    out.push_str("  match args.get(idx) { Some(Value::Int(v)) => Ok(*v), _ => Err(\"expected integer argument\".to_string()) }\n");
    out.push_str("}\n\n");

    out.push_str("fn builtin_str_arg<'a>(args: &'a [Value], idx: usize) -> Result<&'a str, String> {\n");
    out.push_str("  match args.get(idx) { Some(Value::Str(v)) => Ok(v.as_str()), _ => Err(\"expected string argument\".to_string()) }\n");
    out.push_str("}\n\n");

    out.push_str("fn builtin_value_to_string(v: &Value) -> String {\n");
    out.push_str("  match v { Value::Int(n) => n.to_string(), Value::Str(s) => s.clone(), Value::Bool(b) => b.to_string(), Value::Maybe => \"maybe\".to_string(), Value::Ref(name) => format!(\"&{}\", name), Value::Unit => \"unit\".to_string() }\n");
    out.push_str("}\n\n");

    out.push_str("fn escape_json_string(input: &str) -> String { input.replace('\\\\', \"\\\\\\\\\").replace('\\\"', \"\\\\\\\"\") }\n\n");

    out.push_str("fn parse_json_field(json: &str, key: &str) -> Option<Value> {\n");
    out.push_str("  let key_pattern = format!(\"\\\"{}\\\"\", key);\n");
    out.push_str("  let key_pos = json.find(&key_pattern)?;\n");
    out.push_str("  let after_key = &json[key_pos + key_pattern.len()..];\n");
    out.push_str("  let colon_offset = after_key.find(':')?;\n");
    out.push_str("  let mut value_part = after_key[colon_offset + 1..].trim_start();\n");
    out.push_str("  if value_part.starts_with('\\\"') {\n");
    out.push_str("    value_part = &value_part[1..];\n");
    out.push_str("    let mut escaped = false;\n");
    out.push_str("    let mut out = String::new();\n");
    out.push_str("    for ch in value_part.chars() {\n");
    out.push_str("      if escaped { out.push(ch); escaped = false; continue; }\n");
    out.push_str("      if ch == '\\\\' { escaped = true; continue; }\n");
    out.push_str("      if ch == '\\\"' { return Some(Value::Str(out)); }\n");
    out.push_str("      out.push(ch);\n");
    out.push_str("    }\n");
    out.push_str("    return None;\n");
    out.push_str("  }\n");
    out.push_str("  if value_part.starts_with(\"true\") { return Some(Value::Bool(true)); }\n");
    out.push_str("  if value_part.starts_with(\"false\") { return Some(Value::Bool(false)); }\n");
    out.push_str("  if value_part.starts_with(\"null\") { return Some(Value::Maybe); }\n");
    out.push_str("  let end = value_part.find(|c: char| c == ',' || c == '}' || c.is_whitespace()).unwrap_or(value_part.len());\n");
    out.push_str("  let number = &value_part[..end];\n");
    out.push_str("  if let Ok(v) = number.parse::<i64>() { return Some(Value::Int(v)); }\n");
    out.push_str("  None\n");
    out.push_str("}\n\n");

    out.push_str("fn normalize_path(path: impl Into<PathBuf>) -> String { path.into().to_string_lossy().replace('\\\\', \"/\") }\n\n");

    out.push_str("fn shell_command(command: &str) -> Command {\n");
    out.push_str("  #[cfg(windows)]\n");
    out.push_str("  { let mut cmd = Command::new(\"cmd\"); cmd.arg(\"/C\").arg(command); return cmd; }\n");
    out.push_str("  #[cfg(not(windows))]\n");
    out.push_str("  { let mut cmd = Command::new(\"sh\"); cmd.arg(\"-c\").arg(command); return cmd; }\n");
    out.push_str("}\n\n");

    out.push_str("const SEQ_SEPARATOR: char = '\\u{001f}';\n");
    out.push_str("const SEQ_SEPARATOR_STR: &str = \"\\u{001f}\";\n");
    out.push_str("const RING_CAP_SEPARATOR: char = '#';\n\n");

    out.push_str("fn sequence_items(sequence: &str) -> Vec<String> { if sequence.is_empty() { Vec::new() } else { sequence.split(SEQ_SEPARATOR).map(|s| s.to_string()).collect() } }\n\n");
    out.push_str("fn push_sequence_item(sequence: &str, item: &str) -> String { if sequence.is_empty() { item.to_string() } else { format!(\"{}{}{}\", sequence, SEQ_SEPARATOR, item) } }\n\n");
    out.push_str("fn deg_to_rad(degrees: i64) -> f64 { (degrees as f64).to_radians() }\n\n");
    out.push_str("fn scaled_to_f64(value: i64) -> f64 { value as f64 / 1000.0 }\n\n");
    out.push_str("fn rad_to_milli_degrees(rad: f64) -> i64 { (rad.to_degrees() * 1000.0).round() as i64 }\n\n");
    out.push_str("fn f64_to_scaled(value: f64, op: &str) -> Result<Value, String> { let scaled = value * 1000.0; if !scaled.is_finite() { return Err(format!(\"{} failed: value is not finite\", op)); } if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 { return Err(format!(\"{} failed: value out of range\", op)); } Ok(Value::Int(scaled.round() as i64)) }\n\n");
    out.push_str("fn snap_to_step(value: i64, step: i64) -> i64 { let lower = value.div_euclid(step) * step; let upper = lower.saturating_add(step); let dl = (value - lower).abs(); let du = (upper - value).abs(); if du <= dl { upper } else { lower } }\n\n");
    out.push_str("fn gcd_i64(a: i64, b: i64) -> i64 { let mut x = a.abs(); let mut y = b.abs(); while y != 0 { let t = x % y; x = y; y = t; } x }\n\n");
    out.push_str("fn lcm_i64(a: i64, b: i64) -> Result<i64, String> { if a == 0 || b == 0 { return Ok(0); } let g = gcd_i64(a, b); let scaled = (a / g).checked_mul(b).ok_or_else(|| \"lcm overflowed 64-bit range\".to_string())?; Ok(scaled.abs()) }\n\n");
    out.push_str("fn is_prime_i64(n: i64) -> bool { if n < 2 { return false; } if n == 2 { return true; } if n % 2 == 0 { return false; } let mut d = 3i64; while d <= n / d { if n % d == 0 { return false; } d += 2; } true }\n\n");
    out.push_str("fn parse_scaled_thousand(raw: &str) -> Result<i64, String> { let parsed = raw.trim().parse::<f64>().map_err(|e| format!(\"to_float parse failed: {}\", e))?; let scaled = parsed * 1000.0; if !scaled.is_finite() { return Err(\"to_float parse failed: value is not finite\".to_string()); } if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 { return Err(\"to_float parse failed: value out of range\".to_string()); } Ok(scaled.round() as i64) }\n\n");
    out.push_str("fn format_scaled_thousand(value: i64) -> String { let negative = value < 0; let abs = value.unsigned_abs(); let whole = abs / 1000; let frac = abs % 1000; if negative { format!(\"-{}.{:03}\", whole, frac) } else { format!(\"{}.{:03}\", whole, frac) } }\n\n");
    out.push_str("fn parse_ring(raw: &str) -> Result<(i64, Vec<String>), String> { let mut parts = raw.splitn(2, RING_CAP_SEPARATOR); let cap_part = parts.next().unwrap_or_default().trim(); let payload = parts.next().unwrap_or_default(); if cap_part.is_empty() { return Err(\"ring value is invalid: missing capacity\".to_string()); } let capacity = cap_part.parse::<i64>().map_err(|e| format!(\"ring value is invalid: {}\", e))?; if capacity <= 0 { return Err(\"ring value is invalid: capacity must be > 0\".to_string()); } Ok((capacity, sequence_items(payload))) }\n\n");
    out.push_str("fn format_ring(capacity: i64, items: &[String]) -> String { format!(\"{}{}{}\", capacity, RING_CAP_SEPARATOR, items.join(SEQ_SEPARATOR_STR)) }\n\n");
    out.push_str("fn is_memory_target_syntax(value: &str) -> bool { value.starts_with('[') && value.ends_with(']') && value.len() > 2 }\n\n");
    out.push_str("fn ensure_handle_prefix(handle: &str, prefix: &str, builtin_name: &str) -> Result<(), String> { if handle.starts_with(prefix) { Ok(()) } else { Err(format!(\"{} expects handle produced by matching *_new builtin\", builtin_name)) } }\n\n");
    out.push_str("fn ensure_gpio_handle(handle: &str) -> Result<(), String> { if handle.starts_with(\"gpio:[\") && handle.contains(\"]:owned\") { Ok(()) } else { Err(\"gpio builtin expects handle from gpio_claim\".to_string()) } }\n\n");
    out.push_str("fn stable_hash(input: &str) -> u64 { let mut hash = 1469598103934665603u64; for b in input.as_bytes() { hash ^= *b as u64; hash = hash.wrapping_mul(1099511628211u64); } hash }\n\n");

    out.push_str("fn call_builtin(name: &str, args: &[Value]) -> Result<Option<Value>, String> {\n");
    out.push_str("  let out = match name {\n");
    out.push_str("    \"abs\" => Value::Int(builtin_int_arg(args, 0)?.abs()),\n");
    out.push_str("    \"sqrt\" => { let n = builtin_int_arg(args, 0)?; if n < 0 { return Err(\"sqrt expects non-negative integer\".to_string()); } Value::Int((n as f64).sqrt().floor() as i64) },\n");
    out.push_str("    \"floor\" => Value::Int(builtin_int_arg(args, 0)?),\n");
    out.push_str("    \"ceil\" => Value::Int(builtin_int_arg(args, 0)?),\n");
    out.push_str("    \"round\" => { let scaled = builtin_int_arg(args, 0)?; Value::Int(scaled_to_f64(scaled).round() as i64) },\n");
    out.push_str("    \"trunc\" => { let scaled = builtin_int_arg(args, 0)?; Value::Int(scaled / 1000) },\n");
    out.push_str("    \"frac\" => { let scaled = builtin_int_arg(args, 0)?; Value::Int(scaled - (scaled / 1000) * 1000) },\n");
    out.push_str("    \"snap\" => { let value = builtin_int_arg(args, 0)?; let step = builtin_int_arg(args, 1)?; if step == 0 { return Err(\"snap expects non-zero step\".to_string()); } Value::Int(snap_to_step(value, step.abs())) },\n");
    out.push_str("    \"log2\" => { let n = builtin_int_arg(args, 0)?; if n <= 0 { return Err(\"log2 expects positive integer\".to_string()); } Value::Int((i64::BITS - 1 - n.leading_zeros()) as i64) },\n");
    out.push_str("    \"log10\" => { let scaled = builtin_int_arg(args, 0)?; if scaled <= 0 { return Err(\"log10 expects positive fixed-point value\".to_string()); } f64_to_scaled(scaled_to_f64(scaled).log10(), \"log10\")? },\n");
    out.push_str("    \"ln\" => { let scaled = builtin_int_arg(args, 0)?; if scaled <= 0 { return Err(\"ln expects positive fixed-point value\".to_string()); } f64_to_scaled(scaled_to_f64(scaled).ln(), \"ln\")? },\n");
    out.push_str("    \"exp\" => { let scaled = builtin_int_arg(args, 0)?; f64_to_scaled(scaled_to_f64(scaled).exp(), \"exp\")? },\n");
    out.push_str("    \"sin\" => { let deg = builtin_int_arg(args, 0)?; Value::Int((deg_to_rad(deg).sin() * 1000.0).round() as i64) },\n");
    out.push_str("    \"cos\" => { let deg = builtin_int_arg(args, 0)?; Value::Int((deg_to_rad(deg).cos() * 1000.0).round() as i64) },\n");
    out.push_str("    \"tan\" => { let deg = builtin_int_arg(args, 0)?; let cos = deg_to_rad(deg).cos(); if cos.abs() < 1e-9 { return Err(\"tan is undefined for this angle\".to_string()); } Value::Int((deg_to_rad(deg).tan() * 1000.0).round() as i64) },\n");
    out.push_str("    \"asin\" => { let x = builtin_int_arg(args, 0)?; if !(-1000..=1000).contains(&x) { return Err(\"asin expects fixed-point input in range [-1000, 1000]\".to_string()); } Value::Int(rad_to_milli_degrees((scaled_to_f64(x)).asin())) },\n");
    out.push_str("    \"acos\" => { let x = builtin_int_arg(args, 0)?; if !(-1000..=1000).contains(&x) { return Err(\"acos expects fixed-point input in range [-1000, 1000]\".to_string()); } Value::Int(rad_to_milli_degrees((scaled_to_f64(x)).acos())) },\n");
    out.push_str("    \"atan\" => { let x = builtin_int_arg(args, 0)?; Value::Int(rad_to_milli_degrees((scaled_to_f64(x)).atan())) },\n");
    out.push_str("    \"atan2\" => { let y = builtin_int_arg(args, 0)?; let x = builtin_int_arg(args, 1)?; Value::Int(rad_to_milli_degrees((y as f64).atan2(x as f64))) },\n");
    out.push_str("    \"gcd\" => { let a = builtin_int_arg(args, 0)?; let b = builtin_int_arg(args, 1)?; Value::Int(gcd_i64(a, b)) },\n");
    out.push_str("    \"lcm\" => { let a = builtin_int_arg(args, 0)?; let b = builtin_int_arg(args, 1)?; Value::Int(lcm_i64(a, b)?) },\n");
    out.push_str("    \"is_prime\" => { let n = builtin_int_arg(args, 0)?; Value::Bool(is_prime_i64(n)) },\n");
    out.push_str("    \"next_pow2\" => { let n = builtin_int_arg(args, 0)?; if n <= 0 { return Err(\"next_pow2 expects positive integer\".to_string()); } let n_u = n as u64; let next = n_u.checked_next_power_of_two().ok_or_else(|| \"next_pow2 overflowed 64-bit range\".to_string())?; if next > i64::MAX as u64 { return Err(\"next_pow2 result exceeds signed 64-bit range\".to_string()); } Value::Int(next as i64) },\n");
    out.push_str("    \"popcount\" => { let n = builtin_int_arg(args, 0)?; Value::Int((n as u64).count_ones() as i64) },\n");
    out.push_str("    \"leading_zeros\" => { let n = builtin_int_arg(args, 0)?; Value::Int((n as u64).leading_zeros() as i64) },\n");
    out.push_str("    \"trailing_zeros\" => { let n = builtin_int_arg(args, 0)?; Value::Int((n as u64).trailing_zeros() as i64) },\n");
    out.push_str("    \"bit_reverse\" => { let n = builtin_int_arg(args, 0)?; Value::Int(i64::from_ne_bytes((n as u64).reverse_bits().to_ne_bytes())) },\n");
    out.push_str("    \"min\" => Value::Int(builtin_int_arg(args, 0)?.min(builtin_int_arg(args, 1)?)),\n");
    out.push_str("    \"max\" => Value::Int(builtin_int_arg(args, 0)?.max(builtin_int_arg(args, 1)?)),\n");
    out.push_str("    \"pow\" => { let base = builtin_int_arg(args, 0)?; let exp = builtin_int_arg(args, 1)?; if exp < 0 { return Err(\"pow exponent must be non-negative\".to_string()); } Value::Int(base.pow(exp as u32)) },\n");
    out.push_str("    \"clamp\" => { let v = builtin_int_arg(args, 0)?; let lo = builtin_int_arg(args, 1)?; let hi = builtin_int_arg(args, 2)?; Value::Int(v.clamp(lo, hi)) },\n");
    out.push_str("    \"len\" => Value::Int(builtin_str_arg(args, 0)?.chars().count() as i64),\n");
    out.push_str("    \"upper\" => Value::Str(builtin_str_arg(args, 0)?.to_uppercase()),\n");
    out.push_str("    \"lower\" => Value::Str(builtin_str_arg(args, 0)?.to_lowercase()),\n");
    out.push_str("    \"contains\" => Value::Bool(builtin_str_arg(args, 0)?.contains(builtin_str_arg(args, 1)?)),\n");
    out.push_str("    \"split\" => { let source = builtin_str_arg(args, 0)?; let delimiter = builtin_str_arg(args, 1)?; if delimiter.is_empty() { return Err(\"split expects non-empty delimiter\".to_string()); } Value::Str(source.split(delimiter).collect::<Vec<_>>().join(SEQ_SEPARATOR_STR)) },\n");
    out.push_str("    \"join\" => { let sequence = builtin_str_arg(args, 0)?; let delimiter = builtin_str_arg(args, 1)?; Value::Str(sequence_items(sequence).join(delimiter)) },\n");
    out.push_str("    \"phase\" => { let a = to_logic(args.get(0).ok_or_else(|| \"missing argument\".to_string())?)?; let b = to_logic(args.get(1).ok_or_else(|| \"missing argument\".to_string())?)?; from_logic(logic_phase(a, b)) },\n");
    out.push_str("    \"collapse\" => { let v = args.get(0).ok_or_else(|| \"missing argument\".to_string())?; Value::Bool(matches!(to_logic(v)?, Logic3::True)) },\n");
    out.push_str("    \"sleep_until\" => { if args.get(0).is_none() { return Err(\"missing argument\".to_string()); } Value::Bool(true) },\n");
    out.push_str("    \"sleep_ms\" => { let ms = builtin_int_arg(args, 0)?; if ms < 0 { return Err(\"sleep_ms expects non-negative milliseconds\".to_string()); } thread::sleep(Duration::from_millis(ms as u64)); Value::Unit },\n");
    out.push_str("    \"now_ms\" => { let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| \"system time is before UNIX_EPOCH\".to_string())?; Value::Int(now.as_millis() as i64) },\n");
    out.push_str("    \"rand_int\" => { let lo = builtin_int_arg(args, 0)?; let hi = builtin_int_arg(args, 1)?; if lo > hi { return Err(\"rand_int expects lo <= hi\".to_string()); } let span = (hi - lo + 1) as u64; let seed = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| \"system time is before UNIX_EPOCH\".to_string())?.as_nanos() as u64; let mixed = seed ^ (seed.rotate_left(13)).wrapping_mul(0x9E37_79B9_7F4A_7C15); Value::Int(lo + (mixed % span) as i64) },\n");
    out.push_str("    \"input\" => { let prompt = builtin_str_arg(args, 0)?; print!(\"{}\", prompt); io::stdout().flush().map_err(|e| format!(\"input flush failed: {}\", e))?; let mut line = String::new(); io::stdin().read_line(&mut line).map_err(|e| format!(\"input read failed: {}\", e))?; while line.ends_with('\\n') || line.ends_with('\\r') { line.pop(); } Value::Str(line) },\n");
    out.push_str("    \"read_text\" => { let path = builtin_str_arg(args, 0)?; let content = fs::read_to_string(path).map_err(|e| format!(\"read_text failed for `{}`: {}\", path, e))?; Value::Str(content) },\n");
    out.push_str("    \"write_text\" => { let path = builtin_str_arg(args, 0)?; let content = builtin_str_arg(args, 1)?; fs::write(path, content).map_err(|e| format!(\"write_text failed for `{}`: {}\", path, e))?; Value::Bool(true) },\n");
    out.push_str("    \"append_text\" => { let path = builtin_str_arg(args, 0)?; let content = builtin_str_arg(args, 1)?; let mut file = OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!(\"append_text open failed for `{}`: {}\", path, e))?; file.write_all(content.as_bytes()).map_err(|e| format!(\"append_text write failed for `{}`: {}\", path, e))?; Value::Bool(true) },\n");
    out.push_str("    \"exists\" => { let path = builtin_str_arg(args, 0)?; Value::Bool(std::path::Path::new(path).exists()) },\n");
    out.push_str("    \"delete_file\" => { let path = builtin_str_arg(args, 0)?; match fs::remove_file(path) { Ok(_) => Value::Bool(true), Err(e) if e.kind() == io::ErrorKind::NotFound => Value::Bool(false), Err(e) => return Err(format!(\"delete_file failed for `{}`: {}\", path, e)) } },\n");
    out.push_str("    \"env\" => { let key = builtin_str_arg(args, 0)?; Value::Str(std::env::var(key).unwrap_or_default()) },\n");
    out.push_str("    \"to_int\" => { let raw = builtin_str_arg(args, 0)?; let parsed = raw.trim().parse::<i64>().map_err(|e| format!(\"to_int parse failed: {}\", e))?; Value::Int(parsed) },\n");
    out.push_str("    \"to_bool\" => { let raw = builtin_str_arg(args, 0)?.trim().to_ascii_lowercase(); let parsed = match raw.as_str() { \"1\" | \"true\" | \"yes\" | \"on\" => true, \"0\" | \"false\" | \"no\" | \"off\" => false, _ => return Err(\"to_bool parse failed: expected true/false-like string\".to_string()) }; Value::Bool(parsed) },\n");
    out.push_str("    \"to_float\" => { let raw = builtin_str_arg(args, 0)?; Value::Int(parse_scaled_thousand(raw)?) },\n");
    out.push_str("    \"to_string\" => { let v = args.get(0).ok_or_else(|| \"missing argument\".to_string())?; Value::Str(builtin_value_to_string(v)) },\n");
    out.push_str("    \"to_float_string\" => { let scaled = builtin_int_arg(args, 0)?; Value::Str(format_scaled_thousand(scaled)) },\n");
    out.push_str("    \"trim\" => { let s = builtin_str_arg(args, 0)?; Value::Str(s.trim().to_string()) },\n");
    out.push_str("    \"replace\" => { let source = builtin_str_arg(args, 0)?; let from = builtin_str_arg(args, 1)?; let to = builtin_str_arg(args, 2)?; Value::Str(source.replace(from, to)) },\n");
    out.push_str("    \"format\" => { let template = builtin_str_arg(args, 0)?; let placeholders = template.matches(\"{}\").count(); let provided = args.len().saturating_sub(1); if placeholders != provided { return Err(format!(\"format expects {} value(s) for template placeholders, got {}\", placeholders, provided)); } let mut rendered = String::new(); let mut rest = template; for value in &args[1..] { if let Some(pos) = rest.find(\"{}\") { rendered.push_str(&rest[..pos]); rendered.push_str(&builtin_value_to_string(value)); rest = &rest[pos + 2..]; } } rendered.push_str(rest); Value::Str(rendered) },\n");
    out.push_str("    \"iter_len\" => { let iterable = args.get(0).ok_or_else(|| \"missing argument\".to_string())?; match iterable { Value::Int(v) => { if *v < 0 { return Err(\"iter_len expects non-negative integer range\".to_string()); } Value::Int(*v) }, Value::Str(raw) => { if let Ok((_, items)) = parse_ring(raw) { Value::Int(items.len() as i64) } else { Value::Int(sequence_items(raw).len() as i64) } }, _ => return Err(\"iter_len expects integer range or string-backed collection\".to_string()) } },\n");
    out.push_str("    \"iter_get\" => { let iterable = args.get(0).ok_or_else(|| \"missing argument\".to_string())?; let idx = builtin_int_arg(args, 1)?; if idx < 0 { return Err(\"iter_get expects non-negative index\".to_string()); } match iterable { Value::Int(v) => { if *v < 0 { return Err(\"iter_get expects non-negative integer range\".to_string()); } if idx >= *v { Value::Maybe } else { Value::Int(idx) } }, Value::Str(raw) => { let items = if let Ok((_, items)) = parse_ring(raw) { items } else { sequence_items(raw) }; match items.get(idx as usize) { Some(v) => Value::Str(v.clone()), None => Value::Maybe } }, _ => return Err(\"iter_get expects integer range or string-backed collection\".to_string()) } },\n");
    out.push_str("    \"array_new\" => Value::Str(String::new()),\n");
    out.push_str("    \"array_len\" => Value::Int(sequence_items(builtin_str_arg(args, 0)?).len() as i64),\n");
    out.push_str("    \"array_push\" => { let sequence = builtin_str_arg(args, 0)?; let item = builtin_str_arg(args, 1)?; Value::Str(push_sequence_item(sequence, item)) },\n");
    out.push_str("    \"array_get\" => { let sequence = builtin_str_arg(args, 0)?; let idx = builtin_int_arg(args, 1)?; if idx < 0 { return Err(\"array_get expects non-negative index\".to_string()); } match sequence_items(sequence).get(idx as usize) { Some(v) => Value::Str(v.clone()), None => Value::Maybe } },\n");
    out.push_str("    \"queue_new\" => Value::Str(String::new()),\n");
    out.push_str("    \"queue_len\" => Value::Int(sequence_items(builtin_str_arg(args, 0)?).len() as i64),\n");
    out.push_str("    \"queue_push\" => { let sequence = builtin_str_arg(args, 0)?; let item = builtin_str_arg(args, 1)?; Value::Str(push_sequence_item(sequence, item)) },\n");
    out.push_str("    \"queue_peek\" => { let sequence = builtin_str_arg(args, 0)?; match sequence_items(sequence).first() { Some(v) => Value::Str(v.clone()), None => Value::Maybe } },\n");
    out.push_str("    \"queue_pop\" => { let sequence = builtin_str_arg(args, 0)?; let mut items = sequence_items(sequence); if !items.is_empty() { items.remove(0); } Value::Str(items.join(SEQ_SEPARATOR_STR)) },\n");
    out.push_str("    \"ring_new\" => { let cap = builtin_int_arg(args, 0)?; if cap <= 0 { return Err(\"ring_new expects capacity > 0\".to_string()); } Value::Str(format!(\"{}{}\", cap, RING_CAP_SEPARATOR)) },\n");
    out.push_str("    \"ring_len\" => { let ring = builtin_str_arg(args, 0)?; let (_, items) = parse_ring(ring)?; Value::Int(items.len() as i64) },\n");
    out.push_str("    \"ring_push\" => { let ring = builtin_str_arg(args, 0)?; let item = builtin_str_arg(args, 1)?; let (capacity, mut items) = parse_ring(ring)?; items.push(item.to_string()); while items.len() > capacity as usize { items.remove(0); } Value::Str(format_ring(capacity, &items)) },\n");
    out.push_str("    \"ring_peek\" => { let ring = builtin_str_arg(args, 0)?; let (_, items) = parse_ring(ring)?; match items.first() { Some(v) => Value::Str(v.clone()), None => Value::Maybe } },\n");
    out.push_str("    \"gpio_claim\" => { let port = builtin_str_arg(args, 0)?; if !is_memory_target_syntax(port) { return Err(\"gpio_claim expects memory-target style port like `[port_a]`\".to_string()); } Value::Str(format!(\"gpio:{}:owned\", port)) },\n");
    out.push_str("    \"gpio_mode\" => { let handle = builtin_str_arg(args, 0)?; let mode = builtin_str_arg(args, 1)?; let allowed = [\"in\", \"out\", \"pullup\", \"pulldown\"]; if !allowed.contains(&mode) { return Err(\"gpio_mode expects one of: in, out, pullup, pulldown\".to_string()); } ensure_gpio_handle(handle)?; Value::Str(format!(\"{}:mode={}\", handle, mode)) },\n");
    out.push_str("    \"gpio_write\" => { let handle = builtin_str_arg(args, 0)?; let value = builtin_int_arg(args, 1)?; if value != 0 && value != 1 { return Err(\"gpio_write expects value 0 or 1\".to_string()); } ensure_gpio_handle(handle)?; Value::Bool(true) },\n");
    out.push_str("    \"gpio_read\" => { let handle = builtin_str_arg(args, 0)?; ensure_gpio_handle(handle)?; Value::Int((stable_hash(handle) & 1) as i64) },\n");
    out.push_str("    \"uart_new\" => { let bus = builtin_str_arg(args, 0)?; let baud = builtin_int_arg(args, 1)?; if baud <= 0 { return Err(\"uart_new expects baud > 0\".to_string()); } Value::Str(format!(\"uart:{}:baud={}\", bus, baud)) },\n");
    out.push_str("    \"uart_write\" => { let uart = builtin_str_arg(args, 0)?; let payload = builtin_str_arg(args, 1)?; ensure_handle_prefix(uart, \"uart:\", \"uart_write\")?; Value::Int(payload.len() as i64) },\n");
    out.push_str("    \"uart_read\" => { let uart = builtin_str_arg(args, 0)?; ensure_handle_prefix(uart, \"uart:\", \"uart_read\")?; Value::Str(\"uart_rx_stub\".to_string()) },\n");
    out.push_str("    \"spi_new\" => { let bus = builtin_str_arg(args, 0)?; let hz = builtin_int_arg(args, 1)?; let mode = builtin_int_arg(args, 2)?; if hz <= 0 { return Err(\"spi_new expects hz > 0\".to_string()); } if !(0..=3).contains(&mode) { return Err(\"spi_new expects mode in range 0..3\".to_string()); } Value::Str(format!(\"spi:{}:hz={}:mode={}\", bus, hz, mode)) },\n");
    out.push_str("    \"spi_transfer\" => { let spi = builtin_str_arg(args, 0)?; let payload = builtin_str_arg(args, 1)?; ensure_handle_prefix(spi, \"spi:\", \"spi_transfer\")?; Value::Str(payload.to_string()) },\n");
    out.push_str("    \"i2c_new\" => { let bus = builtin_str_arg(args, 0)?; let hz = builtin_int_arg(args, 1)?; if hz <= 0 { return Err(\"i2c_new expects hz > 0\".to_string()); } Value::Str(format!(\"i2c:{}:hz={}\", bus, hz)) },\n");
    out.push_str("    \"i2c_write\" => { let i2c = builtin_str_arg(args, 0)?; let address = builtin_int_arg(args, 1)?; let _payload = builtin_str_arg(args, 2)?; ensure_handle_prefix(i2c, \"i2c:\", \"i2c_write\")?; if !(0..=0x7F).contains(&address) { return Err(\"i2c_write expects 7-bit address in range 0..127\".to_string()); } Value::Bool(true) },\n");
    out.push_str("    \"i2c_read\" => { let i2c = builtin_str_arg(args, 0)?; let address = builtin_int_arg(args, 1)?; let count = builtin_int_arg(args, 2)?; ensure_handle_prefix(i2c, \"i2c:\", \"i2c_read\")?; if !(0..=0x7F).contains(&address) { return Err(\"i2c_read expects 7-bit address in range 0..127\".to_string()); } if count < 0 { return Err(\"i2c_read expects non-negative byte count\".to_string()); } let byte = format!(\"{:02X}\", address); let mut out = Vec::with_capacity(count as usize); for _ in 0..count { out.push(byte.clone()); } Value::Str(out.join(\" \")) },\n");
    out.push_str("    \"timer_new\" => { let name = builtin_str_arg(args, 0)?; let hz = builtin_int_arg(args, 1)?; if hz <= 0 { return Err(\"timer_new expects hz > 0\".to_string()); } Value::Str(format!(\"timer:{}:hz={}\", name, hz)) },\n");
    out.push_str("    \"timer_start\" => { let timer = builtin_str_arg(args, 0)?; let cycles = builtin_int_arg(args, 1)?; ensure_handle_prefix(timer, \"timer:\", \"timer_start\")?; if cycles <= 0 { return Err(\"timer_start expects cycles > 0\".to_string()); } Value::Str(format!(\"{}:cycles={}\", timer, cycles)) },\n");
    out.push_str("    \"timer_elapsed\" => { let timer = builtin_str_arg(args, 0)?; ensure_handle_prefix(timer, \"timer:\", \"timer_elapsed\")?; let now = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| \"system time is before UNIX_EPOCH\".to_string())?; Value::Int((now.as_micros() % 1_000_000) as i64) },\n");
    out.push_str("    \"watchdog_new\" => { let name = builtin_str_arg(args, 0)?; let timeout_ms = builtin_int_arg(args, 1)?; if timeout_ms <= 0 { return Err(\"watchdog_new expects timeout_ms > 0\".to_string()); } Value::Str(format!(\"watchdog:{}:ms={}\", name, timeout_ms)) },\n");
    out.push_str("    \"watchdog_feed\" => { let watchdog = builtin_str_arg(args, 0)?; ensure_handle_prefix(watchdog, \"watchdog:\", \"watchdog_feed\")?; Value::Bool(true) },\n");
    out.push_str("    \"dma_new\" => { let channel = builtin_str_arg(args, 0)?; Value::Str(format!(\"dma:{}\", channel)) },\n");
    out.push_str("    \"dma_transfer\" => { let dma = builtin_str_arg(args, 0)?; let _src = builtin_str_arg(args, 1)?; let _dst = builtin_str_arg(args, 2)?; let bytes = builtin_int_arg(args, 3)?; ensure_handle_prefix(dma, \"dma:\", \"dma_transfer\")?; if bytes < 0 { return Err(\"dma_transfer expects non-negative byte count\".to_string()); } Value::Bool(true) },\n");
    out.push_str("    \"window_loop\" => { let title = builtin_str_arg(args, 0)?; let ticks = builtin_int_arg(args, 1)?; if ticks < 0 { return Err(\"window_loop expects non-negative tick count\".to_string()); } Value::Str(format!(\"window:{}:ticks={}\", title, ticks)) },\n");
    out.push_str("    \"menu\" => { let _title = builtin_str_arg(args, 0)?; let options = builtin_str_arg(args, 1)?; let first = options.split('|').next().map(|s| s.trim().to_string()).unwrap_or_default(); if first.is_empty() { Value::Maybe } else { Value::Str(first) } },\n");
    out.push_str("    \"http_get\" => { let url = builtin_str_arg(args, 0)?; let escaped = escape_json_string(url); Value::Str(format!(\"{{\\\"status\\\":200,\\\"url\\\":\\\"{}\\\",\\\"body\\\":\\\"stub\\\"}}\", escaped)) },\n");
    out.push_str("    \"json_parse\" => { let json = builtin_str_arg(args, 0)?; let key = builtin_str_arg(args, 1)?; parse_json_field(json, key).unwrap_or(Value::Maybe) },\n");
    out.push_str("    \"script_args_count\" => Value::Int(std::env::args().count() as i64),\n");
    out.push_str("    \"script_arg\" => { let idx = builtin_int_arg(args, 0)?; if idx < 0 { return Err(\"script_arg expects non-negative index\".to_string()); } Value::Str(std::env::args().nth(idx as usize).unwrap_or_default()) },\n");
    out.push_str("    \"script_cwd\" => { let cwd = std::env::current_dir().map_err(|e| format!(\"script_cwd failed: {}\", e))?; Value::Str(normalize_path(cwd)) },\n");
    out.push_str("    \"script_chdir\" => { let path = builtin_str_arg(args, 0)?; std::env::set_current_dir(path).map_err(|e| format!(\"script_chdir failed for `{}`: {}\", path, e))?; Value::Bool(true) },\n");
    out.push_str("    \"script_path_join\" => { let base = builtin_str_arg(args, 0)?; let child = builtin_str_arg(args, 1)?; Value::Str(normalize_path(Path::new(base).join(child))) },\n");
    out.push_str("    \"script_dirname\" => { let path = builtin_str_arg(args, 0)?; let parent = Path::new(path).parent().map(normalize_path).unwrap_or_default(); Value::Str(parent) },\n");
    out.push_str("    \"script_basename\" => { let path = builtin_str_arg(args, 0)?; let name = Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string(); Value::Str(name) },\n");
    out.push_str("    \"script_run\" => { let command = builtin_str_arg(args, 0)?; let status = shell_command(command).status().map_err(|e| format!(\"script_run failed for `{}`: {}\", command, e))?; Value::Int(status.code().unwrap_or(-1) as i64) },\n");
    out.push_str("    \"script_run_capture\" => { let command = builtin_str_arg(args, 0)?; let output = shell_command(command).output().map_err(|e| format!(\"script_run_capture failed for `{}`: {}\", command, e))?; let mut out_s = String::new(); out_s.push_str(&String::from_utf8_lossy(&output.stdout)); out_s.push_str(&String::from_utf8_lossy(&output.stderr)); while out_s.ends_with('\\n') || out_s.ends_with('\\r') { out_s.pop(); } Value::Str(out_s) },\n");
    out.push_str("    _ => return Ok(None),\n");
    out.push_str("  };\n");
    out.push_str("  Ok(Some(out))\n");
    out.push_str("}\n\n");

    out.push_str("fn cmp_values(a: &Value, b: &Value) -> Result<Ordering, String> {\n");
    out.push_str("  match (a, b) {\n");
    out.push_str("    (Value::Int(x), Value::Int(y)) => Ok(x.cmp(y)),\n");
    out.push_str("    (Value::Str(x), Value::Str(y)) => Ok(x.cmp(y)),\n");
    out.push_str("    (Value::Bool(x), Value::Bool(y)) => Ok(x.cmp(y)),\n");
    out.push_str("    _ => Err(\"cannot compare different types\".to_string()),\n");
    out.push_str("  }\n}\n\n");

    out.push_str("fn eq_values(a: &Value, b: &Value) -> bool {\n");
    out.push_str("  match (a, b) {\n");
    out.push_str("    (Value::Int(x), Value::Int(y)) => x == y,\n");
    out.push_str("    (Value::Str(x), Value::Str(y)) => x == y,\n");
    out.push_str("    (Value::Bool(x), Value::Bool(y)) => x == y,\n");
    out.push_str("    (Value::Maybe, Value::Maybe) => true,\n");
    out.push_str("    (Value::Unit, Value::Unit) => true,\n");
    out.push_str("    _ => false,\n");
    out.push_str("  }\n}\n\n");

    out.push_str("fn resolve(globals: &HashMap<String, Value>, locals: &HashMap<String, Value>, name: &str) -> Result<Value, String> {\n");
    out.push_str("  if let Some(v) = locals.get(name) {\n");
    out.push_str("    return match v { Value::Ref(target) => resolve(globals, locals, target), _ => Ok(v.clone()) };\n");
    out.push_str("  }\n");
    out.push_str("  globals.get(name).cloned().ok_or_else(|| format!(\"unknown symbol `{}`\", name))\n");
    out.push_str("}\n\n");

    out.push_str("fn store(local: &mut HashMap<String, Value>, name: &str, value: Value) -> Result<(), String> {\n");
    out.push_str("  match local.get(name) {\n");
    out.push_str("    Some(Value::Ref(target)) => Err(format!(\"cannot assign to ref `{}` -> `{}`\", name, target)),\n");
    out.push_str("    Some(_) => { local.insert(name.to_string(), value); Ok(()) },\n");
    out.push_str("    None => Err(format!(\"unknown local `{}`\", name)),\n");
    out.push_str("  }\n}\n\n");

    out.push_str("fn call(fn_name: &str, funcs: &HashMap<String, Function>, globals: &HashMap<String, Value>, args: Vec<Value>) -> Result<Value, String> {\n");
    out.push_str("  if let Some(v) = call_builtin(fn_name, &args)? { return Ok(v); }\n");
    out.push_str("  let f = funcs.get(fn_name).ok_or_else(|| format!(\"unknown function `{}`\", fn_name))?.clone();\n");
    out.push_str("  if f.params.len() != args.len() { return Err(format!(\"arity mismatch for {}\", fn_name)); }\n");
    out.push_str("  let mut locals: HashMap<String, Value> = HashMap::new();\n");
    out.push_str("  for (p, a) in f.params.iter().zip(args.into_iter()) { locals.insert(p.clone(), a); }\n");
    out.push_str("  let mut stack: Vec<Value> = Vec::new();\n");
    out.push_str("  let mut ip: usize = 0;\n");
    out.push_str("  while ip < f.code.len() {\n");
    out.push_str("    match &f.code[ip] {\n");
    out.push_str("      Instruction::PushInt(v) => stack.push(Value::Int(*v)),\n");
    out.push_str("      Instruction::PushStr(v) => stack.push(Value::Str(v.clone())),\n");
    out.push_str("      Instruction::PushBool(v) => stack.push(Value::Bool(*v)),\n");
    out.push_str("      Instruction::PushMaybe => stack.push(Value::Maybe),\n");
    out.push_str("      Instruction::PushUnit => stack.push(Value::Unit),\n");
    out.push_str("      Instruction::LoadVar(n) => stack.push(resolve(globals, &locals, n)?),\n");
    out.push_str("      Instruction::DefineVar(n) => { let v = pop(&mut stack)?; locals.insert(n.clone(), v); },\n");
    out.push_str("      Instruction::StoreVar(n) => { let v = pop(&mut stack)?; store(&mut locals, n, v)?; },\n");
    out.push_str("      Instruction::StoreOrDefine(n) => { let v = pop(&mut stack)?; if locals.contains_key(n) { store(&mut locals, n, v)?; } else { locals.insert(n.clone(), v); } },\n");
    out.push_str("      Instruction::DeclareRef { name, target } => { if !locals.contains_key(target) && !globals.contains_key(target) { return Err(format!(\"unknown symbol `{}`\", target)); } locals.insert(name.clone(), Value::Ref(target.clone())); },\n");
    out.push_str("      Instruction::Add => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; match (l, r) { (Value::Int(a), Value::Int(b)) => stack.push(Value::Int(a + b)), (Value::Str(a), Value::Str(b)) => stack.push(Value::Str(format!(\"{}{}\", a, b))), _ => return Err(\"invalid add\".to_string()) } },\n");
    out.push_str("      Instruction::Sub => { let r = as_int(pop(&mut stack)?)?; let l = as_int(pop(&mut stack)?)?; stack.push(Value::Int(l - r)); },\n");
    out.push_str("      Instruction::Mul => { let r = as_int(pop(&mut stack)?)?; let l = as_int(pop(&mut stack)?)?; stack.push(Value::Int(l * r)); },\n");
    out.push_str("      Instruction::Div => { let r = as_int(pop(&mut stack)?)?; if r == 0 { return Err(\"division by zero\".to_string()); } let l = as_int(pop(&mut stack)?)?; stack.push(Value::Int(l / r)); },\n");
    out.push_str("      Instruction::Mod => { let r = as_int(pop(&mut stack)?)?; if r == 0 { return Err(\"modulo by zero\".to_string()); } let l = as_int(pop(&mut stack)?)?; stack.push(Value::Int(l % r)); },\n");
    out.push_str("      Instruction::Eq => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(Value::Bool(eq_values(&l, &r))); },\n");
    out.push_str("      Instruction::Ne => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(Value::Bool(!eq_values(&l, &r))); },\n");
    out.push_str("      Instruction::Lt => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(Value::Bool(cmp_values(&l, &r)? == Ordering::Less)); },\n");
    out.push_str("      Instruction::Lte => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(Value::Bool(cmp_values(&l, &r)? != Ordering::Greater)); },\n");
    out.push_str("      Instruction::Gt => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(Value::Bool(cmp_values(&l, &r)? == Ordering::Greater)); },\n");
    out.push_str("      Instruction::Gte => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(Value::Bool(cmp_values(&l, &r)? != Ordering::Less)); },\n");
    out.push_str("      Instruction::And => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(from_logic(logic_and(to_logic(&l)?, to_logic(&r)?))); },\n");
    out.push_str("      Instruction::Or => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(from_logic(logic_or(to_logic(&l)?, to_logic(&r)?))); },\n");
    out.push_str("      Instruction::Xor => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; stack.push(from_logic(logic_xor(to_logic(&l)?, to_logic(&r)?))); },\n");
    out.push_str("      Instruction::BitAnd => { let r = as_int(pop(&mut stack)?)?; let l = as_int(pop(&mut stack)?)?; stack.push(Value::Int(l & r)); },\n");
    out.push_str("      Instruction::BitOr => { let r = as_int(pop(&mut stack)?)?; let l = as_int(pop(&mut stack)?)?; stack.push(Value::Int(l | r)); },\n");
    out.push_str("      Instruction::Shl => { let s = as_int(pop(&mut stack)?)?; if !(0..=63).contains(&s) { return Err(\"shift amount out of range\".to_string()); } let v = as_int(pop(&mut stack)?)?; stack.push(Value::Int(v << (s as u32))); },\n");
    out.push_str("      Instruction::Shr => { let s = as_int(pop(&mut stack)?)?; if !(0..=63).contains(&s) { return Err(\"shift amount out of range\".to_string()); } let v = as_int(pop(&mut stack)?)?; stack.push(Value::Int(v >> (s as u32))); },\n");
    out.push_str("      Instruction::Neg => { let v = as_int(pop(&mut stack)?)?; stack.push(Value::Int(-v)); },\n");
    out.push_str("      Instruction::Not => { let v = pop(&mut stack)?; let out = match to_logic(&v)? { Logic3::True => Logic3::False, Logic3::False => Logic3::True, Logic3::Maybe => Logic3::Maybe }; stack.push(from_logic(out)); },\n");
    out.push_str("      Instruction::Cmp3 => { let r = pop(&mut stack)?; let l = pop(&mut stack)?; let o = cmp_values(&l, &r)?; let mapped = match o { Ordering::Less => -1, Ordering::Equal => 0, Ordering::Greater => 1 }; stack.push(Value::Int(mapped)); },\n");
    out.push_str("      Instruction::Jump(t) => { ip = *t; continue; },\n");
    out.push_str("      Instruction::JumpIfFalse(t) => { let c = as_bool(pop(&mut stack)?)?; if !c { ip = *t; continue; } },\n");
    out.push_str("      Instruction::Call(name, argc) => { let mut a = Vec::with_capacity(*argc); for _ in 0..*argc { a.push(pop(&mut stack)?); } a.reverse(); let v = call(name, funcs, globals, a)?; stack.push(v); },\n");
    out.push_str("      Instruction::PrintBegin => println!(\"print:\"),\n");
    out.push_str("      Instruction::PrintField(k) => { let v = pop(&mut stack)?; println!(\"  {}: {}\", k, render(&v)); },\n");
    out.push_str("      Instruction::PrintEnd => {},\n");
    out.push_str("      Instruction::Nop => {},\n");
    out.push_str("      Instruction::Pop => { let _ = pop(&mut stack)?; },\n");
    out.push_str("      Instruction::Return => { return Ok(stack.pop().unwrap_or(Value::Unit)); },\n");
    out.push_str("    }\n");
    out.push_str("    ip += 1;\n");
    out.push_str("  }\n");
    out.push_str("  Ok(Value::Unit)\n}\n\n");

    out.push_str("fn build_program() -> (HashMap<String, Value>, HashMap<String, Function>) {\n");
    out.push_str("  let mut globals: HashMap<String, Value> = HashMap::new();\n");
    for (name, value) in &program.globals {
        out.push_str(&format!("  globals.insert({:?}.to_string(), {});\n", name, value_to_rust(value)));
    }

    out.push_str("  let mut funcs: HashMap<String, Function> = HashMap::new();\n");
    let mut names: Vec<&String> = program.functions.keys().collect();
    names.sort();
    for name in names {
        let f = &program.functions[name];
        out.push_str("  {\n");
        out.push_str("    let mut code: Vec<Instruction> = Vec::new();\n");
        for ins in &f.code {
            out.push_str(&format!("    code.push({});\n", instruction_to_rust(ins)));
        }
        out.push_str("    funcs.insert(\n");
        out.push_str(&format!("      {:?}.to_string(),\n", f.name));
        out.push_str("      Function {\n");
        out.push_str("        params: vec![");
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("{:?}.to_string()", p));
        }
        out.push_str("],\n");
        out.push_str("        code,\n");
        out.push_str("      },\n");
        out.push_str("    );\n");
        out.push_str("  }\n");
    }
    out.push_str("  (globals, funcs)\n}");

    out.push_str("\n#[no_mangle]\n");
    out.push_str("pub extern \"C\" fn h_entry() -> i32 {\n");
    out.push_str("  let (globals, funcs) = build_program();\n");
    out.push_str("  let entry = if funcs.contains_key(\"main\") { \"main\" } else { funcs.keys().next().expect(\"no functions\") };\n");
    out.push_str("  match call(entry, &funcs, &globals, Vec::new()) {\n");
    out.push_str("    Ok(v) => { println!(\"program_return: {}\", render(&v)); 0 },\n");
    out.push_str("    Err(e) => { eprintln!(\"runtime_error: {}\", e); 1 },\n");
    out.push_str("  }\n");
    out.push_str("}\n");

    out
}

fn generate_link_stub_source() -> String {
    let mut out = String::new();
    out.push_str("unsafe extern \"C\" { fn h_entry() -> i32; }\n\n");
    out.push_str("fn main() {\n");
    out.push_str("  let code = unsafe { h_entry() };\n");
    out.push_str("  if code != 0 { std::process::exit(code); }\n");
    out.push_str("}\n");
    out
}

fn value_to_rust(value: &Value) -> String {
    match value {
        Value::Int(v) => format!("Value::Int({})", v),
        Value::Str(v) => format!("Value::Str({:?}.to_string())", v),
        Value::Bool(v) => format!("Value::Bool({})", v),
        Value::Maybe => "Value::Maybe".to_string(),
        Value::Ref(v) => format!("Value::Ref({:?}.to_string())", v),
        Value::Unit => "Value::Unit".to_string(),
    }
}

fn instruction_to_rust(ins: &Instruction) -> String {
    match ins {
        Instruction::PushInt(v) => format!("Instruction::PushInt({})", v),
        Instruction::PushStr(v) => format!("Instruction::PushStr({:?}.to_string())", v),
        Instruction::PushBool(v) => format!("Instruction::PushBool({})", v),
        Instruction::PushMaybe => "Instruction::PushMaybe".to_string(),
        Instruction::PushUnit => "Instruction::PushUnit".to_string(),
        Instruction::LoadVar(v) => format!("Instruction::LoadVar({:?}.to_string())", v),
        Instruction::DefineVar(v) => format!("Instruction::DefineVar({:?}.to_string())", v),
        Instruction::StoreVar(v) => format!("Instruction::StoreVar({:?}.to_string())", v),
        Instruction::StoreOrDefine(v) => {
            format!("Instruction::StoreOrDefine({:?}.to_string())", v)
        }
        Instruction::DeclareRef { name, target } => format!(
            "Instruction::DeclareRef {{ name: {:?}.to_string(), target: {:?}.to_string() }}",
            name, target
        ),
        Instruction::Add => "Instruction::Add".to_string(),
        Instruction::Sub => "Instruction::Sub".to_string(),
        Instruction::Mul => "Instruction::Mul".to_string(),
        Instruction::Div => "Instruction::Div".to_string(),
        Instruction::Mod => "Instruction::Mod".to_string(),
        Instruction::Eq => "Instruction::Eq".to_string(),
        Instruction::Ne => "Instruction::Ne".to_string(),
        Instruction::Lt => "Instruction::Lt".to_string(),
        Instruction::Lte => "Instruction::Lte".to_string(),
        Instruction::Gt => "Instruction::Gt".to_string(),
        Instruction::Gte => "Instruction::Gte".to_string(),
        Instruction::And => "Instruction::And".to_string(),
        Instruction::Or => "Instruction::Or".to_string(),
        Instruction::Xor => "Instruction::Xor".to_string(),
        Instruction::BitAnd => "Instruction::BitAnd".to_string(),
        Instruction::BitOr => "Instruction::BitOr".to_string(),
        Instruction::Shl => "Instruction::Shl".to_string(),
        Instruction::Shr => "Instruction::Shr".to_string(),
        Instruction::Neg => "Instruction::Neg".to_string(),
        Instruction::Not => "Instruction::Not".to_string(),
        Instruction::Cmp3 => "Instruction::Cmp3".to_string(),
        Instruction::Jump(v) => format!("Instruction::Jump({})", v),
        Instruction::JumpIfFalse(v) => format!("Instruction::JumpIfFalse({})", v),
        Instruction::Call(name, argc) => {
            format!("Instruction::Call({:?}.to_string(), {})", name, argc)
        }
        Instruction::PrintBegin => "Instruction::PrintBegin".to_string(),
        Instruction::PrintField(v) => format!("Instruction::PrintField({:?}.to_string())", v),
        Instruction::PrintEnd => "Instruction::PrintEnd".to_string(),
        Instruction::Nop => "Instruction::Nop".to_string(),
        Instruction::Pop => "Instruction::Pop".to_string(),
        Instruction::Return => "Instruction::Return".to_string(),
    }
}





