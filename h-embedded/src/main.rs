mod board;
mod embedded_lint;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use board::Board;
use clap::Parser;
use embedded_lint::validate_embedded_profile;
use hl_lexer::ast::{BinaryOp, Expr, Function, Program, Stmt, UnaryOp};
use hl_lexer::{analyze_with_warnings, parse_source_from_path};

#[derive(Copy, Clone, Debug, Eq, PartialEq, clap::ValueEnum)]
enum Esp32C3Profile {
    DevkitM1,
    SuperMini,
}

impl Esp32C3Profile {
    fn slug(self) -> &'static str {
        match self {
            Esp32C3Profile::DevkitM1 => "devkitm-1",
            Esp32C3Profile::SuperMini => "super-mini",
        }
    }

    fn map_pin(self, pin: i64) -> Result<i64, String> {
        match self {
            Esp32C3Profile::DevkitM1 => {
                if (0..=21).contains(&pin) {
                    Ok(pin)
                } else {
                    Err(format!(
                        "pin {} is out of supported range for ESP32-C3 DevKitM-1 (0..21)",
                        pin
                    ))
                }
            }
            Esp32C3Profile::SuperMini => {
                let allowed = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 20, 21];
                if allowed.contains(&pin) {
                    Ok(pin)
                } else {
                    Err(format!(
                        "pin {} not available on ESP32-C3 Super Mini profile",
                        pin
                    ))
                }
            }
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "h-embedded")]
#[command(about = "Embedded-focused H frontend for Wokwi board targets")]
struct Cli {
    #[arg(value_name = "INPUT")]
    input: PathBuf,

    #[arg(long, value_enum, default_value = "esp32-c3")]
    board: Board,

    #[arg(long)]
    out_dir: Option<PathBuf>,

    #[arg(long, action = clap::ArgAction::SetTrue)]
    build: bool,

    #[arg(long, action = clap::ArgAction::SetTrue)]
    flash: bool,

    #[arg(long, action = clap::ArgAction::SetTrue)]
    monitor: bool,

    #[arg(long)]
    port: Option<String>,

    #[arg(long, value_enum, default_value = "devkit-m1")]
    esp32c3_profile: Esp32C3Profile,
}

fn main() {
    let cli = Cli::parse();

    let source_path = match fs::canonicalize(&cli.input) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("Failed to resolve input file {}: {}", cli.input.display(), err);
            std::process::exit(1);
        }
    };

    let program = match parse_source_from_path(&source_path) {
        Ok(program) => program,
        Err(err) => {
            eprintln!("Parse error: {}", err);
            std::process::exit(1);
        }
    };

    match analyze_with_warnings(&program) {
        Ok(warnings) => {
            for warning in warnings {
                eprintln!("{}", warning);
            }
        }
        Err(err) => {
            eprintln!("Semantic error: {}", err);
            std::process::exit(1);
        }
    }

    let lint_issues = validate_embedded_profile(&program);
    if !lint_issues.is_empty() {
        eprintln!("Embedded profile check failed:");
        for issue in lint_issues {
            eprintln!("  - {}:{} {}", issue.line, issue.column, issue.message);
        }
        std::process::exit(1);
    }

    let repo_root = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("Failed to determine current directory: {}", err);
            std::process::exit(1);
        }
    };

    let out_dir = cli
        .out_dir
        .clone()
        .unwrap_or_else(|| cli.board.default_out_dir(&repo_root));

    if let Err(err) = emit_board_output(&program, &out_dir, cli.board, &source_path, &cli) {
        eprintln!("Emit error: {}", err);
        std::process::exit(1);
    }

    println!("embedded_profile: ok");
    println!("board: {}", cli.board.slug());
    println!("output_dir: {}", out_dir.display());
}

fn emit_board_output(
    program: &Program,
    out_dir: &Path,
    board: Board,
    source_path: &Path,
    cli: &Cli,
) -> Result<(), String> {
    match board {
        Board::Esp32C3 => emit_esp32c3_idf_project(program, out_dir, source_path, cli),
        Board::PiPico | Board::ArduinoUno => emit_placeholder_for_unsupported_board(out_dir, board, source_path),
    }
}

