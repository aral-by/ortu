//! Unfinished values: digits after a cue or a question that do not yet form a
//! TCKN, IBAN or phone number
//! ([algorithm 12](https://github.com/aral-by/ortu/blob/main/docs/algorithms/12-cross-turn-assembly.md)).
//!
//! The caller may finish the value in a later turn, or never. Either way the
//! digits already said are personal data, so they are masked as
//! `[KIND_INCOMPLETE:...]` in both modes; Realtime additionally keeps a run
//! open when the fragment ends the turn.

use std::ops::Range;

use ortu_core::{Decision, EntityKind, TurnAnalysis, ValidatorOutcome};
use ortu_normalize::DigitLattice;
use ortu_tr::Assembled;
use zeroize::Zeroizing;

use crate::engine::Inner;
use crate::mask::{Found, WordSpan};

/// Detector name recorded for assembled and unfinished values.
pub(crate) const ASSEMBLY: &str = "stream.assembly";

/// Score of an assembled value that passed its validator.
const ASSEMBLED_SCORE: f32 = 1.0;

/// Score of an unfinished value: the partial-match floor of section 7.2.
const PARTIAL_SCORE: f32 = 0.7;

/// A single digit after a cue (`TC'mi 2 gün içinde`) says too little to be a
/// fragment of an identifier.
const MIN_FRAGMENT_DIGITS: usize = 2;

/// Kinds tried, most specific first, when a cue rather than a question
/// announces the value.
const ASSEMBLED_KINDS: [EntityKind; 3] = [EntityKind::Iban, EntityKind::Tckn, EntityKind::Phone];

/// An unfinished value found in a lattice.
pub(crate) struct Fragment {
    pub(crate) kind: EntityKind,
    pub(crate) words: Range<u32>,
    pub(crate) digits: Zeroizing<String>,
    pub(crate) assembled: Assembled,
}

/// The lattice's digits as an unfinished value, if a cue in reach or the
/// expected kind says what they are, they do not complete it, and no
/// validated entity already covers them (`covered`).
pub(crate) fn fragment(
    analysis: &TurnAnalysis,
    lattice: &DigitLattice,
    expecting: Option<EntityKind>,
    covered: impl Fn(&Range<u32>) -> bool,
) -> Option<Fragment> {
    let best = lattice.readings.first()?;
    if best.digits.len() < MIN_FRAGMENT_DIGITS || covered(&lattice.words) {
        return None;
    }
    let kind = expecting
        .filter(|kind| ortu_tr::assembles(*kind))
        .or_else(|| {
            ASSEMBLED_KINDS
                .into_iter()
                .find(|kind| !analysis.cues.hits(*kind, &lattice.words).is_empty())
        })?;
    let assembled = ortu_tr::assemble(kind, &best.digits)?;
    assembled.needs_more().then(|| Fragment {
        kind,
        words: lattice.words.clone(),
        digits: Zeroizing::new(best.digits.clone()),
        assembled,
    })
}

/// An assembled or unfinished value as an entity: 1.0 when it passes its
/// validator, the partial floor otherwise.
pub(crate) fn assembled(
    inner: &Inner,
    kind: EntityKind,
    parts: Vec<WordSpan>,
    assembled: Assembled,
    cross_turn: bool,
) -> Found {
    let score = if assembled.outcome == ValidatorOutcome::Passed {
        ASSEMBLED_SCORE
    } else {
        PARTIAL_SCORE
    };
    Found {
        kind,
        cross_turn: cross_turn || parts.len() > 1,
        parts,
        value: assembled.value,
        validator: assembled.outcome,
        score,
        review: inner.policy().decide(score) == Decision::MaskAndReview,
        detector: ASSEMBLY,
    }
}

/// Whether two word ranges share a word.
pub(crate) fn overlap(a: &Range<u32>, b: &Range<u32>) -> bool {
    a.start < b.end && b.start < a.end
}
