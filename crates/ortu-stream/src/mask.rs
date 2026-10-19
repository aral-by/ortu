//! Transforms and masked output (docs/systemdesign.md, sections 5.5 and
//! 10.1): what replaces a value, and the text and reports that leave ortu.
//! Nothing here contains a raw value.

use std::collections::BTreeSet;
use std::ops::Range;

use ortu_core::{
    Entity, EntityId, EntityKind, SecretValue, SourceSpan, Speaker, Transform, Turn,
    ValidatorOutcome,
};
use ortu_crypto::Tokenizer;

use crate::engine::Inner;
use crate::map::{Entry, SessionMap};

/// Words of one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSpan {
    /// The turn's `index`.
    pub turn: u32,
    /// Word indices within the turn.
    pub words: Range<u32>,
}

/// A masked entity, without its value.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityReport {
    /// Unique within the session or batch.
    pub id: EntityId,
    /// The kind.
    pub kind: EntityKind,
    /// The words it covers, one part per turn; several parts when the caller
    /// spread the value over turns.
    pub parts: Vec<WordSpan>,
    /// Source coordinates from its first to its last byte.
    pub span: SourceSpan,
    /// Score in `0.0..=1.0`.
    pub score: f32,
    /// Masked and flagged for review.
    pub review: bool,
    /// The validator outcome.
    pub validator: ValidatorOutcome,
    /// What replaced it in the text: a token, a redaction or a mask.
    pub replacement: String,
    /// The detector that found it (`tr.tckn`, `stream.assembly`).
    pub detector: &'static str,
    /// Evidence came from another turn: an assistant question, or digits the
    /// caller spoke in an earlier turn.
    pub cross_turn: bool,
}

/// A turn returned by `Session::push`, ready for the LLM.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskedTurn {
    /// The turn's `index`.
    pub index: u32,
    /// Who spoke.
    pub speaker: Speaker,
    /// The turn text with every masked span replaced.
    pub text: String,
    /// Entities masked in this turn.
    pub entities: Vec<EntityReport>,
}

/// One turn of the persistent transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedTurn {
    /// The turn's `index`.
    pub index: u32,
    /// Who spoke.
    pub speaker: Speaker,
    /// The turn text with every masked span replaced.
    pub text: String,
}

/// The `PostCall` output (section 9.1): what persistent systems store.
#[derive(Debug, Clone, PartialEq)]
pub struct PostCallResult {
    /// Every turn, in order.
    pub turns: Vec<RedactedTurn>,
    /// Every entity, in order of its first word. Audio regions (section 11)
    /// are derived from their parts.
    pub entities: Vec<EntityReport>,
}

/// An entity before its transform: what was found, where, and its value.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) kind: EntityKind,
    pub(crate) parts: Vec<WordSpan>,
    pub(crate) value: SecretValue,
    pub(crate) validator: ValidatorOutcome,
    pub(crate) score: f32,
    pub(crate) review: bool,
    pub(crate) detector: &'static str,
    pub(crate) cross_turn: bool,
}

impl Found {
    /// An entity the analyzer found in `turn`. Names keep the spelling of the
    /// source (`Ahmet Yılmaz`) for read-back and tools; their token is still
    /// computed over the folded form, so it does not change.
    pub(crate) fn detected(turn: &Turn, entity: Entity) -> Self {
        let value = if matches!(
            entity.kind,
            EntityKind::PersonName | EntityKind::PersonSurname | EntityKind::FullName
        ) {
            source_text(turn, &entity.words).map_or(entity.value, SecretValue::new)
        } else {
            entity.value
        };
        Self {
            kind: entity.kind,
            parts: vec![WordSpan {
                turn: turn.index,
                words: entity.words,
            }],
            value,
            validator: entity.evidence.validator,
            score: entity.score,
            review: entity.review,
            detector: entity.evidence.detector,
            cross_turn: entity.evidence.cross_turn,
        }
    }

