use crate::evaluator::Value;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn builtin_arity(name: &str) -> Option<usize> {
    match name {
        "abs" => Some(1),
        "sqrt" => Some(1),
        "floor" => Some(1),
        "ceil" => Some(1),
        "log2" => Some(1),
        "sin" => Some(1),
        "cos" => Some(1),
        "tan" => Some(1),
        "min" => Some(2),
        "max" => Some(2),
        "pow" => Some(2),
        "clamp" => Some(3),
        "len" => Some(1),
        "upper" => Some(1),
        "lower" => Some(1),
        "contains" => Some(2),
        "split" => Some(2),
        "join" => Some(2),
        "phase" => Some(2),
        "collapse" => Some(1),
        "sleep_until" => Some(1),
        "sleep_ms" => Some(1),
        "now_ms" => Some(0),
        "rand_int" => Some(2),
        "input" => Some(1),
        "read_text" => Some(1),
        "write_text" => Some(2),
        "append_text" => Some(2),
        "exists" => Some(1),
        "delete_file" => Some(1),
        "env" => Some(1),
        "to_int" => Some(1),
        "to_bool" => Some(1),
        "to_float" => Some(1),
        "to_string" => Some(1),
        "to_float_string" => Some(1),
        "trim" => Some(1),
        "replace" => Some(3),
        "array_new" => Some(0),
        "array_len" => Some(1),
        "array_push" => Some(2),
        "array_get" => Some(2),
        "queue_new" => Some(0),
        "queue_len" => Some(1),
        "queue_push" => Some(2),
        "queue_peek" => Some(1),
        "queue_pop" => Some(1),
        "ring_new" => Some(1),
        "ring_len" => Some(1),
        "ring_push" => Some(2),
        "ring_peek" => Some(1),
        "gpio_claim" => Some(1),
        "gpio_mode" => Some(2),
        "gpio_write" => Some(2),
        "gpio_read" => Some(1),
        "uart_new" => Some(2),
        "uart_write" => Some(2),
        "uart_read" => Some(1),
        "spi_new" => Some(3),
        "spi_transfer" => Some(2),
        "i2c_new" => Some(2),
        "i2c_write" => Some(3),
        "i2c_read" => Some(3),
        "timer_new" => Some(2),
        "timer_start" => Some(2),
        "timer_elapsed" => Some(1),
        "watchdog_new" => Some(2),
        "watchdog_feed" => Some(1),
        "dma_new" => Some(1),
        "dma_transfer" => Some(4),
        "window_loop" => Some(2),
        "menu" => Some(2),
        "http_get" => Some(1),
        "json_parse" => Some(2),
        "script_args_count" => Some(0),
        "script_arg" => Some(1),
        "script_cwd" => Some(0),
        "script_chdir" => Some(1),
        "script_path_join" => Some(2),
        "script_dirname" => Some(1),
        "script_basename" => Some(1),
        "script_run" => Some(1),
        "script_run_capture" => Some(1),
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
        "floor" => Value::Int(int_arg(args, 0)?),
        "ceil" => Value::Int(int_arg(args, 0)?),
        "log2" => {
            let n = int_arg(args, 0)?;
            if n <= 0 {
                return Err("log2 expects positive integer".to_string());
            }
            Value::Int((i64::BITS - 1 - n.leading_zeros()) as i64)
        }
        "sin" => {
            let deg = int_arg(args, 0)?;
            Value::Int((deg_to_rad(deg).sin() * 1000.0).round() as i64)
        }
        "cos" => {
            let deg = int_arg(args, 0)?;
            Value::Int((deg_to_rad(deg).cos() * 1000.0).round() as i64)
        }
        "tan" => {
            let deg = int_arg(args, 0)?;
            let cos = deg_to_rad(deg).cos();
            if cos.abs() < 1e-9 {
                return Err("tan is undefined for this angle".to_string());
            }
            Value::Int((deg_to_rad(deg).tan() * 1000.0).round() as i64)
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
        "split" => {
            let source = str_arg(args, 0)?;
            let delimiter = str_arg(args, 1)?;
            if delimiter.is_empty() {
                return Err("split expects non-empty delimiter".to_string());
            }
            Value::Str(
                source
                    .split(delimiter)
                    .collect::<Vec<_>>()
                    .join(SEQ_SEPARATOR_STR),
            )
        }
        "join" => {
            let sequence = str_arg(args, 0)?;
            let delimiter = str_arg(args, 1)?;
            Value::Str(sequence_items(sequence).join(delimiter))
        }
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
        "sleep_ms" => {
            let ms = int_arg(args, 0)?;
            if ms < 0 {
                return Err("sleep_ms expects non-negative milliseconds".to_string());
            }
            thread::sleep(Duration::from_millis(ms as u64));
            Value::Unit
        }
        "now_ms" => {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "System time is before UNIX_EPOCH".to_string())?;
            Value::Int(now.as_millis() as i64)
        }
        "rand_int" => {
            let lo = int_arg(args, 0)?;
            let hi = int_arg(args, 1)?;
            if lo > hi {
                return Err("rand_int expects lo <= hi".to_string());
            }

            let span = (hi - lo + 1) as u64;
            let seed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "System time is before UNIX_EPOCH".to_string())?
                .as_nanos() as u64;
            let mixed = seed ^ (seed.rotate_left(13)).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            Value::Int(lo + (mixed % span) as i64)
        }
        "input" => {
            let prompt = str_arg(args, 0)?;
            print!("{}", prompt);
            io::stdout()
                .flush()
                .map_err(|e| format!("input flush failed: {}", e))?;

            let mut line = String::new();
            io::stdin()
                .read_line(&mut line)
                .map_err(|e| format!("input read failed: {}", e))?;

            while line.ends_with('\n') || line.ends_with('\r') {
                line.pop();
            }
            Value::Str(line)
        }
        "read_text" => {
            let path = str_arg(args, 0)?;
            let content = fs::read_to_string(path)
                .map_err(|e| format!("read_text failed for `{}`: {}", path, e))?;
            Value::Str(content)
        }
        "write_text" => {
            let path = str_arg(args, 0)?;
            let content = str_arg(args, 1)?;
            fs::write(path, content)
                .map_err(|e| format!("write_text failed for `{}`: {}", path, e))?;
            Value::Bool(true)
        }
        "append_text" => {
            let path = str_arg(args, 0)?;
            let content = str_arg(args, 1)?;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| format!("append_text open failed for `{}`: {}", path, e))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("append_text write failed for `{}`: {}", path, e))?;
            Value::Bool(true)
        }
        "exists" => {
            let path = str_arg(args, 0)?;
            Value::Bool(std::path::Path::new(path).exists())
        }
        "delete_file" => {
            let path = str_arg(args, 0)?;
            match fs::remove_file(path) {
                Ok(_) => Value::Bool(true),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Value::Bool(false),
                Err(e) => {
                    return Err(format!("delete_file failed for `{}`: {}", path, e));
                }
            }
        }
        "env" => {
            let key = str_arg(args, 0)?;
            Value::Str(std::env::var(key).unwrap_or_default())
        }
        "to_int" => {
            let raw = str_arg(args, 0)?;
            let parsed = raw
                .trim()
                .parse::<i64>()
                .map_err(|e| format!("to_int parse failed: {}", e))?;
            Value::Int(parsed)
        }
        "to_bool" => {
            let raw = str_arg(args, 0)?.trim().to_ascii_lowercase();
            let parsed = match raw.as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => return Err("to_bool parse failed: expected true/false-like string".to_string()),
            };
            Value::Bool(parsed)
        }
        "to_float" => {
            let raw = str_arg(args, 0)?;
            let scaled = parse_scaled_thousand(raw)?;
            Value::Int(scaled)
        }
        "to_string" => {
            let value = args
                .get(0)
                .ok_or_else(|| "Missing argument".to_string())?;
            Value::Str(value_to_string(value))
        }
        "to_float_string" => {
            let scaled = int_arg(args, 0)?;
            Value::Str(format_scaled_thousand(scaled))
        }
        "trim" => Value::Str(str_arg(args, 0)?.trim().to_string()),
        "replace" => {
            let source = str_arg(args, 0)?;
            let from = str_arg(args, 1)?;
            let to = str_arg(args, 2)?;
            Value::Str(source.replace(from, to))
        }
        "array_new" => Value::Str(String::new()),
        "array_len" => Value::Int(sequence_items(str_arg(args, 0)?).len() as i64),
        "array_push" => {
            let sequence = str_arg(args, 0)?;
            let item = str_arg(args, 1)?;
            Value::Str(push_sequence_item(sequence, item))
        }
        "array_get" => {
            let sequence = str_arg(args, 0)?;
            let idx = int_arg(args, 1)?;
            if idx < 0 {
                return Err("array_get expects non-negative index".to_string());
            }
            match sequence_items(sequence).get(idx as usize) {
                Some(v) => Value::Str(v.clone()),
                None => Value::Maybe,
            }
        }
        "queue_new" => Value::Str(String::new()),
        "queue_len" => Value::Int(sequence_items(str_arg(args, 0)?).len() as i64),
        "queue_push" => {
            let sequence = str_arg(args, 0)?;
            let item = str_arg(args, 1)?;
            Value::Str(push_sequence_item(sequence, item))
        }
        "queue_peek" => {
            let sequence = str_arg(args, 0)?;
            match sequence_items(sequence).first() {
                Some(v) => Value::Str(v.clone()),
                None => Value::Maybe,
            }
        }
        "queue_pop" => {
            let sequence = str_arg(args, 0)?;
            let mut items = sequence_items(sequence);
            if !items.is_empty() {
                items.remove(0);
            }
            Value::Str(items.join(SEQ_SEPARATOR_STR))
        }
        "ring_new" => {
            let cap = int_arg(args, 0)?;
            if cap <= 0 {
                return Err("ring_new expects capacity > 0".to_string());
            }
            Value::Str(format!("{}{}", cap, RING_CAP_SEPARATOR))
        }
        "ring_len" => {
            let ring = str_arg(args, 0)?;
            let (_, items) = parse_ring(ring)?;
            Value::Int(items.len() as i64)
        }
        "ring_push" => {
            let ring = str_arg(args, 0)?;
            let item = str_arg(args, 1)?;
            let (capacity, mut items) = parse_ring(ring)?;
            items.push(item.to_string());
            while items.len() > capacity as usize {
                items.remove(0);
            }
            Value::Str(format_ring(capacity, &items))
        }
        "ring_peek" => {
            let ring = str_arg(args, 0)?;
            let (_, items) = parse_ring(ring)?;
            match items.first() {
                Some(v) => Value::Str(v.clone()),
                None => Value::Maybe,
            }
        }
        "gpio_claim" => {
            let port = str_arg(args, 0)?;
            if !is_memory_target_syntax(port) {
                return Err("gpio_claim expects memory-target style port like `[port_a]`".to_string());
            }
            Value::Str(format!("gpio:{}:owned", port))
        }
        "gpio_mode" => {
            let handle = str_arg(args, 0)?;
            let mode = str_arg(args, 1)?;
            let allowed = ["in", "out", "pullup", "pulldown"];
            if !allowed.contains(&mode) {
                return Err("gpio_mode expects one of: in, out, pullup, pulldown".to_string());
            }
            ensure_gpio_handle(handle)?;
            Value::Str(format!("{}:mode={}", handle, mode))
        }
        "gpio_write" => {
            let handle = str_arg(args, 0)?;
            let value = int_arg(args, 1)?;
            if value != 0 && value != 1 {
                return Err("gpio_write expects value 0 or 1".to_string());
            }
            ensure_gpio_handle(handle)?;
            Value::Bool(true)
        }
        "gpio_read" => {
            let handle = str_arg(args, 0)?;
            ensure_gpio_handle(handle)?;
            Value::Int((stable_hash(handle) & 1) as i64)
        }
        "uart_new" => {
            let bus = str_arg(args, 0)?;
            let baud = int_arg(args, 1)?;
            if baud <= 0 {
                return Err("uart_new expects baud > 0".to_string());
            }
            Value::Str(format!("uart:{}:baud={}", bus, baud))
        }
        "uart_write" => {
            let uart = str_arg(args, 0)?;
            let payload = str_arg(args, 1)?;
            ensure_handle_prefix(uart, "uart:", "uart_write")?;
            Value::Int(payload.len() as i64)
        }
        "uart_read" => {
            let uart = str_arg(args, 0)?;
            ensure_handle_prefix(uart, "uart:", "uart_read")?;
            Value::Str("uart_rx_stub".to_string())
        }
        "spi_new" => {
            let bus = str_arg(args, 0)?;
            let hz = int_arg(args, 1)?;
            let mode = int_arg(args, 2)?;
            if hz <= 0 {
                return Err("spi_new expects hz > 0".to_string());
            }
            if !(0..=3).contains(&mode) {
                return Err("spi_new expects mode in range 0..3".to_string());
            }
            Value::Str(format!("spi:{}:hz={}:mode={}", bus, hz, mode))
        }
        "spi_transfer" => {
            let spi = str_arg(args, 0)?;
            let payload = str_arg(args, 1)?;
            ensure_handle_prefix(spi, "spi:", "spi_transfer")?;
            Value::Str(payload.to_string())
        }
        "i2c_new" => {
            let bus = str_arg(args, 0)?;
            let hz = int_arg(args, 1)?;
            if hz <= 0 {
                return Err("i2c_new expects hz > 0".to_string());
            }
            Value::Str(format!("i2c:{}:hz={}", bus, hz))
        }
        "i2c_write" => {
            let i2c = str_arg(args, 0)?;
            let address = int_arg(args, 1)?;
            let _payload = str_arg(args, 2)?;
            ensure_handle_prefix(i2c, "i2c:", "i2c_write")?;
            if !(0..=0x7F).contains(&address) {
                return Err("i2c_write expects 7-bit address in range 0..127".to_string());
            }
            Value::Bool(true)
        }
        "i2c_read" => {
            let i2c = str_arg(args, 0)?;
            let address = int_arg(args, 1)?;
            let count = int_arg(args, 2)?;
            ensure_handle_prefix(i2c, "i2c:", "i2c_read")?;
            if !(0..=0x7F).contains(&address) {
                return Err("i2c_read expects 7-bit address in range 0..127".to_string());
            }
            if count < 0 {
                return Err("i2c_read expects non-negative byte count".to_string());
            }
            let byte = format!("{:02X}", address);
            let mut out = Vec::with_capacity(count as usize);
            for _ in 0..count {
                out.push(byte.clone());
            }
            Value::Str(out.join(" "))
        }
        "timer_new" => {
            let name = str_arg(args, 0)?;
            let hz = int_arg(args, 1)?;
            if hz <= 0 {
                return Err("timer_new expects hz > 0".to_string());
            }
            Value::Str(format!("timer:{}:hz={}", name, hz))
        }
        "timer_start" => {
            let timer = str_arg(args, 0)?;
            let cycles = int_arg(args, 1)?;
            ensure_handle_prefix(timer, "timer:", "timer_start")?;
            if cycles <= 0 {
                return Err("timer_start expects cycles > 0".to_string());
            }
            Value::Str(format!("{}:cycles={}", timer, cycles))
        }
        "timer_elapsed" => {
            let timer = str_arg(args, 0)?;
            ensure_handle_prefix(timer, "timer:", "timer_elapsed")?;
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| "System time is before UNIX_EPOCH".to_string())?;
            Value::Int((now.as_micros() % 1_000_000) as i64)
        }
        "watchdog_new" => {
            let name = str_arg(args, 0)?;
            let timeout_ms = int_arg(args, 1)?;
            if timeout_ms <= 0 {
                return Err("watchdog_new expects timeout_ms > 0".to_string());
            }
            Value::Str(format!("watchdog:{}:ms={}", name, timeout_ms))
        }
        "watchdog_feed" => {
            let watchdog = str_arg(args, 0)?;
            ensure_handle_prefix(watchdog, "watchdog:", "watchdog_feed")?;
            Value::Bool(true)
        }
        "dma_new" => {
            let channel = str_arg(args, 0)?;
            Value::Str(format!("dma:{}", channel))
        }
        "dma_transfer" => {
            let dma = str_arg(args, 0)?;
            let src = str_arg(args, 1)?;
            let dst = str_arg(args, 2)?;
            let bytes = int_arg(args, 3)?;
            ensure_handle_prefix(dma, "dma:", "dma_transfer")?;
            if bytes < 0 {
                return Err("dma_transfer expects non-negative byte count".to_string());
            }
            let _ = (src, dst);
            Value::Bool(true)
        }
        "window_loop" => {
            let title = str_arg(args, 0)?;
            let ticks = int_arg(args, 1)?;
            if ticks < 0 {
                return Err("window_loop expects non-negative tick count".to_string());
            }
            Value::Str(format!("window:{}:ticks={}", title, ticks))
        }
        "menu" => {
            let _title = str_arg(args, 0)?;
            let options = str_arg(args, 1)?;
            let first = options
                .split('|')
                .next()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if first.is_empty() {
                Value::Maybe
            } else {
                Value::Str(first)
            }
        }
        "http_get" => {
            let url = str_arg(args, 0)?;
            let escaped_url = escape_json_string(url);
            Value::Str(format!(
                "{{\"status\":200,\"url\":\"{}\",\"body\":\"stub\"}}",
                escaped_url
            ))
        }
        "json_parse" => {
            let json = str_arg(args, 0)?;
            let key = str_arg(args, 1)?;
            parse_json_field(json, key).unwrap_or(Value::Maybe)
        }
        "script_args_count" => Value::Int(std::env::args().count() as i64),
        "script_arg" => {
            let idx = int_arg(args, 0)?;
            if idx < 0 {
                return Err("script_arg expects non-negative index".to_string());
            }
            Value::Str(
                std::env::args()
                    .nth(idx as usize)
                    .unwrap_or_default(),
            )
        }
        "script_cwd" => {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("script_cwd failed: {}", e))?;
            Value::Str(normalize_path(cwd))
        }
        "script_chdir" => {
            let path = str_arg(args, 0)?;
            std::env::set_current_dir(path)
                .map_err(|e| format!("script_chdir failed for `{}`: {}", path, e))?;
            Value::Bool(true)
        }
        "script_path_join" => {
            let base = str_arg(args, 0)?;
            let child = str_arg(args, 1)?;
            let joined = Path::new(base).join(child);
            Value::Str(normalize_path(joined))
        }
        "script_dirname" => {
            let path = str_arg(args, 0)?;
            let parent = Path::new(path)
                .parent()
                .map(normalize_path)
                .unwrap_or_default();
            Value::Str(parent)
        }
        "script_basename" => {
            let path = str_arg(args, 0)?;
            let name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            Value::Str(name)
        }
        "script_run" => {
            let command = str_arg(args, 0)?;
            let status = shell_command(command)
                .status()
                .map_err(|e| format!("script_run failed for `{}`: {}", command, e))?;
            Value::Int(status.code().unwrap_or(-1) as i64)
        }
        "script_run_capture" => {
            let command = str_arg(args, 0)?;
            let output = shell_command(command)
                .output()
                .map_err(|e| format!("script_run_capture failed for `{}`: {}", command, e))?;

            let mut out = String::new();
            out.push_str(&String::from_utf8_lossy(&output.stdout));
            out.push_str(&String::from_utf8_lossy(&output.stderr));

            while out.ends_with('\n') || out.ends_with('\r') {
                out.pop();
            }
            Value::Str(out)
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

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Int(v) => v.to_string(),
        Value::Str(v) => v.clone(),
        Value::Bool(v) => v.to_string(),
        Value::Maybe => "maybe".to_string(),
        Value::Ref(v) => format!("&{}", v),
        Value::Unit => "unit".to_string(),
    }
}

