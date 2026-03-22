use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let script = PathBuf::from("scripts").join("generate_changelog.ps1");

    if !script.exists() {
        eprintln!(
            "Missing script: {}. Expected repository script for changelog generation.",
            script.display()
        );
        return ExitCode::from(1);
    }

    let status = Command::new("powershell")
        .args([
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            script.to_string_lossy().as_ref(),
        ])
        .status();

    match status {
        Ok(code) if code.success() => ExitCode::SUCCESS,
        Ok(code) => {
            let c = code.code().unwrap_or(1);
            eprintln!("Changelog generation failed with exit code {}", c);
            ExitCode::from(c as u8)
        }
        Err(err) => {
            eprintln!("Failed to launch PowerShell: {}", err);
            ExitCode::from(1)
        }
    }
}
