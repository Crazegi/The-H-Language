use std::path::{Path, PathBuf};
use std::process::Command;

use crate::bytecode::{BytecodeProgram, Instruction};
use crate::compiler::{compile_program_with_options, CompileOptions};
use crate::evaluator::Value;
use crate::parser::parse_source;
use crate::semantic::analyze;

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
    analyze(&program).map_err(|e| NativeCompileError::new(format!("Semantic error: {}", e)))?;
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

    out.push_str("#[derive(Clone, Debug)]\n");
    out.push_str("enum Value { Int(i64), Str(String), Bool(bool), Maybe, Ref(String), Unit }\n\n");

    out.push_str("#[derive(Clone, Debug)]\n");
    out.push_str("enum Instruction {\n");
    out.push_str("  PushInt(i64), PushStr(String), PushBool(bool), PushMaybe, PushUnit,\n");
    out.push_str("  LoadVar(String), DefineVar(String), StoreVar(String), StoreOrDefine(String),\n");
    out.push_str("  DeclareRef { name: String, target: String },\n");
    out.push_str("  Add, Sub, Mul, Div, Mod, Eq, Ne, Lt, Lte, Gt, Gte, And, Or, Xor, Neg, Not, Cmp3,\n");
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

    out.push_str("fn call_builtin(name: &str, args: &[Value]) -> Result<Option<Value>, String> {\n");
    out.push_str("  let out = match name {\n");
    out.push_str("    \"abs\" => Value::Int(builtin_int_arg(args, 0)?.abs()),\n");
    out.push_str("    \"sqrt\" => { let n = builtin_int_arg(args, 0)?; if n < 0 { return Err(\"sqrt expects non-negative integer\".to_string()); } Value::Int((n as f64).sqrt().floor() as i64) },\n");
    out.push_str("    \"min\" => Value::Int(builtin_int_arg(args, 0)?.min(builtin_int_arg(args, 1)?)),\n");
    out.push_str("    \"max\" => Value::Int(builtin_int_arg(args, 0)?.max(builtin_int_arg(args, 1)?)),\n");
    out.push_str("    \"pow\" => { let base = builtin_int_arg(args, 0)?; let exp = builtin_int_arg(args, 1)?; if exp < 0 { return Err(\"pow exponent must be non-negative\".to_string()); } Value::Int(base.pow(exp as u32)) },\n");
    out.push_str("    \"clamp\" => { let v = builtin_int_arg(args, 0)?; let lo = builtin_int_arg(args, 1)?; let hi = builtin_int_arg(args, 2)?; Value::Int(v.clamp(lo, hi)) },\n");
    out.push_str("    \"len\" => Value::Int(builtin_str_arg(args, 0)?.chars().count() as i64),\n");
    out.push_str("    \"upper\" => Value::Str(builtin_str_arg(args, 0)?.to_uppercase()),\n");
    out.push_str("    \"lower\" => Value::Str(builtin_str_arg(args, 0)?.to_lowercase()),\n");
    out.push_str("    \"contains\" => Value::Bool(builtin_str_arg(args, 0)?.contains(builtin_str_arg(args, 1)?)),\n");
    out.push_str("    \"phase\" => { let a = to_logic(args.get(0).ok_or_else(|| \"missing argument\".to_string())?)?; let b = to_logic(args.get(1).ok_or_else(|| \"missing argument\".to_string())?)?; from_logic(logic_phase(a, b)) },\n");
    out.push_str("    \"collapse\" => { let v = args.get(0).ok_or_else(|| \"missing argument\".to_string())?; Value::Bool(matches!(to_logic(v)?, Logic3::True)) },\n");
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
