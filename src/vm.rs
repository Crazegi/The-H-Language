use std::cmp::Ordering;
use std::collections::HashMap;

use crate::bytecode::{BytecodeProgram, Instruction};
use crate::evaluator::Value;

#[derive(Debug, Clone)]
pub struct VmError {
    pub message: String,
}

impl VmError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for VmError {}

#[derive(Debug, Clone)]
struct Frame {
    locals: HashMap<String, Value>,
    stack: Vec<Value>,
}

pub fn run_bytecode(program: &BytecodeProgram) -> Result<Value, VmError> {
    let entry = if program.functions.contains_key("main") {
        "main"
    } else {
        program
            .functions
            .keys()
            .next()
            .ok_or_else(|| VmError::new("No functions to execute"))?
    };

    call_function(program, entry, Vec::new())
}

fn call_function(program: &BytecodeProgram, name: &str, args: Vec<Value>) -> Result<Value, VmError> {
    if let Some(v) = call_builtin(name, &args)? {
        return Ok(v);
    }

    let function = program
        .functions
        .get(name)
        .ok_or_else(|| VmError::new(format!("Unknown function `{}`", name)))?;

    if function.params.len() != args.len() {
        return Err(VmError::new(format!(
            "Function `{}` expects {} args, got {}",
            name,
            function.params.len(),
            args.len()
        )));
    }

    let mut frame = Frame {
        locals: HashMap::new(),
        stack: Vec::new(),
    };

    for (p, arg) in function.params.iter().zip(args.into_iter()) {
        frame.locals.insert(p.clone(), arg);
    }

    let mut ip = 0usize;
    while ip < function.code.len() {
        match &function.code[ip] {
            Instruction::PushInt(v) => frame.stack.push(Value::Int(*v)),
            Instruction::PushStr(v) => frame.stack.push(Value::Str(v.clone())),
            Instruction::PushBool(v) => frame.stack.push(Value::Bool(*v)),
            Instruction::PushMaybe => frame.stack.push(Value::Maybe),
            Instruction::PushUnit => frame.stack.push(Value::Unit),
            Instruction::LoadVar(name) => {
                let v = resolve_name(program, &frame.locals, name)?;
                frame.stack.push(v);
            }
            Instruction::DefineVar(name) => {
                let v = pop(&mut frame.stack)?;
                frame.locals.insert(name.clone(), v);
            }
            Instruction::StoreVar(name) => {
                let v = pop(&mut frame.stack)?;
                store_var(&mut frame.locals, name, v)?;
            }
            Instruction::StoreOrDefine(name) => {
                let v = pop(&mut frame.stack)?;
                if frame.locals.contains_key(name) {
                    store_var(&mut frame.locals, name, v)?;
                } else {
                    frame.locals.insert(name.clone(), v);
                }
            }
            Instruction::DeclareRef { name, target } => {
                if !frame.locals.contains_key(target) && !program.globals.contains_key(target) {
                    return Err(VmError::new(format!(
                        "Cannot borrow unknown symbol `{}`",
                        target
                    )));
                }
                frame.locals.insert(name.clone(), Value::Ref(target.clone()));
            }
            Instruction::Add => {
                let r = pop(&mut frame.stack)?;
                let l = pop(&mut frame.stack)?;
                frame.stack.push(add_values(l, r)?);
            }
            Instruction::Sub => {
                let r = as_int(pop(&mut frame.stack)?)?;
                let l = as_int(pop(&mut frame.stack)?)?;
                frame.stack.push(Value::Int(l - r));
            }
            Instruction::Mul => {
                let r = as_int(pop(&mut frame.stack)?)?;
                let l = as_int(pop(&mut frame.stack)?)?;
                frame.stack.push(Value::Int(l * r));
            }
            Instruction::Div => {
                let r = as_int(pop(&mut frame.stack)?)?;
                if r == 0 {
                    return Err(VmError::new("Division by zero"));
                }
                let l = as_int(pop(&mut frame.stack)?)?;
                frame.stack.push(Value::Int(l / r));
            }
            Instruction::Mod => {
                let r = as_int(pop(&mut frame.stack)?)?;
                if r == 0 {
                    return Err(VmError::new("Modulo by zero"));
                }
                let l = as_int(pop(&mut frame.stack)?)?;
                frame.stack.push(Value::Int(l % r));
            }
            Instruction::Eq => {
                let r = pop(&mut frame.stack)?;
                let l = pop(&mut frame.stack)?;
                frame.stack.push(Value::Bool(equals(&l, &r)));
            }
            Instruction::Ne => {
                let r = pop(&mut frame.stack)?;
                let l = pop(&mut frame.stack)?;
                frame.stack.push(Value::Bool(!equals(&l, &r)));
            }
            Instruction::Lt => cmp_push(&mut frame.stack, Ordering::Less)?,
            Instruction::Lte => cmp_push_lte(&mut frame.stack)?,
            Instruction::Gt => cmp_push(&mut frame.stack, Ordering::Greater)?,
            Instruction::Gte => cmp_push_gte(&mut frame.stack)?,
            Instruction::And => {
                let r = pop(&mut frame.stack)?;
                let l = pop(&mut frame.stack)?;
                let out = logic_and(to_logic(&l)?, to_logic(&r)?);
                frame.stack.push(from_logic(out));
            }
            Instruction::Or => {
                let r = pop(&mut frame.stack)?;
                let l = pop(&mut frame.stack)?;
                let out = logic_or(to_logic(&l)?, to_logic(&r)?);
                frame.stack.push(from_logic(out));
            }
            Instruction::Xor => {
                let r = pop(&mut frame.stack)?;
                let l = pop(&mut frame.stack)?;
                let out = logic_xor(to_logic(&l)?, to_logic(&r)?);
                frame.stack.push(from_logic(out));
            }
            Instruction::Neg => {
                let v = as_int(pop(&mut frame.stack)?)?;
                frame.stack.push(Value::Int(-v));
            }
            Instruction::Not => {
                let v = pop(&mut frame.stack)?;
                let out = match to_logic(&v)? {
                    Logic3::True => Logic3::False,
                    Logic3::False => Logic3::True,
                    Logic3::Maybe => Logic3::Maybe,
                };
                frame.stack.push(from_logic(out));
            }
            Instruction::Cmp3 => {
                let r = pop(&mut frame.stack)?;
                let l = pop(&mut frame.stack)?;
                let ord = compare_values(&l, &r)?;
                let mapped = match ord {
                    Ordering::Less => -1,
                    Ordering::Equal => 0,
                    Ordering::Greater => 1,
                };
                frame.stack.push(Value::Int(mapped));
            }
            Instruction::Jump(target) => {
                ip = *target;
                continue;
            }
            Instruction::JumpIfFalse(target) => {
                let cond = as_bool(pop(&mut frame.stack)?)?;
                if !cond {
                    ip = *target;
                    continue;
                }
            }
            Instruction::Call(name, argc) => {
                let mut args = Vec::with_capacity(*argc);
                for _ in 0..*argc {
                    args.push(pop(&mut frame.stack)?);
                }
                args.reverse();
                let result = call_function(program, name, args)?;
                frame.stack.push(result);
            }
            Instruction::PrintBegin => {
                println!("print:");
            }
            Instruction::PrintField(key) => {
                let value = pop(&mut frame.stack)?;
                println!("  {}: {}", key, value.render());
            }
            Instruction::PrintEnd => {}
            Instruction::Pop => {
                let _ = pop(&mut frame.stack)?;
            }
            Instruction::Return => {
                let ret = frame.stack.pop().unwrap_or(Value::Unit);
                return Ok(ret);
            }
        }

        ip += 1;
    }

    Ok(Value::Unit)
}

