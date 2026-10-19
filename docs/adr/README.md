# Architecture Decision Records

An ADR records one decision that deviates from, refines or fills a gap in [systemdesign.md](../systemdesign.md). The design document's decision log (section 19) holds the decisions made before implementation; ADRs hold everything decided after.

## Process

1. Copy [0000-template.md](0000-template.md) to the next free number with a short kebab-case title.
2. Open a pull request with status `Proposed`.
3. On merge, set the status to `Accepted` and update `systemdesign.md` in the same pull request if the decision changes it.
4. ADRs are never deleted. A reversed decision gets a new ADR, and the old one is marked `Superseded by ADR-NNNN`.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-rust-edition-and-msrv.md) | Rust edition 2024 and MSRV 1.85 | Accepted |
| [0002](0002-cli-exit-codes.md) | CLI exit codes | Accepted |
