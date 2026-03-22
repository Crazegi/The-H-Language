use std::env;
use std::fs;
use std::path::PathBuf;

use hl_lexer::{
    analyze, compile_h_to_native_artifacts_with_options, compile_program_with_options,
    diagnose_cycle_profile_coverage,
    disassemble, load_cycle_profiles_from_file, parse_source, read_package,
    render_contract_report_text, render_profile_doctor_report_text, run_bytecode, run_program,
    write_package, CompileOptions, CycleProfile, Lexer, OptimizationLevel, TokenKind,
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

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut mode = Mode::Run;
    let mut path: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut cycle_profile_name = "generic".to_string();
    let mut cycle_profile_file: Option<String> = None;
    let mut unknown_cycle_cost_policy: Option<UnknownCycleCostPolicy> = None;
    let mut unknown_cycle_cost_fallback: Option<u64> = None;
    let mut contract_report_out: Option<String> = None;
    let mut opt_level = OptimizationLevel::O2;
    let mut const_folding = true;
    let mut peephole = true;
    let mut fast_math = false;
    let mut strict_cycle_contracts = true;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--tokens" => mode = Mode::Tokens,
            "--ast" => mode = Mode::Ast,
            "--compile" => mode = Mode::Compile,
            "--profile-doctor" => mode = Mode::ProfileDoctor,
            "--pack" => mode = Mode::Pack,
            "--run-package" => mode = Mode::RunPackage,
            "--vm" => mode = Mode::Vm,
            "--native" => mode = Mode::Native,
            "--out" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected a file path after --out");
                    std::process::exit(1);
                }
                out_path = Some(args[i + 1].clone());
                i += 1;
            }
            "--cycle-profile" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected profile after --cycle-profile");
                    std::process::exit(1);
                }
                cycle_profile_name = args[i + 1].clone();
                i += 1;
            }
            "--cycle-profile-file" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected file path after --cycle-profile-file");
                    std::process::exit(1);
                }
                cycle_profile_file = Some(args[i + 1].clone());
                i += 1;
            }
            "--unknown-cycle-cost" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected mode after --unknown-cycle-cost (strict|conservative)");
                    std::process::exit(1);
                }
                let value = &args[i + 1];
                unknown_cycle_cost_policy = match UnknownCycleCostPolicy::from_str(value) {
                    Some(v) => Some(v),
                    None => {
                        eprintln!("Unknown unknown-cost mode `{}`", value);
                        std::process::exit(1);
                    }
                };
                i += 1;
            }
            "--unknown-cycle-cost-fallback" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected integer after --unknown-cycle-cost-fallback");
                    std::process::exit(1);
                }
                let value = &args[i + 1];
                unknown_cycle_cost_fallback = match value.parse::<u64>() {
                    Ok(v) => Some(v),
                    Err(_) => {
                        eprintln!("Invalid fallback `{}`; expected non-negative integer", value);
                        std::process::exit(1);
                    }
                };
                i += 1;
            }
            "--contract-report" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected file path after --contract-report");
                    std::process::exit(1);
                }
                contract_report_out = Some(args[i + 1].clone());
                i += 1;
            }
            "--opt-level" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected level after --opt-level (0|1|2|3)");
                    std::process::exit(1);
                }
                let value = &args[i + 1];
                opt_level = match OptimizationLevel::from_str(value) {
                    Some(v) => v,
                    None => {
                        eprintln!("Unknown optimization level `{}`", value);
                        std::process::exit(1);
                    }
                };
                i += 1;
            }
            "--no-const-fold" => const_folding = false,
            "--no-peephole" => peephole = false,
            "--fast-math" => fast_math = true,
            "--relaxed-contracts" => strict_cycle_contracts = false,
            value => path = Some(value.to_string()),
        }
        i += 1;
    }

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

            if let Err(err) = analyze(&program) {
                eprintln!("Semantic error: {}", err);
                std::process::exit(1);
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

            if let Err(err) = analyze(&program) {
                eprintln!("Semantic error: {}", err);
                std::process::exit(1);
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

            if let Err(err) = analyze(&program) {
                eprintln!("Semantic error: {}", err);
                std::process::exit(1);
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

            if let Err(err) = analyze(&program) {
                eprintln!("Semantic error: {}", err);
                std::process::exit(1);
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

            if let Err(err) = analyze(&program) {
                eprintln!("Semantic error: {}", err);
                std::process::exit(1);
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