fn emit_placeholder_for_unsupported_board(
    out_dir: &Path,
    board: Board,
    source_path: &Path,
) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|e| format!("failed to create output dir: {}", e))?;
    for file in board.firmware_files() {
        let target = out_dir.join(file);
        let content = format!(
            "placeholder artifact\nboard={}\nsource={}\nstatus=real emitter not implemented for this board yet\n",
            board.slug(),
            source_path.display()
        );
        fs::write(&target, content)
            .map_err(|e| format!("failed to write {}: {}", target.display(), e))?;
    }
    Ok(())
}

fn emit_esp32c3_idf_project(
    program: &Program,
    out_dir: &Path,
    source_path: &Path,
    cli: &Cli,
) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|e| format!("failed to create output dir: {}", e))?;

    let generated = lower_program_to_esp32c3_c(program, cli.esp32c3_profile)?;
    let project_dir = out_dir.join("esp32c3-idf");
    let main_dir = project_dir.join("main");
    fs::create_dir_all(&main_dir)
        .map_err(|e| format!("failed to create {}: {}", main_dir.display(), e))?;

    fs::write(
        project_dir.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.16)\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\nproject(h_embedded_fw)\n",
    )
    .map_err(|e| format!("failed to write root CMakeLists.txt: {}", e))?;

    fs::write(
        main_dir.join("CMakeLists.txt"),
        "idf_component_register(SRCS \"main.c\" INCLUDE_DIRS \".\")\n",
    )
    .map_err(|e| format!("failed to write component CMakeLists.txt: {}", e))?;

    fs::write(main_dir.join("main.c"), generated.c_source)
        .map_err(|e| format!("failed to write generated C source: {}", e))?;

    fs::write(
        project_dir.join("sdkconfig.defaults"),
        "CONFIG_IDF_TARGET=\"esp32c3\"\n",
    )
    .map_err(|e| format!("failed to write sdkconfig.defaults: {}", e))?;

    let build_requested = cli.build || cli.flash || cli.monitor;
    if build_requested {
        run_idf_py(&project_dir, &["-B", "build", "-DIDF_TARGET=esp32c3", "build"])?;

        let built_elf = project_dir.join("build").join("h_embedded_fw.elf");
        if !built_elf.exists() {
            return Err(format!(
                "expected ELF not found at {} after build",
                built_elf.display()
            ));
        }

        fs::copy(&built_elf, out_dir.join("firmware.elf"))
            .map_err(|e| format!("failed to copy firmware ELF: {}", e))?;
    } else {
        let placeholder = out_dir.join("firmware.elf");
        fs::write(
            &placeholder,
            "Build not requested. Run with --build to compile real ESP32-C3 firmware.\n",
        )
        .map_err(|e| format!("failed to write {}: {}", placeholder.display(), e))?;
    }

    if cli.flash {
        let mut flash_args = vec!["-B", "build", "-DIDF_TARGET=esp32c3"];
        if let Some(port) = cli.port.as_deref() {
            flash_args.push("-p");
            flash_args.push(port);
        }
        flash_args.push("flash");
        run_idf_py(&project_dir, &flash_args)?;
    }

    if cli.monitor {
        let mut monitor_args = vec!["-B", "build", "-DIDF_TARGET=esp32c3"];
        if let Some(port) = cli.port.as_deref() {
            monitor_args.push("-p");
            monitor_args.push(port);
        }
        monitor_args.push("monitor");
        run_idf_py(&project_dir, &monitor_args)?;
    }

    let report_path = out_dir.join("h_embedded_report.txt");
    let report = format!(
        "H Embedded Build Report\nboard: esp32-c3\nprofile: {}\nsource: {}\nproject_dir: {}\ngpio_pins: {}\nuart_ports: {}\nstatus: generated ESP-IDF project{}\n",
        cli.esp32c3_profile.slug(),
        source_path.display(),
        project_dir.display(),
        generated
            .gpio_pins
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        generated
            .uart_ports
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        if build_requested {
            ", built"
        } else {
            ""
        }
    );
    fs::write(&report_path, report)
        .map_err(|e| format!("failed to write {}: {}", report_path.display(), e))?;

    Ok(())
}

