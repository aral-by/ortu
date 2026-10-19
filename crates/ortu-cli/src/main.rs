//! The `ortu` command-line interface.
//!
//! Subcommands are added by the milestone that implements them
//! (docs/systemdesign.md, section 13.2), so the binary never advertises a
//! command that does not work yet. Exit codes follow
//! docs/adr/0002-cli-exit-codes.md.

use std::process::ExitCode;

use clap::Parser;

/// PII detection, masking and encryption for Turkish speech and LLM applications.
#[derive(Debug, Parser)]
// `bin_name` keeps usage lines identical on every platform (no `ortu.exe`).
#[command(
    name = "ortu",
    bin_name = "ortu",
    version,
    arg_required_else_help = true
)]
struct Cli {}

fn main() -> ExitCode {
    let _cli = Cli::parse();
    ExitCode::SUCCESS
}
