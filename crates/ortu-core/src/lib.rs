//! Core of ortu: the data model and the detection pipeline that every other
//! crate plugs into.
//!
//! - data model, spans and [`SecretValue`] (docs/systemdesign.md, section 5)
//! - the [`Detector`] and [`Validator`] traits (section 7.1)
//! - context cues (section 7.3, algorithm 10)
//! - the explainable score model (section 8.1, algorithm 10)
//! - conflict resolution (section 8.2, algorithm 11)
//! - the decision policy and `ortu.toml` (section 8.3)
//! - [`Analyzer`], which runs one turn through all of the above
//! - audit events and the [`AuditSink`] trait (section 12)
//!
//! The crate performs no I/O and holds no global state: configuration is
//! passed in as strings.

mod analyze;
mod audit;
mod cue;
mod detect;
mod entity;
mod model;
mod policy;
mod resolve;
mod score;
mod secret;

use std::fmt;

pub use analyze::{Analyzer, Suppressed, TurnAnalysis};
pub use audit::{AuditAction, AuditError, AuditEvent, AuditSink, DiscardAuditSink};
pub use cue::{CueIndex, CuePattern, CueRule};
pub use detect::{DetectContext, Detector, SessionHints, Validator};
pub use entity::{
    Candidate, CueHit, Entity, EntityId, EntityKind, Evidence, FailureReason, ValidatorOutcome,
};
pub use model::{Channel, SourceSpan, SpanSnap, Speaker, TimingQuality, Transcript, Turn, Word};
pub use policy::{Decision, FailMode, KindPolicy, Mode, Policy, TokenScope, Transform};
pub use resolve::{Resolution, ResolveInput, resolve};
pub use score::{ScoreBreakdown, Weights, score};
pub use secret::SecretValue;

/// A configuration document could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    message: String,
}

impl ConfigError {
    /// An error with the given message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn from_toml(error: &toml::de::Error) -> Self {
        Self::new(error.message().to_owned())
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid configuration: {}", self.message)
    }
}

impl std::error::Error for ConfigError {}
