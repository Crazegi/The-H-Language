use std::env;
use std::fs;

use hl_lexer::{Lexer, TokenKind};

const SAMPLE: &str = r#"section .data:
    name: "Engine_Temp"

section .text:
  fn calculate_temp():
    own r1 = 45
    own r2 = 15
    add r1, r2
    ref label = &name

    print:
            event: "diagnostic"
      sensor: label
      reading: r1
            status: "stable"
"#;

fn main() {
    let input = match env::args().nth(1) {
        Some(path) => match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(err) => {
                eprintln!("Failed to read {}: {}", path, err);
                std::process::exit(1);
            }
        },
        None => SAMPLE.to_string(),
    };

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