fn normalize_path(path: impl Into<PathBuf>) -> String {
    path.into().to_string_lossy().replace('\\', "/")
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

const SEQ_SEPARATOR: char = '\u{001f}';
const SEQ_SEPARATOR_STR: &str = "\u{001f}";
const RING_CAP_SEPARATOR: char = '#';

fn sequence_items(sequence: &str) -> Vec<String> {
    if sequence.is_empty() {
        Vec::new()
    } else {
        sequence
            .split(SEQ_SEPARATOR)
            .map(|s| s.to_string())
            .collect()
    }
}

fn push_sequence_item(sequence: &str, item: &str) -> String {
    if sequence.is_empty() {
        item.to_string()
    } else {
        format!("{}{}{}", sequence, SEQ_SEPARATOR, item)
    }
}

fn deg_to_rad(degrees: i64) -> f64 {
    (degrees as f64).to_radians()
}

fn parse_scaled_thousand(raw: &str) -> Result<i64, String> {
    let parsed = raw
        .trim()
        .parse::<f64>()
        .map_err(|e| format!("to_float parse failed: {}", e))?;
    let scaled = parsed * 1000.0;
    if !scaled.is_finite() {
        return Err("to_float parse failed: value is not finite".to_string());
    }
    if scaled < i64::MIN as f64 || scaled > i64::MAX as f64 {
        return Err("to_float parse failed: value out of range".to_string());
    }
    Ok(scaled.round() as i64)
}

fn format_scaled_thousand(value: i64) -> String {
    let negative = value < 0;
    let abs = value.unsigned_abs();
    let whole = abs / 1000;
    let frac = abs % 1000;
    if negative {
        format!("-{}.{:03}", whole, frac)
    } else {
        format!("{}.{:03}", whole, frac)
    }
}

fn parse_ring(raw: &str) -> Result<(i64, Vec<String>), String> {
    let mut parts = raw.splitn(2, RING_CAP_SEPARATOR);
    let cap_part = parts.next().unwrap_or_default().trim();
    let payload = parts.next().unwrap_or_default();

    if cap_part.is_empty() {
        return Err("ring value is invalid: missing capacity".to_string());
    }

    let capacity = cap_part
        .parse::<i64>()
        .map_err(|e| format!("ring value is invalid: {}", e))?;
    if capacity <= 0 {
        return Err("ring value is invalid: capacity must be > 0".to_string());
    }

    Ok((capacity, sequence_items(payload)))
}

fn format_ring(capacity: i64, items: &[String]) -> String {
    format!("{}{}{}", capacity, RING_CAP_SEPARATOR, items.join(SEQ_SEPARATOR_STR))
}

fn is_memory_target_syntax(value: &str) -> bool {
    value.starts_with('[') && value.ends_with(']') && value.len() > 2
}

fn ensure_handle_prefix(handle: &str, prefix: &str, builtin_name: &str) -> Result<(), String> {
    if handle.starts_with(prefix) {
        Ok(())
    } else {
        Err(format!(
            "{} expects handle produced by matching *_new builtin",
            builtin_name
        ))
    }
}

fn ensure_gpio_handle(handle: &str) -> Result<(), String> {
    if handle.starts_with("gpio:[") && handle.contains("]:owned") {
        Ok(())
    } else {
        Err("gpio builtin expects handle from gpio_claim".to_string())
    }
}

fn stable_hash(input: &str) -> u64 {
    let mut hash = 1469598103934665603u64;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1099511628211u64);
    }
    hash
}

fn escape_json_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn parse_json_field(json: &str, key: &str) -> Option<Value> {
    let key_pattern = format!("\"{}\"", key);
    let key_pos = json.find(&key_pattern)?;
    let after_key = &json[key_pos + key_pattern.len()..];
    let colon_offset = after_key.find(':')?;
    let mut value_part = after_key[colon_offset + 1..].trim_start();

    if value_part.starts_with('"') {
        value_part = &value_part[1..];
        let mut escaped = false;
        let mut out = String::new();
        for ch in value_part.chars() {
            if escaped {
                out.push(ch);
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                return Some(Value::Str(out));
            }
            out.push(ch);
        }
        return None;
    }

    if value_part.starts_with("true") {
        return Some(Value::Bool(true));
    }
    if value_part.starts_with("false") {
        return Some(Value::Bool(false));
    }
    if value_part.starts_with("null") {
        return Some(Value::Maybe);
    }

    let end = value_part
        .find(|c: char| c == ',' || c == '}' || c.is_whitespace())
        .unwrap_or(value_part.len());
    let number = &value_part[..end];
    if let Ok(v) = number.parse::<i64>() {
        return Some(Value::Int(v));
    }

    None
}
