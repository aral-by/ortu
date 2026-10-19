# ADR-0004: Normalization data, dependencies and types

- **Status:** Accepted
- **Date:** 2026-10-19
- **Affects:** systemdesign.md sections 4.4, 6.1-6.3 and 18; algorithms 01-03; `ortu-normalize`

## Context

Implementing M1 surfaced five points where the design documents and a correct implementation disagreed:

1. The design listed `ortu-normalize` as having no dependencies, but NFC normalization (section 6.1) is not in the standard library.
2. The algorithm documents used `CompactString`, `SmallString` and `SmallVec` for per-word and per-reading data, before any measurement showed allocation to matter.
3. The rule "look words up by their flattened key as a fallback" made `ön` ("front") read as `on` (ten) and `uç` ("tip") as `üç` (three).
4. Word lists (number words, fillers, suffixes, letter names) were described per algorithm, with no single place to review the Turkish data.
5. Beam-search costs were described as real numbers, which makes ordering on ties platform- and order-dependent.

## Decision

1. `ortu-normalize` depends on `unicode-normalization` and nothing else at runtime. "Pure" means no I/O and no global state, not zero dependencies.
2. Standard `String` and `Vec` until the M7 benchmarks show a measurable gain from small-string or small-vector types.
3. The flattened fallback applies only to words written **without any** Turkish-specific letter (`ı`, `ğ`, `ş`, `ç`, `ö`, `ü`, circumflex vowels). STT that drops diacritics writes `uc`; a word written as `uç` was meant as `uç`.
4. All Turkish word lists of normalization live in one data-only module, `crates/ortu-normalize/src/lexicon.rs`, compiled in and versioned with the code. Tests enforce that every entry is in folded form and that keyed tables have no duplicates. Large curated datasets (name gazetteers, provinces, districts) will be data files in `crates/ortu-tr/data/`; adopter-specific vocabulary is supplied at runtime (`Canonicalizer::with_stems`).
5. Costs are integers in thousandths (`SPLIT_COST = 1000`, `FALLBACK_COST = 700`), and the beam uses a stable sort, so the lattice is fully deterministic. Runs longer than 64 number words are cut into consecutive runs to bound the search on adversarial input.

## Rationale

- A dependency-free NFC implementation would duplicate Unicode tables that the ecosystem already maintains and audits; `unicode-normalization` is MIT OR Apache-2.0 and passes the cargo-deny policy.
- Optimizing storage before measuring adds API surface and dependencies for an unknown gain.
- Restricting the fallback to plain-ASCII words removes the false readings without losing the case the fallback exists for.
- Separating data from rules lets a reviewer check the Turkish lists without reading the algorithms, and the tests make the "dictionaries are folded at build time" rule from algorithm 01 impossible to break silently.
- Deterministic output is a precondition for snapshot tests and reproducible evaluation.

## Consequences

- systemdesign.md and algorithms 01-03 are updated to match.
- The M7 benchmarks decide whether storage types change; such a change needs a new ADR.
- Changing a Turkish word list is a change to `lexicon.rs` only, covered by its tests and the lattice snapshots.
