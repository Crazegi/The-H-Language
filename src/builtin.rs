use crate::evaluator::Value;

pub fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        "abs" => Some(1),
        "sqrt" => Some(1),
        "min" => Some(2),
        "max" => Some(2),
        "pow" => Some(2),
        "clamp" => Some(3),
        "len" => Some(1),
        "upper" => Some(1),
        "lower" => Some(1),
        "contains" => Some(2),
        "phase" => Some(2),
        "collapse" => Some(1),
        "sleep_until" => Some(1),
        _ => None,
    }
}

pub fn call_builtin(name: &str, args: &[Value]) -> Result<Option<Value>, String> {
    fn int_arg(args: &[Value], idx: usize) -> Result<i64, String> {
        match args.get(idx) {
            Some(Value::Int(v)) => Ok(*v),
            _ => Err("Expected integer argument".to_string()),
        }
    }

    fn str_arg<'a>(args: &'a [Value], idx: usize) -> Result<&'a str, String> {
        match args.get(idx) {
            Some(Value::Str(v)) => Ok(v.as_str()),
            _ => Err("Expected string argument".to_string()),
        }
    }

    fn logic_arg(args: &[Value], idx: usize) -> Result<Logic3, String> {
        let value = args
            .get(idx)
            .ok_or_else(|| "Missing argument".to_string())?;
        to_logic(value)
    }

    let out = match name {
        "abs" => Value::Int(int_arg(args, 0)?.abs()),
        "sqrt" => {
            let n = int_arg(args, 0)?;
            if n < 0 {
                return Err("sqrt expects non-negative integer".to_string());
            }
            Value::Int((n as f64).sqrt().floor() as i64)
        }
        "min" => Value::Int(int_arg(args, 0)?.min(int_arg(args, 1)?)),
        "max" => Value::Int(int_arg(args, 0)?.max(int_arg(args, 1)?)),
        "pow" => {
            let base = int_arg(args, 0)?;
            let exp = int_arg(args, 1)?;
            if exp < 0 {
                return Err("pow exponent must be non-negative".to_string());
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
            let a = logic_arg(args, 0)?;
            let b = logic_arg(args, 1)?;
            from_logic(logic_phase(a, b))
        }
        "collapse" => {
            let value = args
                .get(0)
                .ok_or_else(|| "Missing argument".to_string())?;
            Value::Bool(matches!(to_logic(value)?, Logic3::True))
        }
        "sleep_until" => {
            if args.get(0).is_none() {
                return Err("Missing argument".to_string());
            }
            // Runtime-neutral hook: this will become a real interrupt wait in backend-specific runtimes.
            Value::Bool(true)
        }
        _ => return Ok(None),
    };

    Ok(Some(out))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Logic3 {
    False,
    Maybe,
    True,
}

fn to_logic(value: &Value) -> Result<Logic3, String> {
    match value {
        Value::Bool(true) => Ok(Logic3::True),
        Value::Bool(false) => Ok(Logic3::False),
        Value::Int(v) => Ok(if *v == 0 { Logic3::False } else { Logic3::True }),
        Value::Maybe => Ok(Logic3::Maybe),
        _ => Err("Expected logical-compatible value".to_string()),
    }
}

fn from_logic(value: Logic3) -> Value {
    match value {
        Logic3::True => Value::Bool(true),
        Logic3::False => Value::Bool(false),
        Logic3::Maybe => Value::Maybe,
    }
}

fn logic_phase(a: Logic3, b: Logic3) -> Logic3 {
    match (a, b) {
        (Logic3::False, Logic3::False) => Logic3::False,
        (Logic3::True, Logic3::True) => Logic3::True,
        _ => Logic3::Maybe,
    }
}
