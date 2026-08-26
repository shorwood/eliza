// nix develop path:. -c cargo test --test cli --locked
//! End-to-end UI contracts for the compiled CLI.

#![allow(
    clippy::expect_used,
    clippy::missing_panics_doc,
    reason = "UI test failures must identify broken process and fixture contracts"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

// -----------------------------------------------------------------------------
// UiOutput: Captures the complete observable process result.
// -----------------------------------------------------------------------------

#[derive(Debug, Eq, PartialEq)]
struct UiOutput {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

// -----------------------------------------------------------------------------
// UiCase: Loads one command-line interaction from a JSON fixture.
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct UiCase {
    args: Vec<String>,
    code: i32,
}

fn ui_case_load(path: &Path) -> UiCase {
    serde_json::from_str(&fs::read_to_string(path).expect("UI case should be readable"))
        .expect("UI case should be valid JSON")
}

fn ui_cases_load() -> Vec<std::path::PathBuf> {
    let mut paths = fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui"))
        .expect("UI fixture directory should be readable")
        .map(|entry| entry.expect("UI fixture should be readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

// -----------------------------------------------------------------------------
// UiOutputRun: Runs one interaction through the compiled executable.
// -----------------------------------------------------------------------------

fn ui_output_run(case: &UiCase) -> UiOutput {
    let output = Command::new(env!("CARGO_BIN_EXE_eliza"))
        .args(&case.args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ELIZA process should run");
    UiOutput {
        code: output.status.code(),
        stdout: String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    }
}

fn ui_output_load(path: &Path, code: i32) -> UiOutput {
    let output_load = |extension| {
        let path = path.with_extension(extension);
        fs::read_to_string(path).unwrap_or_default()
    };
    UiOutput {
        code: Some(code),
        stdout: output_load("stdout"),
        stderr: output_load("stderr"),
    }
}

// -----------------------------------------------------------------------------
// CliUi: Auto-loads fixtures and verifies complete process results.
// -----------------------------------------------------------------------------

#[test]
fn cli_ui() {
    for path in ui_cases_load() {
        let case = ui_case_load(&path);
        let expected = ui_output_load(&path, case.code);
        assert_eq!(ui_output_run(&case), expected, "{}", path.display());
    }
}