fn call_builtin(name: &str, args: &[Value]) -> Result<Option<Value>, VmError> {
    fn int_arg(args: &[Value], idx: usize) -> Result<i64, VmError> {
        match args.get(idx) {
            Some(Value::Int(v)) => Ok(*v),
            _ => Err(VmError::new("Expected integer argument")),
        }
    }

    fn str_arg<'a>(args: &'a [Value], idx: usize) -> Result<&'a str, VmError> {
        match args.get(idx) {
            Some(Value::Str(v)) => Ok(v.as_str()),
            _ => Err(VmError::new("Expected string argument")),
        }
    }

    let out = match name {
        "abs" => Value::Int(int_arg(args, 0)?.abs()),
        "sqrt" => {
            let n = int_arg(args, 0)?;
            if n < 0 {
                return Err(VmError::new("sqrt expects non-negative integer"));
            }
            Value::Int((n as f64).sqrt().floor() as i64)
        }
        "min" => Value::Int(int_arg(args, 0)?.min(int_arg(args, 1)?)),
        "max" => Value::Int(int_arg(args, 0)?.max(int_arg(args, 1)?)),
        "pow" => {
            let base = int_arg(args, 0)?;
            let exp = int_arg(args, 1)?;
            if exp < 0 {
                return Err(VmError::new("pow exponent must be non-negative"));
            }
            Value::Int(base.pow(exp as u32))
        }
        "clamp" => {
            let v = int_arg(args, 0)?;
            let lo = int_arg(args, 1)?;
            let hi = int_arg(args, 2)?;
            Value::Int(v.clamp(lo, hi))
        }
        "len" => Value::Int(str_arg(args, 0)?.chars().count() as i64),
        "upper" => Value::Str(str_arg(args, 0)?.to_uppercase()),
        "lower" => Value::Str(str_arg(args, 0)?.to_lowercase()),
        "contains" => Value::Bool(str_arg(args, 0)?.contains(str_arg(args, 1)?)),
        "phase" => {
            let a = to_logic(args.get(0).ok_or_else(|| VmError::new("Missing argument"))?)?;
            let b = to_logic(args.get(1).ok_or_else(|| VmError::new("Missing argument"))?)?;
            from_logic(logic_phase(a, b))
        }
        "collapse" => {
            let v = args.get(0).ok_or_else(|| VmError::new("Missing argument"))?;
            Value::Bool(matches!(to_logic(v)?, Logic3::True))
        }
        _ => return Ok(None),
    };
    Ok(Some(out))
}

