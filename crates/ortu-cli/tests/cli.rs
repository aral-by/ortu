//! Smoke tests for the `ortu` binary: the milestone M0 acceptance criteria.

use std::io;
use std::process::{Command, Output};

fn run_ortu(args: &[&str]) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_ortu")).args(args).output()
}

fn usage_line(text: &str) -> Option<&str> {
    text.lines().find(|line| line.starts_with("Usage: "))
}

#[test]
fn help_succeeds_and_shows_a_platform_independent_usage_line() {
    let output = run_ortu(&["--help"]).unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        usage_line(&stdout),
        Some("Usage: ortu"),
        "help output: {stdout}"
    );
}

#[test]
fn version_matches_the_workspace_version() {
    let output = run_ortu(&["--version"]).unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), format!("ortu {}", env!("CARGO_PKG_VERSION")));
}

#[test]
fn missing_arguments_print_help_and_exit_with_the_usage_code() {
    let output = run_ortu(&[]).unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "usage errors must exit with 2 (ADR-0002)"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(usage_line(&stderr), Some("Usage: ortu"), "stderr: {stderr}");
}
