# Contributing to ortu

Thank you for considering a contribution. This document describes how the project works so that your change can be reviewed and merged quickly.

## Ground rules

1. **No real personal data, ever.** Not in code, tests, fixtures, issues, pull requests or commit messages. Use the synthetic generators, or values that are obviously synthetic.
2. **The design document is the source of truth.** Changes follow [docs/systemdesign.md](docs/systemdesign.md). If a change deviates from it, add an ADR ([docs/adr/](docs/adr/README.md)) and update the design document in the same pull request.
3. **Milestones are implemented in order** (systemdesign.md, section 17).

## Development setup

- Install [rustup](https://rustup.rs). The toolchain pinned in `rust-toolchain.toml` (with `rustfmt` and `clippy`) is installed on first use.
- On Windows with the MSVC toolchain, install the Visual Studio C++ build tools and a Windows SDK.
- Optional tools used by CI: `cargo install --locked cargo-deny cargo-audit`.

## Checks

Every pull request must pass the same checks CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo deny check
cargo audit
```

The MSRV job additionally runs `cargo +1.85 check --workspace --all-targets`.

## Code conventions

- All identifiers, comments, documentation and file names are in English. Turkish appears only as data (spoken examples, cue phrases, gazetteers).
- Library crates never print to stdout or stderr, never call `unwrap` or `expect` outside tests, and never log raw values. `SecretValue` exists to make the last rule a compile-time guarantee; do not work around it.
- New algorithms get a document under [docs/algorithms/](docs/algorithms/README.md) following the structure described there.

## Commits and pull requests

- Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/): `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, with the crate as scope where it applies, for example `feat(normalize): parse spoken hundreds`.
- Keep commits focused: one logical change per commit, each one building and passing tests.
- A pull request describes what changed, why, and how it was tested. Link the milestone or ADR it belongs to.

## Reporting security issues

See [SECURITY.md](SECURITY.md). Do not open public issues for vulnerabilities.

## License

By contributing, you agree that your contributions are dual licensed under MIT OR Apache-2.0, as described in the [README](README.md#license).