    /// Whether any part covers a word of `words` in `turn`.
    pub(crate) fn overlaps(&self, turn: u32, words: &Range<u32>) -> bool {
        self.parts.iter().any(|part| {
            part.turn == turn && part.words.start < words.end && words.start < part.words.end
        })
    }

    fn first(&self) -> (u32, u32) {
        self.parts
            .first()
            .map_or((0, 0), |part| (part.turn, part.words.start))
    }
}

/// Applies transforms. Tokens are recorded in the session map when there is
/// one (Realtime and tool results), so that they can be resolved later.
pub(crate) struct Masker<'a> {
    pub(crate) inner: &'a Inner,
    pub(crate) tokenizer: &'a Tokenizer,
    pub(crate) map: Option<&'a mut SessionMap>,
}

impl Masker<'_> {
    /// The replacement for a value under the policy's transform for its kind.
    pub(crate) fn replacement(
        &mut self,
        kind: EntityKind,
        value: &SecretValue,
        validator: ValidatorOutcome,
    ) -> String {
        match self.inner.policy().transform_for(kind) {
            // `encrypt` is rejected when the engine is built (vault: M5).
            Transform::Token | Transform::Encrypt => {
                let token = self.tokenizer.token(kind, value, validator);
                if let Some(map) = self.map.as_deref_mut() {
                    map.insert(
                        &token,
                        Entry {
                            kind,
                            value: value.clone(),
                            outcome: validator,
                        },
                    );
                }
                token.to_string()
            }
            Transform::Redact => format!("[{}]", kind.label()),
            Transform::Mask => partial(value.expose_secret()),
        }
    }

    /// Transforms entities into reports, in order of their first word, with
    /// identifiers from `next_id` on.
    pub(crate) fn reports<'t>(
        &mut self,
        mut found: Vec<Found>,
        next_id: &mut u32,
        turn: impl Fn(u32) -> Option<&'t Turn>,
    ) -> Vec<EntityReport> {
        found.sort_by_key(Found::first);
        found
            .into_iter()
            .map(|f| {
                let replacement = self.replacement(f.kind, &f.value, f.validator);
                let id = EntityId(*next_id);
                *next_id = next_id.saturating_add(1);
                EntityReport {
                    id,
                    kind: f.kind,
                    span: span_of(&f.parts, &turn),
                    parts: f.parts,
                    score: f.score,
                    review: f.review,
                    validator: f.validator,
                    replacement,
                    detector: f.detector,
                    cross_turn: f.cross_turn,
                }
            })
            .collect()
    }
}

/// The source words of a range, joined by spaces, without edge punctuation
/// (`Yılmaz,` becomes `Yılmaz`).
fn source_text(turn: &Turn, words: &Range<u32>) -> Option<String> {
    let start = usize::try_from(words.start).ok()?;
    let end = usize::try_from(words.end).ok()?;
    let text = turn
        .words
        .get(start..end)?
        .iter()
        .map(|word| word.text.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!text.is_empty()).then_some(text)
}

/// `*******0146`: every character but the last four hidden; values of four
/// characters or fewer are hidden completely.
pub(crate) fn partial(value: &str) -> String {
    let count = value.chars().count();
    let visible = if count > 4 { 4 } else { 0 };
    value
        .chars()
        .enumerate()
        .map(|(i, c)| if i + visible < count { '*' } else { c })
        .collect()
}

