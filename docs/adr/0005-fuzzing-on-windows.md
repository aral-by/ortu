# ADR-0005: Randomized stress tests where coverage-guided fuzzing is unavailable

- **Status:** Accepted
- **Date:** 2026-10-19
- **Affects:** systemdesign.md sections 16 and 17 (M1 acceptance); `fuzz/`; `crates/ortu-normalize/tests/stress.rs`; xtask

## Context

M1 requires "no panic in 1M fuzz iterations". The fuzz targets in `fuzz/` (`normalize`, `parse_spoken_number`) use cargo-fuzz and libFuzzer. On Windows with the MSVC toolchain they fail to link: the SanitizerCoverage PC table symbols (`__start___sancov_pcs`, `__stop___sancov_pcs`) are not provided, with or without AddressSanitizer. The maintainer's only machine runs Windows, there is no hosted CI (ADR-0003), and no Linux environment is set up.

## Options considered

1. **Wait for a Linux environment.** Blocks M1 on infrastructure.
2. **Run cargo-fuzz in a Docker container.** Works, but requires starting Docker Desktop and building cargo-fuzz in the container for every session.
3. **Add a seeded, randomized stress test** that drives the same input generators and checks the same invariants as the fuzz targets, without coverage feedback. Keep the cargo-fuzz targets for Linux and macOS.

## Decision

Option 3, with the cargo-fuzz targets kept as they are. `cargo xtask fuzz [ITERATIONS]` runs the stress test (`crates/ortu-normalize/tests/stress.rs`, ignored in the normal test run because of its length). `ORTU_STRESS_SEED` reproduces a failure.

The M1 acceptance criterion is stated precisely: **1M randomized iterations without a panic or invariant violation**. It does not claim coverage-guided fuzzing.

## Rationale

- Random inputs cover the panic and invariant classes the fuzz targets check (offsets, lengths, priors, idempotent folding), on every platform and in under a minute.
- On its first run the stress test found a real defect: `turkish_fold` was not idempotent for `İ` followed by U+0308, because lowercasing created a sequence that NFC composes. The fix and a regression test were added before M1 closed.
- Coverage guidance still matters for deeper paths; keeping the cargo-fuzz targets means that work is ready the moment a Linux or macOS environment is available.

## Consequences

- systemdesign.md section 17 states the M1 criterion as above; section 16 lists both mechanisms.
- When a Linux or macOS environment becomes available (a contributor, Docker, or hosted CI under a new ADR), run `cargo +nightly fuzz run normalize` and `cargo +nightly fuzz run parse_spoken_number` and record the result.
- Later milestones add their targets (`iban_validate`, `redact_pipeline`, `rehydrate`) to both the stress test and `fuzz/`.