fn pop(stack: &mut Vec<Value>) -> Result<Value, VmError> {
    stack
        .pop()
        .ok_or_else(|| VmError::new("VM stack underflow"))
}

fn resolve_name(
    program: &BytecodeProgram,
    locals: &HashMap<String, Value>,
    name: &str,
) -> Result<Value, VmError> {
    if let Some(v) = locals.get(name) {
        return match v {
            Value::Ref(target) => resolve_name(program, locals, target),
            _ => Ok(v.clone()),
        };
    }

    if let Some(v) = program.globals.get(name) {
        return Ok(v.clone());
    }

    Err(VmError::new(format!("Unknown symbol `{}`", name)))
}

fn store_var(locals: &mut HashMap<String, Value>, name: &str, value: Value) -> Result<(), VmError> {
    match locals.get(name) {
        Some(Value::Ref(target)) => Err(VmError::new(format!(
            "Cannot assign to ref binding `{}` (points to `{}`)",
            name, target
        ))),
        Some(_) => {
            locals.insert(name.to_string(), value);
            Ok(())
        }
        None => Err(VmError::new(format!("Unknown local symbol `{}`", name))),
    }
}

fn as_int(v: Value) -> Result<i64, VmError> {
    match v {
        Value::Int(n) => Ok(n),
        _ => Err(VmError::new("Expected integer value")),
    }
}

fn as_bool(v: Value) -> Result<bool, VmError> {
    match v {
        Value::Bool(b) => Ok(b),
        Value::Int(n) => Ok(n != 0),
        Value::Maybe => Ok(false),
        _ => Err(VmError::new("Expected boolean-compatible value")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Logic3 {
    False,
    Maybe,
    True,
}

fn to_logic(v: &Value) -> Result<Logic3, VmError> {
    match v {
        Value::Bool(true) => Ok(Logic3::True),
        Value::Bool(false) => Ok(Logic3::False),
        Value::Int(n) => Ok(if *n == 0 { Logic3::False } else { Logic3::True }),
        Value::Maybe => Ok(Logic3::Maybe),
        _ => Err(VmError::new("Expected logical-compatible value")),
    }
}

fn from_logic(v: Logic3) -> Value {
    match v {
        Logic3::True => Value::Bool(true),
        Logic3::False => Value::Bool(false),
        Logic3::Maybe => Value::Maybe,
    }
}

fn logic_and(a: Logic3, b: Logic3) -> Logic3 {
    match (a, b) {
        (Logic3::False, _) | (_, Logic3::False) => Logic3::False,
        (Logic3::True, x) | (x, Logic3::True) => x,
        (Logic3::Maybe, Logic3::Maybe) => Logic3::Maybe,
    }
}

fn logic_or(a: Logic3, b: Logic3) -> Logic3 {
    match (a, b) {
        (Logic3::True, _) | (_, Logic3::True) => Logic3::True,
        (Logic3::False, x) | (x, Logic3::False) => x,
        (Logic3::Maybe, Logic3::Maybe) => Logic3::Maybe,
    }
}

fn logic_xor(a: Logic3, b: Logic3) -> Logic3 {
    match (a, b) {
        (Logic3::Maybe, _) | (_, Logic3::Maybe) => Logic3::Maybe,
        (Logic3::True, Logic3::True) | (Logic3::False, Logic3::False) => Logic3::False,
        _ => Logic3::True,
    }
}

fn logic_phase(a: Logic3, b: Logic3) -> Logic3 {
    if a == b {
        a
    } else {
        Logic3::Maybe
    }
}

fn add_values(left: Value, right: Value) -> Result<Value, VmError> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
        (Value::Str(a), Value::Str(b)) => Ok(Value::Str(format!("{}{}", a, b))),
        _ => Err(VmError::new("Invalid types for `+`")),
    }
}

fn cmp_push(stack: &mut Vec<Value>, expected: Ordering) -> Result<(), VmError> {
    let r = pop(stack)?;
    let l = pop(stack)?;
    let ord = compare_values(&l, &r)?;
    stack.push(Value::Bool(ord == expected));
    Ok(())
}

fn cmp_push_lte(stack: &mut Vec<Value>) -> Result<(), VmError> {
    let r = pop(stack)?;
    let l = pop(stack)?;
    let ord = compare_values(&l, &r)?;
    stack.push(Value::Bool(ord != Ordering::Greater));
    Ok(())
}

fn cmp_push_gte(stack: &mut Vec<Value>) -> Result<(), VmError> {
    let r = pop(stack)?;
    let l = pop(stack)?;
    let ord = compare_values(&l, &r)?;
    stack.push(Value::Bool(ord != Ordering::Less));
    Ok(())
}

fn compare_values(left: &Value, right: &Value) -> Result<Ordering, VmError> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b)),
        (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Ok(a.cmp(b)),
        _ => Err(VmError::new("Cannot compare values of different types")),
    }
}

fn equals(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Str(a), Value::Str(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Maybe, Value::Maybe) => true,
        (Value::Unit, Value::Unit) => true,
        _ => false,
    }
}
