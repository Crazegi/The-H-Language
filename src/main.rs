use std::fs;
use std::path::PathBuf;

use clap::{ArgAction, ArgGroup, Parser, ValueEnum};
use hl_lexer::{
    analyze_with_warnings, compile_h_to_native_artifacts_with_options, compile_program_with_options,
    diagnose_cycle_profile_coverage, disassemble, load_cycle_profiles_from_file, parse_source,
    read_package, render_contract_report_text, render_profile_doctor_report_text, run_bytecode,
    run_program, write_package, CompileOptions, CycleProfile, Lexer, OptimizationLevel, TokenKind,
    UnknownCycleCostPolicy,
};

const SAMPLE: &str = r#"section .data:
  engine_name: "Engine_Temp"
  threshold: 65

section .text:
  fn calibrate(base, delta):
    own r9 = base
    add r9, delta
    return r9

  fn main():
    own r1 = 45
    own r2 = 15
    add r1, r2

    own r3 = calibrate(r1, 5)
    ref label = &engine_name

    if r3 >= threshold:
      print:
        event: "warning"
        sensor: label
        reading: r3
        status: "high"
    else:
      print:
        event: "diagnostic"
        sensor: label
        reading: r3
        status: "stable"

    while r1 < 70:
      add r1, 2

    return r1
"#;

#[derive(Clone, Copy)]
enum Mode {
    Run,
    Tokens,
    Ast,
    Compile,
    ProfileDoctor,
    Pack,
    RunPackage,
    Vm,
    Native,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliUnknownCycleCost {
    Strict,
    Conservative,
}

impl From<CliUnknownCycleCost> for UnknownCycleCostPolicy {
    fn from(value: CliUnknownCycleCost) -> Self {
        match value {
            CliUnknownCycleCost::Strict => UnknownCycleCostPolicy::Strict,
            CliUnknownCycleCost::Conservative => UnknownCycleCostPolicy::Conservative,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum CliOptimizationLevel {
    #[value(name = "0")]
    O0,
    #[value(name = "1")]
    O1,
    #[value(name = "2")]
    O2,
    #[value(name = "3")]
    O3,
}

impl From<CliOptimizationLevel> for OptimizationLevel {
    fn from(value: CliOptimizationLevel) -> Self {
        match value {
            CliOptimizationLevel::O0 => OptimizationLevel::O0,
            CliOptimizationLevel::O1 => OptimizationLevel::O1,
            CliOptimizationLevel::O2 => OptimizationLevel::O2,
            CliOptimizationLevel::O3 => OptimizationLevel::O3,
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "hl-lex")]
#[command(about = "H language lexer/compiler/runtime driver")]
#[command(group(
    ArgGroup::new("mode")
        .args([
            "tokens",
            "ast",
            "compile",
            "profile_doctor",
            "pack",
            "run_package",
            "vm",
            "native",
        ])
        .multiple(false)
))]
struct Cli {
    #[arg(long, action = ArgAction::SetTrue)]
    tokens: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    ast: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    compile: bool,
    #[arg(long = "profile-doctor", action = ArgAction::SetTrue)]
    profile_doctor: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    pack: bool,
    #[arg(long = "run-package", action = ArgAction::SetTrue)]
    run_package: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    vm: bool,
    #[arg(long, action = ArgAction::SetTrue)]
    native: bool,

    #[arg(long)]
    out: Option<String>,
    #[arg(long = "cycle-profile", default_value = "generic")]
    cycle_profile: String,
    #[arg(long = "cycle-profile-file")]
    cycle_profile_file: Option<String>,
    #[arg(long = "unknown-cycle-cost", value_enum)]
    unknown_cycle_cost: Option<CliUnknownCycleCost>,
    #[arg(long = "unknown-cycle-cost-fallback")]
    unknown_cycle_cost_fallback: Option<u64>,
    #[arg(long = "contract-report")]
    contract_report: Option<String>,
    #[arg(long = "opt-level", value_enum, default_value = "2")]
    opt_level: CliOptimizationLevel,
    #[arg(long = "no-const-fold", action = ArgAction::SetFalse, default_value_t = true)]
    const_folding: bool,
    #[arg(long = "no-peephole", action = ArgAction::SetFalse, default_value_t = true)]
    peephole: bool,
    #[arg(long = "fast-math", action = ArgAction::SetTrue)]
    fast_math: bool,
    #[arg(long = "relaxed-contracts", action = ArgAction::SetFalse, default_value_t = true)]
    strict_cycle_contracts: bool,