fn run_idf_py(project_dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("idf.py")
        .args(args)
        .current_dir(project_dir)
        .output()
        .map_err(|e| {
            format!(
                "failed to execute idf.py (is ESP-IDF installed and exported?): {}",
                e
            )
        })?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "idf.py failed\nargs: {}\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            stdout,
            stderr
        ));
    }

    Ok(())
}

#[derive(Clone, Debug)]
enum EmittedValue {
    IntLiteral(i64),
    IntVar(String),
    StrLiteral(String),
    StrVar(String),
    UartHandle { id: i64, baud: i64 },
}

#[derive(Debug)]
struct GeneratedEsp32C3 {
    c_source: String,
    gpio_pins: BTreeSet<i64>,
    uart_ports: BTreeSet<i64>,
}

#[derive(Debug)]
struct LowerContext {
    vars: BTreeMap<String, EmittedValue>,
    gpio_pins: BTreeSet<i64>,
    uart_configs: BTreeMap<i64, i64>,
    temp_counter: usize,
    esp32_profile: Esp32C3Profile,
    body_lines: Vec<String>,
}

fn lower_program_to_esp32c3_c(
    program: &Program,
    esp32_profile: Esp32C3Profile,
) -> Result<GeneratedEsp32C3, String> {
    let main_fn = find_main_function(program)?;
    let mut ctx = LowerContext {
        vars: BTreeMap::new(),
        gpio_pins: BTreeSet::new(),
        uart_configs: BTreeMap::new(),
        temp_counter: 0,
        esp32_profile,
        body_lines: Vec::new(),
    };

    for stmt in &main_fn.body {
        lower_stmt(stmt, &mut ctx)?;
    }

    let mut uart_init = String::new();
    for (uart_id, baud) in &ctx.uart_configs {
        uart_init.push_str(&format!(
            "  uart_config_t cfg_{} = {{ .baud_rate = {}, .data_bits = UART_DATA_8_BITS, .parity = UART_PARITY_DISABLE, .stop_bits = UART_STOP_BITS_1, .flow_ctrl = UART_HW_FLOWCTRL_DISABLE, .source_clk = UART_SCLK_DEFAULT }};\n",
            uart_id, baud
        ));
        uart_init.push_str(&format!(
            "  ESP_ERROR_CHECK(uart_param_config({}, &cfg_{}));\n",
            uart_id, uart_id
        ));
        uart_init.push_str(&format!(
            "  ESP_ERROR_CHECK(uart_driver_install({}, 1024, 0, 0, NULL, 0));\n",
            uart_id
        ));
    }

    let mut c = String::new();
    c.push_str("#include <stdio.h>\n");
    c.push_str("#include <string.h>\n");
    c.push_str("#include \"freertos/FreeRTOS.h\"\n");
    c.push_str("#include \"freertos/task.h\"\n");
    c.push_str("#include \"driver/gpio.h\"\n");
    c.push_str("#include \"driver/uart.h\"\n");
    c.push_str("#include \"esp_err.h\"\n\n");
    c.push_str("void app_main(void) {\n");
    c.push_str(&uart_init);
    for pin in &ctx.gpio_pins {
        c.push_str(&format!("  gpio_reset_pin({});\n", pin));
    }
    for line in &ctx.body_lines {
        c.push_str(line);
        c.push('\n');
    }
    c.push_str("  while (1) { vTaskDelay(pdMS_TO_TICKS(1000)); }\n");
    c.push_str("}\n");

    Ok(GeneratedEsp32C3 {
        c_source: c,
        gpio_pins: ctx.gpio_pins,
        uart_ports: ctx.uart_configs.keys().copied().collect(),
    })
}

fn find_main_function(program: &Program) -> Result<&Function, String> {
    program
        .functions
        .iter()
        .find(|f| f.name == "main")
        .ok_or_else(|| "embedded emitter requires `fn main()`".to_string())
}

