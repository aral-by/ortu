# ADR-0001: Rust edition 2024 and MSRV 1.85

- **Status:** Accepted
- **Date:** 2026-10-19
- **Affects:** systemdesign.md section 16 (MSRV), workspace `Cargo.toml`, quality gate

## Context

The design document set the minimum supported Rust version (MSRV) to 1.80. That version predates Rust edition 2024 (stabilized in Rust 1.85, February 2025) and Cargo's resolver 3, which respects `rust-version` when selecting dependency versions. At the start of implementation the current stable release is 1.99.

## Options considered

1. **Keep MSRV 1.80, edition 2021.** Widest compatibility; forgoes edition 2024 and the MSRV-aware resolver.
2. **MSRV 1.85, edition 2024.** The oldest release that supports edition 2024; about twenty months old at the time of this decision.
3. **Track the latest stable.** Simplest for maintainers; hostile to adopters on long-term-support distributions and pinned enterprise toolchains.

## Decision

Edition 2024 with MSRV 1.85, enforced by the quality gate (`cargo xtask check`, ADR-0003). The development toolchain is pinned separately in `rust-toolchain.toml` and bumped deliberately.

## Rationale

- The engine has no users yet, so there is no installed base on 1.80 to protect.
- Resolver 3 keeps dependency upgrades from silently raising the effective MSRV, which matters more to adopters than the five releases between 1.80 and 1.85.
- Edition 2024 tightens defaults that matter for this code base, for example `unsafe_op_in_unsafe_fn` and the narrower `impl Trait` capture rules.
- Pinning the development toolchain separates "what we build with" from "what we support", so new Clippy lints cannot break the quality gate without a reviewed commit.

## Consequences

- The quality gate runs an `msrv` step with `cargo +1.85 check --workspace --all-targets` (ADR-0003).
- Raising the MSRV requires a new ADR and a minor version bump once the crates are published.
- systemdesign.md section 16 now states MSRV 1.85.