    path: Option<String>,
}

fn mode_from_cli(cli: &Cli) -> Mode {
    if cli.tokens {
        Mode::Tokens
    } else if cli.ast {
        Mode::Ast
    } else if cli.compile {
        Mode::Compile
    } else if cli.profile_doctor {
        Mode::ProfileDoctor
    } else if cli.pack {
        Mode::Pack
    } else if cli.run_package {
        Mode::RunPackage
    } else if cli.vm {
        Mode::Vm
    } else if cli.native {
        Mode::Native
    } else {
        Mode::Run
    }
}

fn main() {
    let cli = Cli::parse();
    let mode = mode_from_cli(&cli);
    let path = cli.path.clone();
    let out_path = cli.out.clone();
    let cycle_profile_name = cli.cycle_profile.clone();
    let cycle_profile_file = cli.cycle_profile_file.clone();
    let unknown_cycle_cost_policy = cli.unknown_cycle_cost.map(UnknownCycleCostPolicy::from);
    let unknown_cycle_cost_fallback = cli.unknown_cycle_cost_fallback;
    let contract_report_out = cli.contract_report.clone();
    let opt_level = OptimizationLevel::from(cli.opt_level);
    let const_folding = cli.const_folding;
    let peephole = cli.peephole;
    let fast_math = cli.fast_math;
    let strict_cycle_contracts = cli.strict_cycle_contracts;

    let (cycle_profile, cycle_profile_override) = if let Some(file_path) = cycle_profile_file {
        let profiles = match load_cycle_profiles_from_file(&file_path) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("Failed to load cycle profiles: {}", err);
                std::process::exit(1);
            }
        };

        let mut selected = match profiles.get(&cycle_profile_name) {
            Some(p) => p.clone(),
            None => {
                eprintln!(
                    "Unknown cycle profile `{}` in profile file {}",
                    cycle_profile_name, file_path
                );
                std::process::exit(1);
            }
        };

        if let Some(policy) = unknown_cycle_cost_policy {
            selected.unknown_policy = policy;
        }
        if let Some(fallback) = unknown_cycle_cost_fallback {
            selected.conservative_fallback = fallback;
        }

        let base = CycleProfile::from_str(&cycle_profile_name).unwrap_or(CycleProfile::Generic);
        (base, Some(selected))
    } else {
        let base = match CycleProfile::from_str(&cycle_profile_name) {
            Some(v) => v,
            None => {
                eprintln!(
                    "Unknown built-in cycle profile `{}`. Use --cycle-profile-file for custom profiles.",
                    cycle_profile_name
                );
                std::process::exit(1);
            }
        };

        (base, None)
    };

    let compile_options = CompileOptions {
        cycle_profile,
        cycle_profile_override,
        opt_level,
        const_folding,
        peephole,
        fast_math,
        strict_cycle_contracts,
    };

    let input_path = path.clone();

