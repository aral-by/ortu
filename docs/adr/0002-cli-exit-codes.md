# ADR-0002: CLI exit codes

- **Status:** Accepted
- **Date:** 2026-10-19
- **Affects:** systemdesign.md section 13.2, `ortu-cli`

## Context

The design document defined exit codes `0` (success), `2` (PII found, for `scan --fail-on-pii` in CI) and `3` (fail-closed). The CLI is built on `clap`, which exits with `2` on usage errors, following the long-standing Unix convention. With the original table, a CI job running `ortu scan --fail-on-pii` with a mistyped flag would report "PII found", and a pipeline that treats `2` as a finding would block merges for the wrong reason. Runtime errors also had no code.

## Options considered

1. **Override clap's usage exit code.** Keeps the original table but breaks the convention that scripts and shells rely on.
2. **Move "PII found" to a dedicated code.** Keeps clap's behavior and gives every outcome a distinct code.

## Decision

| Code | Meaning |
|---|---|
| `0` | Success; for `scan --fail-on-pii`, no PII found |
| `1` | Runtime error (I/O, configuration, invalid input file) |
| `2` | Usage error (reported by clap) |
| `3` | Fail-closed: processing stopped and no output was released |
| `4` | `scan --fail-on-pii` found PII |

## Rationale

Distinct codes let CI distinguish "the repository contains PII" from "the tool was invoked incorrectly" or "the tool failed". Keeping `2` for usage errors matches every other Unix tool that adopters already script against.

## Consequences

- systemdesign.md section 13.2 is updated to this table.
- The M0 smoke test asserts that missing arguments exit with `2`.
- Every subcommand added later maps its errors onto this table; new codes require a new ADR.
