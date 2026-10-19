# ADR-0003: Local quality gate instead of hosted CI

- **Status:** Accepted
- **Date:** 2026-10-19
- **Affects:** systemdesign.md sections 0, 12, 15, 16, 17, 18, 19 (D1) and 22; ADR-0001; `.github/workflows/`; README; CONTRIBUTING

## Context

M0 introduced a GitHub Actions workflow running rustfmt, Clippy, tests on three operating systems, an MSRV check, cargo-deny and cargo-audit. The project is maintained by a single author with no budget for paid services, and the hosting account cannot run Actions jobs at the moment. A workflow that never runs is worse than none: its badge and the documents describing it would claim a guarantee that does not exist.

## Options considered

1. **Keep the workflow and wait.** Documents stay unchanged, but every claim of "checked in CI" is false until jobs run, and a red badge greets every visitor.
2. **Remove the workflow; document the commands.** Honest, but six long commands with exact flags drift between contributors and documents.
3. **Remove the workflow; ship the gate as code.** A `cargo xtask check` command runs every check with the flags CI used, on any platform, with no extra tools beyond Rust.

## Decision

Option 3. `.github/workflows/ci.yml` is removed. `cargo xtask check` (the `xtask` crate, invoked through a Cargo alias in `.cargo/config.toml`) is the quality gate. Every commit on `main` must pass it, and every milestone's acceptance criteria refer to it.

## Rationale

- The gate is the same everywhere: one command, versioned with the code, so the checks cannot silently diverge from the documents.
- It needs nothing beyond the Rust toolchain, `cargo-deny` and `cargo-audit`, all free and local.
- The `xtask` crate carries a consistency test that fails when the MSRV differs between `Cargo.toml`, `.clippy.toml`, the README badge and the design document.
- Dependabot stays enabled: it is free for public repositories and only proposes pull requests; each proposal is verified locally with the gate before merging.

## Consequences

- Checks run only on the author's machine (Windows). Linux and macOS are not exercised until hosted CI returns or a contributor runs the gate there; platform-specific code paths (audio I/O, PyO3 wheels) must be reviewed with that in mind.
- Release artifacts for other platforms (Python wheels, M9) need a decision when that milestone starts; this ADR does not settle it.
- Re-introducing hosted CI requires a new ADR. The workflow should then call `cargo xtask check` so that the gate stays defined in one place.
