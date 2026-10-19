//! One turn through the detection pipeline: normalize, detect, score,
//! resolve, decide (docs/systemdesign.md, section 4.1, stages 1-5, 7 and 8).
//! Cross-turn assembly (stage 6) and transforms (stage 9) belong to the
//! session API (M3).

use std::collections::BTreeSet;
use std::ops::Range;

use ortu_normalize::{Canonicalizer, LatticeConfig, NormalizeConfig, NormalizedTurn, normalize};

use crate::cue::{CueIndex, CuePattern, CueRule};
use crate::detect::{DetectContext, Detector, SessionHints};
use crate::entity::{Candidate, Entity, EntityId, EntityKind, Evidence, ValidatorOutcome};
use crate::model::{SourceSpan, Turn};
use crate::policy::{Decision, Policy};
use crate::resolve::{ResolveInput, resolve};
use crate::score::{ScoreBreakdown, Weights, score};

/// A candidate that lost a conflict to an entity, kept for the audit trail.
#[derive(Debug, Clone, PartialEq)]
pub struct Suppressed {
    /// The candidate's kind.
    pub kind: EntityKind,
    /// The detector that proposed it.
    pub detector: &'static str,
    /// Word indices covered.
    pub words: Range<u32>,
    /// Its score.
    pub score: f32,
    /// The entity it lost to.
    pub suppressed_by: EntityId,
}

/// The result of analyzing one turn.
#[derive(Debug, Clone)]
pub struct TurnAnalysis {
    /// The normalized turn.
    pub normalized: NormalizedTurn,
    /// Masked entities, in order of their first word.
    pub entities: Vec<Entity>,
    /// Every masked word: the union of all qualifying candidates (algorithm 11).
    pub masked_words: BTreeSet<u32>,
    /// Candidates that lost a conflict.
    pub suppressed: Vec<Suppressed>,
    /// For assistant turns: the kind the assistant asked for, which the
    /// caller's next turn will probably contain.
    pub expecting_next: Option<EntityKind>,
}

/// Runs detectors over turns and decides what to mask.
pub struct Analyzer {
    normalize: NormalizeConfig,
    detectors: Vec<Box<dyn Detector>>,
    cue_rules: Vec<CueRule>,
    weights: Weights,
    policy: Policy,
}

impl Analyzer {
    /// Builds an analyzer. Every cue pattern word and every detector stem is
    /// registered for suffix splitting, so `numaram` matches the cue `numara`.
    #[must_use]
    pub fn new(
        detectors: Vec<Box<dyn Detector>>,
        cue_rules: Vec<CueRule>,
        weights: Weights,
        policy: Policy,
    ) -> Self {
        let stems: Vec<&str> = cue_rules
            .iter()
            .flat_map(|rule| rule.patterns.iter().flat_map(CuePattern::stems))
            .chain(detectors.iter().flat_map(|d| d.stems().iter().copied()))
            .collect();
        let normalize = NormalizeConfig {
            canonicalizer: Canonicalizer::with_stems(stems),
            lattice: LatticeConfig::default(),
        };
        Self {
            normalize,
            detectors,
            cue_rules,
            weights,
            policy,
        }
    }

    /// The policy in use.
    #[must_use]
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Analyzes one turn. Entity identifiers start at `first_id`.
    #[must_use]
    pub fn analyze_turn(&self, turn: &Turn, hints: &SessionHints, first_id: u32) -> TurnAnalysis {
        let raw: Vec<&str> = turn.words.iter().map(|word| word.text.as_str()).collect();
        let normalized = normalize(&raw, &self.normalize);
        let cues = CueIndex::build(&self.cue_rules, &normalized.words);
        let ctx = DetectContext {
            turn,
            normalized: &normalized,
            cues: &cues,
            hints,
        };

        let mut candidates = Vec::new();
        for detector in &self.detectors {
            detector.detect(&ctx, &mut candidates);
        }
        let scored: Vec<(Candidate, Evidence)> = candidates
            .into_iter()
            .map(|candidate| {
                let evidence = self.evidence(&candidate, &ctx);
                (candidate, evidence)
            })
            .collect();

        let inputs: Vec<ResolveInput> = scored
            .iter()
            .map(|(candidate, evidence)| ResolveInput {
                words: candidate.words.clone(),
                kind: candidate.kind,
                validated: candidate.validator == ValidatorOutcome::Passed,
                score: evidence.score.score,
            })
            .collect();
        let resolution = resolve(&inputs, self.policy.review_threshold);

        let mut entities = Vec::with_capacity(resolution.winners.len());
        for (offset, &index) in resolution.winners.iter().enumerate() {
            let (candidate, evidence) = &scored[index];
            let id = EntityId(first_id.saturating_add(u32::try_from(offset).unwrap_or(u32::MAX)));
            entities.push(Entity {
                id,
                kind: candidate.kind,
                span: span_of(turn.index, &normalized, &candidate.words),
                words: candidate.words.clone(),
                score: evidence.score.score,
                review: self.policy.decide(evidence.score.score) == Decision::MaskAndReview,
                evidence: evidence.clone(),
                value: candidate.value.clone(),
            });
        }
        let suppressed = resolution
            .suppressed
            .iter()
            .filter_map(|&(loser, winner)| {
                let position = resolution.winners.iter().position(|&w| w == winner)?;
                let (candidate, evidence) = &scored[loser];
                Some(Suppressed {
                    kind: candidate.kind,
                    detector: candidate.detector,
                    words: candidate.words.clone(),
                    score: evidence.score.score,
                    suppressed_by: entities[position].id,
                })
            })
            .collect();

        TurnAnalysis {
            expecting_next: turn
                .speaker
                .is_assistant()
                .then(|| cues.first_cross_turn_kind())
                .flatten(),
            normalized,
            entities,
            masked_words: resolution.masked_words,
            suppressed,
        }
    }

    fn evidence(&self, candidate: &Candidate, ctx: &DetectContext<'_>) -> Evidence {
        let mut cues = ctx.cues.hits(candidate.kind, &candidate.words);
        cues.extend(candidate.cues.iter().cloned());
        let cross_turn = ctx.hints.expecting == Some(candidate.kind)
            && self
                .cue_rules
                .iter()
                .any(|rule| rule.kind == candidate.kind && rule.cross_turn);
        if let Some(rule) = self
            .cue_rules
            .iter()
            .find(|rule| cross_turn && rule.kind == candidate.kind)
        {
            cues.push(crate::entity::CueHit {
                pattern: "<previous assistant turn>".to_owned(),
                distance: rule.window_before,
                boost: rule.boost,
                cross_turn: true,
            });
        }
        let confidence = ctx.turn.mean_confidence(candidate.words.clone());
        let breakdown: ScoreBreakdown = score(candidate, &cues, confidence, false, &self.weights);
        Evidence {
            detector: candidate.detector,
            validator: candidate.validator,
            cues,
            normalization: candidate.trace.clone(),
            cross_turn,
            score: breakdown,
        }
    }
}

/// The source span of a word range: from the first word's first byte to the
/// last word's last byte (Speech snaps to whole words, section 5.2).
fn span_of(turn: u32, normalized: &NormalizedTurn, words: &Range<u32>) -> SourceSpan {
    let start = usize::try_from(words.start).unwrap_or(usize::MAX);
    let end = usize::try_from(words.end).unwrap_or(usize::MAX);
    let first = normalized.words.get(start).map_or(0, |w| w.bytes.start);
    let last = end
        .checked_sub(1)
        .and_then(|i| normalized.words.get(i))
        .map_or(first, |w| w.bytes.end);
    SourceSpan::within_turn(turn, first..last)
}