/// The turn text with masked words replaced. Each maximal run of masked words
/// becomes the replacements of the entities inside it; a run with no entity
/// (impossible by construction) is redacted rather than released.
pub(crate) fn render(turn: &Turn, masked: &BTreeSet<u32>, reports: &[EntityReport]) -> String {
    let parts: Vec<(&Range<u32>, &str)> = reports
        .iter()
        .flat_map(|report| {
            report
                .parts
                .iter()
                .filter(|part| part.turn == turn.index)
                .map(|part| (&part.words, report.replacement.as_str()))
        })
        .collect();
    let is_masked = |i: u32| masked.contains(&i) || parts.iter().any(|(w, _)| w.contains(&i));
    let mut out: Vec<&str> = Vec::with_capacity(turn.words.len());
    let count = u32::try_from(turn.words.len()).unwrap_or(u32::MAX);
    let mut i = 0;
    while i < count {
        if !is_masked(i) {
            if let Some(word) = usize::try_from(i).ok().and_then(|k| turn.words.get(k)) {
                out.push(&word.text);
            }
            i += 1;
            continue;
        }
        let start = i;
        while i < count && is_masked(i) {
            i += 1;
        }
        let run = start..i;
        let before = out.len();
        for (words, replacement) in &parts {
            let inside = words.start < run.end && run.start < words.end;
            if inside && !out[before..].contains(replacement) {
                out.push(replacement);
            }
        }
        if out.len() == before {
            out.push("[REDACTED]");
        }
    }
    out.join(" ")
}

/// Byte range of a word range in the turn text (words joined by one space).
fn word_bytes(turn: &Turn, words: &Range<u32>) -> Range<u32> {
    let mut offset = 0usize;
    let mut start = 0usize;
    let mut end = 0usize;
    for (i, word) in (0u32..).zip(&turn.words) {
        if i == words.start {
            start = offset;
        }
        offset += word.text.len();
        if i + 1 == words.end {
            end = offset;
        }
        offset += 1;
    }
    let clamp = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    clamp(start)..clamp(end.max(start))
}

fn span_of<'t>(parts: &[WordSpan], turn: &impl Fn(u32) -> Option<&'t Turn>) -> SourceSpan {
    let bytes = |part: &WordSpan| turn(part.turn).map_or(0..0, |t| word_bytes(t, &part.words));
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) => SourceSpan {
            turn_start: first.turn,
            byte_start: bytes(first).start,
            turn_end: last.turn,
            byte_end: bytes(last).end,
        },
        _ => SourceSpan::within_turn(0, 0..0),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ortu_core::{EntityId, EntityKind, SourceSpan, Speaker, Turn, ValidatorOutcome};

    use super::{EntityReport, WordSpan, partial, render, word_bytes};

    fn report(words: std::ops::Range<u32>, replacement: &str) -> EntityReport {
        EntityReport {
            id: EntityId(0),
            kind: EntityKind::Tckn,
            parts: vec![WordSpan { turn: 0, words }],
            span: SourceSpan::within_turn(0, 0..0),
            score: 1.0,
            review: false,
            validator: ValidatorOutcome::Passed,
            replacement: replacement.to_owned(),
            detector: "test",
            cross_turn: false,
        }
    }

    #[test]
    fn partial_masks_keep_the_last_four_characters() {
        assert_eq!(partial("10000000146"), "*******0146");
        assert_eq!(partial("+905321234567"), "*********4567");
        assert_eq!(partial("1234"), "****");
    }

    #[test]
    fn masked_runs_become_their_replacements() {
        let turn = Turn::from_text(0, Speaker::Caller, "TC'm on bir iki tamam mı", 0, 1);
        let masked: BTreeSet<u32> = [1, 2, 3].into();
        let text = render(&turn, &masked, &[report(1..4, "[TCKN:00000000]")]);
        assert_eq!(text, "TC'm [TCKN:00000000] tamam mı");
        assert_eq!(render(&turn, &masked, &[]), "TC'm [REDACTED] tamam mı");
    }

    #[test]
    fn byte_ranges_match_the_joined_turn_text() {
        let turn = Turn::from_text(0, Speaker::Caller, "TC'm on bir", 0, 1);
        let text = turn.text();
        let bytes = word_bytes(&turn, &(1..3));
        assert_eq!(&text[bytes.start as usize..bytes.end as usize], "on bir");
    }
}
