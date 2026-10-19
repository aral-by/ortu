//! Detector and validator traits (docs/systemdesign.md, section 7.1).

use std::ops::Range;

use ortu_normalize::NormalizedTurn;

use crate::cue::CueIndex;
use crate::entity::{Candidate, EntityKind, ValidatorOutcome};
use crate::model::Turn;

/// State carried into a turn from earlier turns of the same session. In M2 it
/// is supplied by the caller; the session API (M3) maintains it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionHints {
    /// The previous assistant turn asked for this kind.
    pub expecting: Option<EntityKind>,
    /// Folded names of the agent or bot, never masked as person names.
    pub agent_names: Vec<String>,
}

/// Everything a detector may read. Detectors are pure functions of it.
#[derive(Debug, Clone, Copy)]
pub struct DetectContext<'a> {
    /// The source turn.
    pub turn: &'a Turn,
    /// Its normalization (canonical words, roles, lattices, spelled runs).
    pub normalized: &'a NormalizedTurn,
    /// Cue occurrences in the turn.
    pub cues: &'a CueIndex,
    /// Session state from earlier turns.
    pub hints: &'a SessionHints,
}

impl DetectContext<'_> {
    /// Whether a cue for `kind` supports the given words, in this turn or from
    /// the previous assistant turn.
    #[must_use]
    pub fn has_cue(&self, kind: EntityKind, words: &Range<u32>) -> bool {
        self.hints.expecting == Some(kind) || !self.cues.hits(kind, words).is_empty()
    }
}

/// Proposes candidates of one or more kinds.
pub trait Detector: Send + Sync {
    /// Stable identifier, recorded in the evidence (`tr.tckn`).
    fn id(&self) -> &'static str;

    /// The kinds this detector can propose.
    fn kinds(&self) -> &'static [EntityKind];

    /// Folded stems the detector's patterns rely on, registered for suffix
    /// splitting.
    fn stems(&self) -> &'static [&'static str] {
        &[]
    }

    /// Appends candidates found in the turn.
    fn detect(&self, ctx: &DetectContext<'_>, out: &mut Vec<Candidate>);
}

/// Checks the structure of a value.
pub trait Validator: Send + Sync {
    /// Validates `value` as `kind`.
    fn validate(&self, kind: EntityKind, value: &str) -> ValidatorOutcome;
}
