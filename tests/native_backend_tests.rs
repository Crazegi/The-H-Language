use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use hl_lexer::compile_h_to_native_binary;

const PROGRAM: &str = r#"section .data:
  name: "Engine"

section .text:
  fn main():
    own r1 = 40
    add r1, 2
    print:
      event: "native"
      sensor: name
      reading: r1
    return r1
"#;

fn unique_temp_path(file_name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock error")
        .as_nanos();
    std::env::temp_dir().join(format!("hl_native_{}_{}", nanos, file_name))
}

#[test]
fn compiles_h_program_to_native_binary_and_runs() {
    let mut out = unique_temp_path("program");
    if cfg!(windows) {
        out.set_extension("exe");
    }

    let binary = compile_h_to_native_binary(PROGRAM, &out).expect("native compile should succeed");
    assert!(binary.exists(), "native binary should be created");

    let output = Command::new(&binary)
        .output()
        .expect("compiled binary should execute");

    assert!(
        output.status.success(),
        "binary execution failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("program_return: 42"));

    let _ = fs::remove_file(&binary);
    let generated_rs = binary.with_extension("rs");
    let _ = fs::remove_file(generated_rs);
}