    let input = match path {
        Some(path) => match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                eprintln!("Failed to read {}: {}", path, err);
                std::process::exit(1);
            }
        },
        None => SAMPLE.to_string(),
    };

    match mode {
        Mode::Tokens => {
            let mut lexer = Lexer::new(&input);
            match lexer.tokenize() {
                Ok(tokens) => {
                    for t in tokens {
                        if t.kind == TokenKind::Eof {
                            println!("{:>14} @ {}:{}", t.kind, t.span.line, t.span.column);
                        } else {
                            println!(
                                "{:>14} {:<20} @ {}:{}",
                                t.kind,
                                format!("{:?}", t.lexeme),
                                t.span.line,
                                t.span.column
                            );
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Lexer error: {}", err);
                    std::process::exit(1);
                }
            }
        }
        Mode::Ast => match parse_source(&input) {
            Ok(program) => println!("{:#?}", program),
            Err(err) => {
                eprintln!("Parse error: {}", err);
                std::process::exit(1);
            }
        },
        Mode::Compile => {
            let program = match parse_source(&input) {
                Ok(p) => p,
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

            let compiled = match compile_program_with_options(&program, compile_options.clone()) {
                Ok(bc) => bc,
                Err(err) => {
                    eprintln!("Compile error: {}", err);
                    std::process::exit(1);
                }
            };
            let bytecode = compiled.bytecode;

            let disasm = disassemble(&bytecode);
            if let Some(path) = out_path {
                if let Err(err) = fs::write(&path, disasm) {
                    eprintln!("Failed to write {}: {}", path, err);
                    std::process::exit(1);
                }
                println!("compiled_output: {}", path);
            } else {
                println!("{}", disasm);
            }

            if let Some(path) = contract_report_out.as_ref() {
                let text = render_contract_report_text(&compiled.contract_reports);
                if let Err(err) = fs::write(path, text) {
                    eprintln!("Failed to write {}: {}", path, err);
                    std::process::exit(1);
                }
                println!("contract_report: {}", path);
            }
        }
        Mode::ProfileDoctor => {
            let program = match parse_source(&input) {
                Ok(p) => p,
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

            let report = diagnose_cycle_profile_coverage(&program, &compile_options);
            let text = render_profile_doctor_report_text(&report);

            if let Some(path) = out_path {
                if let Err(err) = fs::write(&path, text) {
                    eprintln!("Failed to write {}: {}", path, err);
                    std::process::exit(1);
                }
                println!("profile_doctor_output: {}", path);
            } else {
                println!("{}", text);
            }
        }
        Mode::Pack => {
            let program = match parse_source(&input) {
                Ok(p) => p,
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

            let compiled = match compile_program_with_options(&program, compile_options.clone()) {
                Ok(bc) => bc,
                Err(err) => {
                    eprintln!("Compile error: {}", err);
                    std::process::exit(1);
                }
            };

            let out = match out_path {
                Some(path) => PathBuf::from(path),
                None => {
                    let mut default_name = match input_path {
                        Some(p) => {
                            let stem = std::path::Path::new(&p)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("program");
                            PathBuf::from(stem)
                        }
                        None => PathBuf::from("program"),
                    };
                    default_name.set_extension("hbcp");
                    default_name
                }
            };

            if let Err(err) = write_package(&compiled.bytecode, &out) {
                eprintln!("Package error: {}", err);
                std::process::exit(1);
            }

            if let Some(path) = contract_report_out.as_ref() {
                let text = render_contract_report_text(&compiled.contract_reports);
                if let Err(err) = fs::write(path, text) {
                    eprintln!("Failed to write {}: {}", path, err);
                    std::process::exit(1);
                }
                println!("contract_report: {}", path);
            }

            println!("package_output: {}", out.display());
        }
        Mode::RunPackage => {
            let package_path = match input_path {
                Some(p) => PathBuf::from(p),
                None => {
                    eprintln!("Expected a package path for --run-package");
                    std::process::exit(1);
                }
            };

            let bytecode = match read_package(&package_path) {
                Ok(p) => p,
                Err(err) => {
                    eprintln!("Package error: {}", err);
                    std::process::exit(1);
                }
            };

            match run_bytecode(&bytecode) {
                Ok(value) => println!("program_return: {}", value.render()),
                Err(err) => {
                    eprintln!("VM error: {}", err);
                    std::process::exit(1);
                }
            }
        }
        Mode::Vm => {
            let program = match parse_source(&input) {
                Ok(p) => p,
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

            let compiled = match compile_program_with_options(&program, compile_options.clone()) {
                Ok(bc) => bc,
                Err(err) => {
                    eprintln!("Compile error: {}", err);
                    std::process::exit(1);
                }
            };
            let bytecode = compiled.bytecode;

            if let Some(path) = contract_report_out.as_ref() {
                let text = render_contract_report_text(&compiled.contract_reports);
                if let Err(err) = fs::write(path, text) {
                    eprintln!("Failed to write {}: {}", path, err);
                    std::process::exit(1);
                }
                println!("contract_report: {}", path);
            }

            match run_bytecode(&bytecode) {
                Ok(value) => println!("program_return: {}", value.render()),
                Err(err) => {
                    eprintln!("VM error: {}", err);
                    std::process::exit(1);
                }
            }
        }
        Mode::Run => {
            let program = match parse_source(&input) {
                Ok(p) => p,
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

            match run_program(&program) {
                Ok(value) => println!("program_return: {}", value.render()),
                Err(err) => {
                    eprintln!("Runtime error: {}", err);
                    std::process::exit(1);
                }
            }
        }
        Mode::Native => {
            let bin_path = match out_path {
                Some(path) => PathBuf::from(path),
                None => {
                    let mut default_name = match input_path {
                        Some(p) => {
                            let stem = std::path::Path::new(&p)
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("a.out");
                            PathBuf::from(stem)
                        }
                        None => PathBuf::from("h_program"),
                    };
                    if cfg!(windows) {
                        default_name.set_extension("exe");
                    }
                    default_name
                }
            };

            match compile_h_to_native_artifacts_with_options(&input, &bin_path, compile_options) {
                Ok(artifacts) => {
                    println!("native_object: {}", artifacts.object_path.display());
                    println!("native_binary: {}", artifacts.executable_path.display());
                }
                Err(err) => {
                    eprintln!("Native compile error: {}", err);
                    std::process::exit(1);
                }
            }
        }
    }
}