fn lower_stmt(stmt: &Stmt, ctx: &mut LowerContext) -> Result<(), String> {
    match stmt {
        Stmt::ConstDecl { name, expr, .. } | Stmt::OwnDecl { name, expr, .. } => {
            let value = eval_expr(expr, ctx)?;
            let (decl_ty, expr_c) = declaration_parts(&value)?;
            ctx.body_lines
                .push(format!("  {} {} = {};", decl_ty, name, expr_c));
            let stored_value = match value {
                EmittedValue::UartHandle { id, baud } => EmittedValue::UartHandle { id, baud },
                EmittedValue::StrLiteral(_) | EmittedValue::StrVar(_) => {
                    EmittedValue::StrVar(name.clone())
                }
                _ => EmittedValue::IntVar(name.clone()),
            };
            ctx.vars.insert(name.clone(), stored_value);
            Ok(())
        }
        Stmt::RefDecl { .. } | Stmt::PortOwn { .. } | Stmt::PortRef { .. } => Ok(()),
        Stmt::Assign { name, expr, .. } => {
            let value = eval_expr(expr, ctx)?;
            let existing = ctx
                .vars
                .get(name)
                .cloned()
                .ok_or_else(|| format!("assignment to undeclared variable `{}`", name))?;
            let assign_expr = match existing {
                EmittedValue::StrVar(_) | EmittedValue::StrLiteral(_) => value_to_string_expr(value, ctx)?,
                _ => value_to_int_expr(value, ctx)?.0,
            };
            ctx.body_lines.push(format!("  {} = {};", name, assign_expr));
            ctx.vars.insert(name.clone(), variable_ref_for_existing(name, existing));
            Ok(())
        }
        Stmt::Repeat { times, body, .. } => {
            let repeat_count = eval_repeat_count(times, ctx)?;
            for _ in 0..repeat_count {
                ctx.body_lines.push("  {".to_string());
                for nested in body {
                    lower_stmt(nested, ctx)?;
                }
                ctx.body_lines.push("  }".to_string());
            }
            Ok(())
        }
        Stmt::While { condition, body, .. } => {
            let cond = eval_condition_expr(condition, ctx)?;
            ctx.body_lines.push(format!("  while ({}) {{", cond));
            for nested in body {
                lower_stmt(nested, ctx)?;
            }
            ctx.body_lines.push("  }".to_string());
            Ok(())
        }
        Stmt::Expr { expr, .. } => lower_call_expr(expr, ctx),
        Stmt::Return { .. } => Ok(()),
        other => Err(format!(
            "ESP32-C3 minimal emitter currently does not support statement kind: {:?}",
            other
        )),
    }
}

