//! The explainable score model
//! ([algorithm 10](https://github.com/aral-by/ortu/blob/main/docs/algorithms/10-cue-scoring.md)).

use serde::Deserialize;

use crate::ConfigError;
use crate::entity::{Candidate, CueHit, ValidatorOutcome};

/// Weights of the logistic score model. The defaults are the hand-set initial
/// values; `config/weights.toml` holds the same values, and the M7
/// calibration replaces them.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Weights {
    /// Base term: a bare pattern match is not enough on its own.
    pub detector: f32,
    /// Added when the validator passed.
    pub validator: f32,
    /// Subtracted when the length was right but the checksum or structure failed.
    pub invalid: f32,
    /// Multiplies the lattice prior.
    pub prior: f32,
    /// Multiplies the mean STT confidence.
    pub stt: f32,
    /// Added when the value was assembled across turns.
    pub cross: f32,
    /// Distance, in words, over which a cue's influence decays by `e`.
    pub tau: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            detector: -1.0,
            validator: 3.0,
            invalid: 2.0,
            prior: 1.0,
            stt: 0.5,
            cross: 0.5,
            tau: 6.0,
        }
    }
}

impl Weights {
    /// Parses the `[weights]` table of a TOML document.
    ///
    /// # Errors
    ///
    /// Returns an error if the document is not valid TOML, a weight is
    /// missing or unknown, or `tau` is not positive.
    pub fn from_toml_str(document: &str) -> Result<Self, ConfigError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            weights: Weights,
        }
        let file: File =
            toml::from_str(document).map_err(|error| ConfigError::from_toml(&error))?;
        if file.weights.tau <= 0.0 {
            return Err(ConfigError::new("weights.tau must be positive"));
        }
        Ok(file.weights)
    }
}

/// A score, term by term, for the audit trail. The terms sum to `logit`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScoreBreakdown {
    /// `w_detector`.
    pub detector: f32,
    /// `+w_validator`, `-w_invalid` or 0.
    pub validator: f32,
    /// Sum of `w_cue * exp(-distance / tau)` over the cue hits.
    pub cues: f32,
    /// `w_prior * lattice prior`.
    pub prior: f32,
    /// `w_stt * mean STT confidence`; 0 when STT gave no confidence.
    pub stt: f32,
    /// `w_cross` when assembled across turns.
    pub cross: f32,
    /// Sum of the terms.
    pub logit: f32,
    /// `sigmoid(logit)`, raised to the policy floor when one applies.
    pub score: f32,
    /// Whether a partial-match floor raised the score.
    pub floor_applied: bool,
}

/// Scores a candidate (section 8.1).
#[must_use]
pub fn score(
    candidate: &Candidate,
    hits: &[CueHit],
    stt_confidence: Option<f32>,
    assembled_across_turns: bool,
    weights: &Weights,
) -> ScoreBreakdown {
    let validator = match candidate.validator {
        ValidatorOutcome::Passed => weights.validator,
        outcome if outcome.is_exact_length_failure() => -weights.invalid,
        _ => 0.0,
    };
    let cues = hits
        .iter()
        .map(|hit| hit.boost * (-f32::from(hit.distance) / weights.tau).exp())
        .sum::<f32>();
    let prior = weights.prior * candidate.prior;
    let stt = stt_confidence.map_or(0.0, |confidence| weights.stt * confidence);
    let cross = if assembled_across_turns {
        weights.cross
    } else {
        0.0
    };
    let logit = weights.detector + validator + cues + prior + stt + cross;
    let raw = 1.0 / (1.0 + (-logit).exp());
    let (score, floor_applied) = match candidate.score_floor {
        Some(floor) if floor > raw => (floor, true),
        _ => (raw, false),
    };
    ScoreBreakdown {
        detector: weights.detector,
        validator,
        cues,
        prior,
        stt,
        cross,
        logit,
        score,
        floor_applied,
    }
}

#[cfg(test)]
mod tests {
    use super::{Weights, score};
    use crate::entity::{Candidate, CueHit, EntityKind, FailureReason, ValidatorOutcome};
    use crate::secret::SecretValue;

    fn candidate(validator: ValidatorOutcome, prior: f32, floor: Option<f32>) -> Candidate {
        Candidate {
            kind: EntityKind::Tckn,
            detector: "test",
            words: 0..1,
            value: SecretValue::new("x"),
            validator,
            prior,
            trace: None,
            cues: Vec::new(),
            score_floor: floor,
        }
    }

    fn cue(distance: u16) -> CueHit {
        CueHit {
            pattern: "tc".into(),
            distance,
            boost: 2.0,
            cross_turn: false,
        }
    }

    #[test]
    fn worked_examples_from_algorithm_10() {
        let weights = Weights::default();

        let valid = score(
            &candidate(ValidatorOutcome::Passed, 1.0, None),
            &[cue(2)],
            Some(0.9),
            false,
            &weights,
        );
        assert!((valid.logit - 4.883).abs() < 1e-3, "{valid:?}");
        assert!((valid.score - 0.992).abs() < 1e-3);

        let failed = ValidatorOutcome::Failed(FailureReason::Checksum);
        let invalid = score(
            &candidate(failed, 0.6, None),
            &[],
            Some(0.9),
            false,
            &weights,
        );
        assert!((invalid.logit + 1.95).abs() < 1e-3, "{invalid:?}");
        assert!((invalid.score - 0.125).abs() < 1e-3);

        let short = ValidatorOutcome::Failed(FailureReason::TooShort);
        let partial = score(
            &candidate(short, 1.0, Some(0.7)),
            &[],
            None,
            false,
            &weights,
        );
        assert!(partial.floor_applied);
        assert!((partial.score - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn terms_sum_to_the_logit() {
        let breakdown = score(
            &candidate(ValidatorOutcome::Passed, 0.37, None),
            &[cue(0), cue(5)],
            Some(0.5),
            true,
            &Weights::default(),
        );
        let sum = breakdown.detector
            + breakdown.validator
            + breakdown.cues
            + breakdown.prior
            + breakdown.stt
            + breakdown.cross;
        assert!((sum - breakdown.logit).abs() < 1e-6);
    }

    #[test]
    fn adding_evidence_never_lowers_the_score() {
        let weights = Weights::default();
        let base = candidate(ValidatorOutcome::NotApplicable, 1.0, None);
        let without = score(&base, &[], None, false, &weights).score;
        let with_cue = score(&base, &[cue(3)], None, false, &weights).score;
        let passed = score(
            &candidate(ValidatorOutcome::Passed, 1.0, None),
            &[],
            None,
            false,
            &weights,
        )
        .score;
        assert!(with_cue > without);
        assert!(passed > without);
    }
}
