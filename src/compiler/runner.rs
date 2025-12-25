use anyhow::Result;
use std::process::Command;
use tempfile::TempDir;

#[derive(Debug)]
pub struct CompileResult {
    pub success: bool,
    pub stderr: String,
    pub stdout: String,
}

#[derive(Debug)]
pub enum ValidationResult {
    CompileError(String),
    WrongOutput { expected: String, got: String },
    Success,
}

pub fn validate_solution(code: &str, expected_output: &str) -> Result<ValidationResult> {
    let temp_dir = TempDir::new()?;
    let source_path = temp_dir.path().join("solution.rs");
    let binary_path = temp_dir.path().join("solution");

    // Write the player's code
    std::fs::write(&source_path, code)?;

    // Compile with rustc
    let compile_output = Command::new("rustc")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .arg("--edition=2021")
        .output()?;

    if !compile_output.status.success() {
        let stderr = String::from_utf8_lossy(&compile_output.stderr).to_string();
        return Ok(ValidationResult::CompileError(clean_error_output(&stderr)));
    }

    // Run the compiled binary
    let run_output = Command::new(&binary_path).output()?;

    let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
    let stdout_trimmed = stdout.trim();
    let expected_trimmed = expected_output.trim();

    if stdout_trimmed == expected_trimmed {
        Ok(ValidationResult::Success)
    } else {
        Ok(ValidationResult::WrongOutput {
            expected: expected_trimmed.to_string(),
            got: stdout_trimmed.to_string(),
        })
    }
}

fn clean_error_output(stderr: &str) -> String {
    // Remove the temp file path noise, keep the useful error info
    stderr
        .lines()
        .filter(|line| !line.contains("Compiling") && !line.trim().is_empty())
        .map(|line| {
            // Replace temp path with just "solution.rs"
            if line.contains("solution.rs") {
                line.split("solution.rs")
                    .enumerate()
                    .map(|(i, part)| {
                        if i == 0 {
                            "solution.rs".to_string()
                        } else {
                            part.to_string()
                        }
                    })
                    .collect::<String>()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
