# Contributing to ortu

Thank you for considering a contribution. This document describes how the project works so that your change can be reviewed and merged quickly.

## Ground rules

1. **No real personal data, ever.** Not in code, tests, fixtures, issues, pull requests or commit messages. Use the synthetic generators, or values that are obviously synthetic.
2. **The design document is the source of truth.** Changes follow [docs/systemdesign.md](docs/systemdesign.md). If a change deviates from it, add an ADR ([docs/adr/](docs/adr/README.md)) and update the design document in the same pull request.
3. **Milestones are implemented in order** (systemdesign.md, section 17).

## Development setup

- Install [rustup](https://rustup.rs). The toolchain pinned in `rust-toolchain.toml` (with `rustfmt` and `clippy`) is installed on first use.
- On Windows with the MSVC toolchain, install the Visual Studio C++ build tools and a Windows SDK.
- Tools used by the quality gate: `cargo install --locked cargo-deny cargo-audit`.

## Quality gate

The project has no hosted CI (ADR-0003). Instead, every commit must pass one command:

```sh
cargo xtask check
```

It runs, in order and with the exact flags shown in its output: rustfmt, Clippy with warnings as errors, the test suite, the MSRV 1.85 build, cargo-deny and cargo-audit. Every step runs even if an earlier one fails, and a summary lists the result of each. Run a subset with `cargo xtask check clippy test`.

Paste the summary into your pull request description. Changes that touch platform-specific behavior should state which operating systems the gate was run on.

## Code conventions

- All identifiers, comments, documentation and file names are in English. Turkish appears only as data (spoken examples, cue phrases, gazetteers).
- Library crates never print to stdout or stderr, never call `unwrap` or `expect` outside tests, and never log raw values. `SecretValue` exists to make the last rule a compile-time guarantee; do not work around it.
- New algorithms get a document under [docs/algorithms/](docs/algorithms/README.md) following the structure described there.
- Turkish word lists are data, not code: normalization's lists live in `crates/ortu-normalize/src/lexicon.rs` and the detectors' cue rules, area codes and name lists in `crates/ortu-tr/src/data.rs`, written in folded form (tests enforce it). Changing a list never requires touching an algorithm (ADR-0004, ADR-0006).
- Test vectors for validators and tokens come from a published reference or an independent implementation, never from the code under test.

## Fuzzing and stress tests

- `cargo xtask fuzz [ITERATIONS]` runs the seeded randomized stress tests (default 1,000,000 iterations; `ORTU_STRESS_SEED` reproduces a failure). It works on every platform.
- On Linux or macOS, coverage-guided fuzzing is available as well: `cargo +nightly fuzz run <target>` with `normalize`, `parse_spoken_number` or `iban_validate`. It does not link on Windows with MSVC (ADR-0005).

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/): `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, with the crate as scope where it applies, for example `feat(normalize): parse spoken hundreds`.
- Keep commits focused: one logical change per commit, each one building and passing tests.
- A pull request describes what changed, why, and how it was tested. Link the milestone or ADR it belongs to.

## Reporting security issues

See [SECURITY.md](SECURITY.md). Do not open public issues for vulnerabilities.

## License

By contributing, you agree that your contributions are dual licensed under MIT OR Apache-2.0, as described in the [README](README.md#license).