fn lower_call_expr(expr: &Expr, ctx: &mut LowerContext) -> Result<(), String> {
    let (name, args) = match expr {
        Expr::Call { name, args, .. } => (name.as_str(), args),
        _ => return Ok(()),
    };

    match name {
        "gpio.claim" | "gpio_claim" => Ok(()),
        "gpio.mode" | "gpio_mode" => {
            if args.len() != 2 {
                return Err("gpio.mode expects 2 args (pin, mode)".to_string());
            }
            let (pin_expr, pin_const) = value_to_int_expr(eval_expr(&args[0], ctx)?, ctx)?;
            if let Some(pin) = pin_const {
                let mapped = ctx.esp32_profile.map_pin(pin)?;
                ctx.gpio_pins.insert(mapped);
            }
            let (mode_expr, mode_const) = value_to_int_expr(eval_expr(&args[1], ctx)?, ctx)?;
            let mode_c = if mode_const == Some(1) || mode_expr == "1" {
                "GPIO_MODE_OUTPUT"
            } else {
                "GPIO_MODE_INPUT"
            };
            ctx.body_lines
                .push(format!("  ESP_ERROR_CHECK(gpio_set_direction({}, {}));", pin_expr, mode_c));
            Ok(())
        }
        "gpio.write" | "gpio_write" => {
            if args.len() != 2 {
                return Err("gpio.write expects 2 args (pin, value)".to_string());
            }
            let (pin_expr, pin_const) = value_to_int_expr(eval_expr(&args[0], ctx)?, ctx)?;
            if let Some(pin) = pin_const {
                let mapped = ctx.esp32_profile.map_pin(pin)?;
                ctx.gpio_pins.insert(mapped);
            }
            let (value_expr, _) = value_to_int_expr(eval_expr(&args[1], ctx)?, ctx)?;
            ctx.body_lines.push(format!(
                "  ESP_ERROR_CHECK(gpio_set_level({}, ({} != 0) ? 1 : 0));",
                pin_expr,
                value_expr
            ));
            Ok(())
        }
        "sleep_ms" => {
            if args.len() != 1 {
                return Err("sleep_ms expects 1 arg (milliseconds)".to_string());
            }
            let (delay_expr, _) = value_to_int_expr(eval_expr(&args[0], ctx)?, ctx)?;
            ctx.body_lines
                .push(format!("  vTaskDelay(pdMS_TO_TICKS({}));", delay_expr));
            Ok(())
        }
        "uart.write" | "uart_write" => {
            if args.len() != 2 {
                return Err("uart.write expects 2 args (uart, value)".to_string());
            }
            let uart_id = eval_uart_id(&args[0], ctx)?;
            let msg_value = eval_expr(&args[1], ctx)?;
            match msg_value {
                EmittedValue::StrLiteral(s) => {
                    let escaped = escape_c_string(&s);
                    ctx.body_lines.push(format!(
                        "  uart_write_bytes({}, \"{}\\n\", {});",
                        uart_id,
                        escaped,
                        escaped.len() + 1
                    ));
                }
                EmittedValue::StrVar(name) => {
                    ctx.body_lines
                        .push(format!("  uart_write_bytes({}, {}, strlen({}));", uart_id, name, name));
                    ctx.body_lines
                        .push(format!("  uart_write_bytes({}, \"\\n\", 1);", uart_id));
                }
                other => {
                    let (expr_c, _) = value_to_int_expr(other, ctx)?;
                    let tmp = next_temp_name(ctx, "uart_fmt");
                    ctx.body_lines.push(format!("  char {}[32];", tmp));
                    ctx.body_lines
                        .push(format!("  snprintf({}, sizeof({}), \"%d\", (int)({}));", tmp, tmp, expr_c));
                    ctx.body_lines.push(format!(
                        "  uart_write_bytes({}, {}, strlen({}));",
                        uart_id, tmp, tmp
                    ));
                    ctx.body_lines
                        .push(format!("  uart_write_bytes({}, \"\\n\", 1);", uart_id));
                }
            }
            Ok(())
        }
        _ => Err(format!(
            "ESP32-C3 minimal runtime does not support call `{}` yet",
            name
        )),
    }
}

