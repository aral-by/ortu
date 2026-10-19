//! Development automation for the ortu workspace.
//!
//! `cargo xtask check` is the quality gate every change must pass before it is
//! committed (docs/adr/0003-local-quality-gate.md). It runs the checks a hosted
//! CI would run, with the same flags, and reports every step instead of stopping
//! at the first failure.

// A task runner reports progress on the terminal; this crate is never shipped.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::env;
use std::process::{Command, ExitCode};

/// Minimum supported Rust version (ADR-0001). A test keeps it in sync with
/// `Cargo.toml`, `.clippy.toml`, the README badge and the design document.
const MSRV: &str = "1.85";

struct Step {
    name: &'static str,
    commands: Vec<Vec<String>>,
    hint: Option<&'static str>,
}

fn command(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn steps() -> Vec<Step> {
    let msrv_toolchain = format!("+{MSRV}");
    vec![
        Step {
            name: "fmt",
            commands: vec![command(&["cargo", "fmt", "--all", "--check"])],
            hint: Some("run `cargo fmt --all` to apply the formatting"),
        },
        Step {
            name: "clippy",
            commands: vec![command(&[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--locked",
                "--",
                "-D",
                "warnings",
            ])],
            hint: None,
        },
        Step {
            name: "test",
            commands: vec![command(&[
                "cargo",
                "test",
                "--workspace",
                "--all-features",
                "--locked",
            ])],
            hint: None,
        },
        Step {
            name: "msrv",
            commands: vec![
                command(&[
                    "rustup",
                    "toolchain",
                    "install",
                    MSRV,
                    "--profile",
                    "minimal",
                    "--no-self-update",
                ]),
                command(&[
                    "cargo",
                    &msrv_toolchain,
                    "check",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--locked",
                ]),
            ],
            hint: None,
        },
        Step {
            name: "deny",
            commands: vec![command(&["cargo", "deny", "check"])],
            hint: Some("if cargo-deny is missing: `cargo install --locked cargo-deny`"),
        },
        Step {
            name: "audit",
            commands: vec![command(&["cargo", "audit"])],
            hint: Some("if cargo-audit is missing: `cargo install --locked cargo-audit`"),
        },
    ]
}

fn run(step: &Step) -> bool {
    for parts in &step.commands {
        println!("    $ {}", parts.join(" "));
        let Some((program, args)) = parts.split_first() else {
            continue;
        };
        match Command::new(program).args(args).status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("    `{program}` exited with {status}");
                return false;
            }
            Err(error) => {
                eprintln!("    could not start `{program}`: {error}");
                return false;
            }
        }
    }
    true
}

fn check(selected: &[String]) -> ExitCode {
    let all = steps();
    if let Some(unknown) = selected
        .iter()
        .find(|name| !all.iter().any(|step| step.name == name.as_str()))
    {
        let names: Vec<&str> = all.iter().map(|step| step.name).collect();
        eprintln!("unknown step `{unknown}`; available: {}", names.join(", "));
        return ExitCode::from(2);
    }

    let mut results = Vec::new();
    for step in all
        .iter()
        .filter(|step| selected.is_empty() || selected.iter().any(|name| name == step.name))
    {
        println!("\n==> {}", step.name);
        results.push((step, run(step)));
    }

    println!("\nQuality gate summary");
    for (step, passed) in &results {
        println!(
            "    {:<8} {}",
            step.name,
            if *passed { "ok" } else { "FAILED" }
        );
    }
    let failed: Vec<_> = results.iter().filter(|(_, passed)| !passed).collect();
    if failed.is_empty() {
        return ExitCode::SUCCESS;
    }
    for (step, _) in failed {
        if let Some(hint) = step.hint {
            eprintln!("    {}: {hint}", step.name);
        }
    }
    ExitCode::FAILURE
}

/// Runs the randomized stress tests (ADR-0005). Coverage-guided fuzzing with
/// cargo-fuzz lives in `fuzz/` and needs Linux or macOS with nightly Rust.
fn fuzz(args: &[String]) -> ExitCode {
    let iterations = match args {
        [] => "1000000",
        [count] if count.parse::<u64>().is_ok() => count.as_str(),
        _ => {
            eprintln!("usage: cargo xtask fuzz [ITERATIONS]");
            return ExitCode::from(2);
        }
    };
    // Every crate with a `tests/stress.rs` target (ADR-0005).
    let args = [
        "test",
        "--package",
        "ortu-normalize",
        "--package",
        "ortu-tr",
        "--release",
        "--locked",
        "--test",
        "stress",
        "--",
        "--ignored",
        "--nocapture",
    ];
    println!("==> stress ({iterations} iterations)");
    println!(
        "    $ ORTU_STRESS_ITERATIONS={iterations} cargo {}",
        args.join(" ")
    );
    // The iteration count goes to the child only; this process's environment
    // is never modified.
    match Command::new("cargo")
        .args(args)
        .env("ORTU_STRESS_ITERATIONS", iterations)
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("    stress tests exited with {status}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("    could not start `cargo`: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    let names: Vec<&str> = steps().iter().map(|step| step.name).collect();
    println!(
        "Usage:\n  \
         cargo xtask check [STEP...]    run the quality gate (steps: {})\n  \
         cargo xtask fuzz [ITERATIONS]  run the randomized stress tests (default 1000000)",
        names.join(", ")
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => check(&args[1..]),
        Some("fuzz") => fuzz(&args[1..]),
        None | Some("help" | "--help" | "-h") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown task `{other}`\n");
            print_help();
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::MSRV;

    fn workspace_file(relative: &str) -> String {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        fs::read_to_string(root.join(relative)).unwrap()
    }

    /// Value of a `key = "value"` line, ignoring surrounding whitespace.
    fn quoted_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix(key)?
                .trim_start()
                .strip_prefix('=')?
                .trim()
                .strip_prefix('"')?
                .strip_suffix('"')
        })
    }

    #[test]
    fn msrv_is_declared_consistently_across_the_repository() {
        assert_eq!(
            quoted_value(&workspace_file("Cargo.toml"), "rust-version"),
            Some(MSRV)
        );
        assert_eq!(
            quoted_value(&workspace_file(".clippy.toml"), "msrv"),
            Some(MSRV)
        );
        assert!(workspace_file("README.md").contains(&format!("MSRV-{MSRV}")));
        assert!(workspace_file("docs/systemdesign.md").contains(&format!("MSRV: Rust {MSRV}")));
    }
}
