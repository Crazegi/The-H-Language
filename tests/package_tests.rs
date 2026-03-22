use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use hl_lexer::{
    analyze, compile_program_with_options, parse_source, read_package, run_bytecode, write_package,
    CompileOptions,
};

fn unique_package_path() -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock error")
        .as_nanos();
    std::env::temp_dir().join(format!("hl_pkg_{}.hbcp", nanos))
}

#[test]
fn package_roundtrip_runs_without_parser_pipeline() {
    let src = r#"section .text:
  fn main():
    own r1 = 40
    own r2 = 2
    add r1, r2
    return r1
"#;

    let program = parse_source(src).expect("parse should pass");
    analyze(&program).expect("semantic should pass");

    let compiled = compile_program_with_options(&program, CompileOptions::default())
        .expect("compile should pass");

    let pkg = unique_package_path();
    write_package(&compiled.bytecode, &pkg).expect("package write should pass");

    let loaded = read_package(&pkg).expect("package read should pass");
    let result = run_bytecode(&loaded).expect("loaded package should execute");
    assert_eq!(result.render(), "42");

    let _ = fs::remove_file(pkg);
}
