// nix develop path:. -c cargo test --test cli --locked
//! End-to-end UI contracts for the compiled CLI.

#![allow(
    clippy::expect_used,
    clippy::missing_panics_doc,
    reason = "UI test failures must identify broken process and fixture contracts"
)]

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

// -----------------------------------------------------------------------------
// UiOutput: Captures the complete observable process result.
// -----------------------------------------------------------------------------

/// Complete observable output from one CLI invocation.
#[derive(Debug, Eq, PartialEq)]
struct UiOutput {
    /// Process exit code, or `None` when terminated by a signal.
    code: Option<i32>,
    /// Complete standard output.
    stdout: String,
    /// Complete standard error.
    stderr: String,
}

impl UiOutput {
    /// Run the compiled executable with one fixture's arguments.
    fn from_process(case: &UiCase) -> Self {
        let output = Command::new(env!("CARGO_BIN_EXE_eliza"))
            .args(&case.args)
            .env("NO_COLOR", "1")
            .output()
            .expect("ELIZA process should run");
        Self {
            code: output.status.code(),
            stdout: String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
            stderr: String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
        }
    }

    /// Load the exact output expected beside one JSON fixture.
    fn from_fixture(path: &Path, code: i32) -> Self {
        Self {
            code: Some(code),
            stdout: fixture_output(&path.with_extension("stdout")),
            stderr: fixture_output(&path.with_extension("stderr")),
        }
    }
}

// -----------------------------------------------------------------------------
// UiCase: Loads one command-line interaction from a JSON fixture.
// -----------------------------------------------------------------------------

/// Arguments and expected exit code loaded from one UI fixture.
#[derive(Deserialize)]
struct UiCase {
    /// Command-line arguments passed verbatim to the executable.
    args: Vec<String>,
    /// Exact expected process exit code.
    code: i32,
}

impl UiCase {
    /// Load one command-line interaction from a JSON fixture.
    fn from_fixture(path: &Path) -> Self {
        let content = fs::read_to_string(path).expect("UI case should be readable");
        serde_json::from_str(&content).expect("UI case should be valid JSON")
    }
}

/// Find every JSON fixture in stable path order.
fn ui_cases_load() -> Vec<PathBuf> {
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
// FixtureOutput: Loads an optional exact-output fixture.
// -----------------------------------------------------------------------------

/// Load exact output, treating an absent fixture as intentionally empty output.
fn fixture_output(path: &Path) -> String {
    match fs::read_to_string(path) {
        Ok(output) => output,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => panic!("{} should be readable: {error}", path.display()),
    }
}

// -----------------------------------------------------------------------------
// CliUi: Auto-loads fixtures and verifies complete process results.
// -----------------------------------------------------------------------------

#[test]
fn cli_ui() {
    for path in ui_cases_load() {
        let case = UiCase::from_fixture(&path);
        let expected = UiOutput::from_fixture(&path, case.code);
        assert_eq!(
            UiOutput::from_process(&case),
            expected,
            "{}",
            path.display()
        );
    }
}