fn eval_expr(expr: &Expr, ctx: &mut LowerContext) -> Result<EmittedValue, String> {
    match expr {
        Expr::Number(v, _) => Ok(EmittedValue::IntLiteral(*v)),
        Expr::String(s, _) => Ok(EmittedValue::StrLiteral(s.clone())),
        Expr::Bool(b, _) => Ok(EmittedValue::IntLiteral(if *b { 1 } else { 0 })),
        Expr::Var(name, _) => ctx
            .vars
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unsupported unresolved variable `{}` in emitter", name)),
        Expr::Call { name, args, .. } if name == "uart.new" || name == "uart_new" => {
            if args.len() != 2 {
                return Err("uart.new expects 2 args (uart_id, baud)".to_string());
            }
            let uart_id = eval_int(&args[0], ctx)?;
            let baud = eval_int(&args[1], ctx)?;
            ctx.uart_configs.entry(uart_id).or_insert(baud);
            Ok(EmittedValue::UartHandle { id: uart_id, baud })
        }
        Expr::Call { name, args, .. } if name == "gpio.read" || name == "gpio_read" => {
            if args.len() != 1 {
                return Err("gpio.read expects 1 arg (pin)".to_string());
            }
            let (pin_expr, pin_const) = value_to_int_expr(eval_expr(&args[0], ctx)?, ctx)?;
            if let Some(pin) = pin_const {
                let mapped = ctx.esp32_profile.map_pin(pin)?;
                ctx.gpio_pins.insert(mapped);
            }
            let tmp = next_temp_name(ctx, "gpio_read");
            ctx.body_lines
                .push(format!("  int {} = gpio_get_level({});", tmp, pin_expr));
            Ok(EmittedValue::IntVar(tmp))
        }
        Expr::Call { name, args, .. } if name == "uart.read" || name == "uart_read" => {
            if args.len() != 1 {
                return Err("uart.read expects 1 arg (uart)".to_string());
            }
            let uart_id = eval_uart_id(&args[0], ctx)?;
            let buf = next_temp_name(ctx, "uart_buf");
            let n = next_temp_name(ctx, "uart_n");
            ctx.body_lines.push(format!("  char {}[128];", buf));
            ctx.body_lines.push(format!(
                "  int {} = uart_read_bytes({}, (uint8_t*){}, sizeof({}) - 1, pdMS_TO_TICKS(20));",
                n, uart_id, buf, buf
            ));
            ctx.body_lines.push(format!("  if ({} < 0) {} = 0;", n, n));
            ctx.body_lines.push(format!("  {}[{}] = '\\0';", buf, n));
            Ok(EmittedValue::StrVar(buf))
        }
        Expr::Unary { op, rhs, .. } => {
            let (rhs_expr, rhs_const) = value_to_int_expr(eval_expr(rhs, ctx)?, ctx)?;
            let c_op = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
            };
            if let Some(v) = rhs_const {
                let out = match op {
                    UnaryOp::Neg => -v,
                    UnaryOp::Not => {
                        if v == 0 {
                            1
                        } else {
                            0
                        }
                    }
                };
                Ok(EmittedValue::IntLiteral(out))
            } else {
                Ok(EmittedValue::IntVar(format!("({}{})", c_op, rhs_expr)))
            }
        }
        Expr::Binary {
            left,
            op,
            right,
            ..
        } => {
            let (left_expr, left_const) = value_to_int_expr(eval_expr(left, ctx)?, ctx)?;
            let (right_expr, right_const) = value_to_int_expr(eval_expr(right, ctx)?, ctx)?;
            if let (Some(l), Some(r)) = (left_const, right_const) {
                let folded = fold_binary_const(*op, l, r)?;
                Ok(EmittedValue::IntLiteral(folded))
            } else {
                let c_op = binary_op_to_c(*op)?;
                Ok(EmittedValue::IntVar(format!(
                    "(({}) {} ({}))",
                    left_expr, c_op, right_expr
                )))
            }
        }
        _ => Err(format!("unsupported expression in ESP32-C3 emitter: {:?}", expr)),
    }
}

fn binary_op_to_c(op: BinaryOp) -> Result<&'static str, String> {
    match op {
        BinaryOp::Add => Ok("+"),
        BinaryOp::Sub => Ok("-"),
        BinaryOp::Mul => Ok("*"),
        BinaryOp::Div => Ok("/"),
        BinaryOp::Mod => Ok("%"),
        BinaryOp::Eq => Ok("=="),
        BinaryOp::Ne => Ok("!="),
        BinaryOp::Lt => Ok("<"),
        BinaryOp::Lte => Ok("<="),
        BinaryOp::Gt => Ok(">"),
        BinaryOp::Gte => Ok(">="),
        BinaryOp::And => Ok("&&"),
        BinaryOp::Or => Ok("||"),
        BinaryOp::BitAnd => Ok("&"),
        BinaryOp::BitOr => Ok("|"),
        BinaryOp::Shl => Ok("<<"),
        BinaryOp::Shr => Ok(">>"),
        BinaryOp::Xor => Err("xor expression is not yet supported in ESP32-C3 emitter".to_string()),
    }
}

fn fold_binary_const(op: BinaryOp, l: i64, r: i64) -> Result<i64, String> {
    match op {
        BinaryOp::Add => Ok(l + r),
        BinaryOp::Sub => Ok(l - r),
        BinaryOp::Mul => Ok(l * r),
        BinaryOp::Div => {
            if r == 0 {
                Err("division by zero in constant expression".to_string())
            } else {
                Ok(l / r)
            }
        }
        BinaryOp::Mod => {
            if r == 0 {
                Err("modulo by zero in constant expression".to_string())
            } else {
                Ok(l % r)
            }
        }
        BinaryOp::Eq => Ok((l == r) as i64),
        BinaryOp::Ne => Ok((l != r) as i64),
        BinaryOp::Lt => Ok((l < r) as i64),
        BinaryOp::Lte => Ok((l <= r) as i64),
        BinaryOp::Gt => Ok((l > r) as i64),
        BinaryOp::Gte => Ok((l >= r) as i64),
        BinaryOp::And => Ok(((l != 0) && (r != 0)) as i64),
        BinaryOp::Or => Ok(((l != 0) || (r != 0)) as i64),
        BinaryOp::BitAnd => Ok(l & r),
        BinaryOp::BitOr => Ok(l | r),
        BinaryOp::Shl => Ok(l << r),
        BinaryOp::Shr => Ok(l >> r),
        BinaryOp::Xor => Err("xor expression is not yet supported in ESP32-C3 emitter".to_string()),
    }
}

