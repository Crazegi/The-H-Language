use std::env;
use std::fs;

use hl_lexer::{
    analyze, compile_program, disassemble, parse_source, run_bytecode, run_program, Lexer,
    TokenKind,
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
    Vm,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut mode = Mode::Run;
    let mut path: Option<String> = None;
    let mut out_path: Option<String> = None;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--tokens" => mode = Mode::Tokens,
            "--ast" => mode = Mode::Ast,
            "--compile" => mode = Mode::Compile,
            "--vm" => mode = Mode::Vm,
            "--out" => {
                if i + 1 >= args.len() {
                    eprintln!("Expected a file path after --out");
                    std::process::exit(1);
                }
                out_path = Some(args[i + 1].clone());
                i += 1;
            }
            value => path = Some(value.to_string()),
        }
        i += 1;
    }

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

            let bytecode = match compile_program(&program) {
                Ok(bc) => bc,
                Err(err) => {
                    eprintln!("Compile error: {}", err);
                    std::process::exit(1);
                }
            };

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

            let bytecode = match compile_program(&program) {
                Ok(bc) => bc,
                Err(err) => {
                    eprintln!("Compile error: {}", err);
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
    }
}