fn eval_int(expr: &Expr, ctx: &mut LowerContext) -> Result<i64, String> {
    match eval_expr(expr, ctx)? {
        EmittedValue::IntLiteral(v) => Ok(v),
        other => Err(format!("expected int, got {:?}", other)),
    }
}

fn eval_uart_id(expr: &Expr, ctx: &mut LowerContext) -> Result<i64, String> {
    match eval_expr(expr, ctx)? {
        EmittedValue::UartHandle { id, baud } => {
            ctx.uart_configs.entry(id).or_insert(baud);
            Ok(id)
        }
        EmittedValue::IntLiteral(v) => Ok(v),
        EmittedValue::IntVar(v) => v
            .parse::<i64>()
            .map_err(|_| format!("uart id variable `{}` is not a constant integer", v)),
        other => Err(format!("expected uart handle or int uart id, got {:?}", other)),
    }
}

fn declaration_parts(value: &EmittedValue) -> Result<(&'static str, String), String> {
    match value {
        EmittedValue::IntLiteral(v) => Ok(("int", v.to_string())),
        EmittedValue::IntVar(expr) => Ok(("int", expr.clone())),
        EmittedValue::StrLiteral(s) => Ok(("const char*", format!("\"{}\"", escape_c_string(s)))),
        EmittedValue::StrVar(expr) => Ok(("const char*", expr.clone())),
        EmittedValue::UartHandle { id, .. } => Ok(("int", id.to_string())),
    }
}

fn value_to_int_expr(value: EmittedValue, _ctx: &mut LowerContext) -> Result<(String, Option<i64>), String> {
    match value {
        EmittedValue::IntLiteral(v) => Ok((v.to_string(), Some(v))),
        EmittedValue::IntVar(v) => Ok((v, None)),
        EmittedValue::UartHandle { id, .. } => Ok((id.to_string(), Some(id))),
        other => Err(format!("expected int-compatible value, got {:?}", other)),
    }
}

fn value_to_string_expr(value: EmittedValue, _ctx: &mut LowerContext) -> Result<String, String> {
    match value {
        EmittedValue::StrLiteral(s) => Ok(format!("\"{}\"", escape_c_string(&s))),
        EmittedValue::StrVar(v) => Ok(v),
        other => Err(format!("expected string-compatible value, got {:?}", other)),
    }
}

fn eval_repeat_count(times: &Expr, ctx: &mut LowerContext) -> Result<i64, String> {
    let value = eval_expr(times, ctx)?;
    let (expr, constant) = value_to_int_expr(value, ctx)?;
    let Some(count) = constant else {
        return Err(format!(
            "repeat currently requires constant iteration count, got expression `{}`",
            expr
        ));
    };
    if count < 0 || count > 1000 {
        return Err("repeat count must be between 0 and 1000".to_string());
    }
    Ok(count)
}

fn eval_condition_expr(condition: &Expr, ctx: &mut LowerContext) -> Result<String, String> {
    let value = eval_expr(condition, ctx)?;
    let (expr, _) = value_to_int_expr(value, ctx)?;
    Ok(format!("({}) != 0", expr))
}

fn next_temp_name(ctx: &mut LowerContext, prefix: &str) -> String {
    let name = format!("__h_{}_{}", prefix, ctx.temp_counter);
    ctx.temp_counter += 1;
    name
}

fn escape_c_string(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn variable_ref_for_existing(name: &str, existing: EmittedValue) -> EmittedValue {
    match existing {
        EmittedValue::StrVar(_) | EmittedValue::StrLiteral(_) => EmittedValue::StrVar(name.to_string()),
        _ => EmittedValue::IntVar(name.to_string()),
    }
}
