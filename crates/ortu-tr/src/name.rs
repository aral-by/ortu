//! Person names, cue-based
//! ([algorithm 09](https://github.com/aral-by/ortu/blob/main/docs/algorithms/09-name-and-address-detection.md)).
//!
//! M2 detects names only where Turkish speech marks them: an introducer
//! (`adım`, `ismim`, `soyadım`, `adım soyadım`), an honorific (`Ahmet Bey`),
//! or an answer to an assistant asking for a name. Gazetteers, `ben X` and
//! capitalization on written channels arrive in M4.

use std::ops::Range;

use ortu_core::{
    Candidate, CueHit, DetectContext, Detector, EntityKind, SecretValue, Speaker, ValidatorOutcome,
};
use ortu_normalize::{NormWord, WordRole};

use crate::data::{
    FULL_NAME_INTRODUCERS, HONORIFICS, NAME_INTRODUCERS, NOT_NAMES, SURNAME_INTRODUCERS,
    VERB_ENDINGS,
};

/// Weight of an introducer or honorific cue (`w_cue`).
const CUE_BOOST: f32 = 2.0;

/// The longest answer treated as "just a name" after an assistant asks for one.
const MAX_ANSWER_WORDS: usize = 4;

/// [`Detector`] for [`EntityKind::PersonName`], [`EntityKind::PersonSurname`]
/// and [`EntityKind::FullName`].
#[derive(Debug, Clone, Copy, Default)]
pub struct NameDetector;

struct Turn<'a> {
    ctx: &'a DetectContext<'a>,
}

impl Turn<'_> {
    fn words(&self) -> &[NormWord] {
        &self.ctx.normalized.words
    }

    fn folded(&self, i: usize) -> Option<&str> {
        self.words().get(i).map(|w| w.folded.as_str())
    }

    /// Whether word `i` can be (part of) a name.
    fn is_name_like(&self, i: usize) -> bool {
        let Some(word) = self.words().get(i) else {
            return false;
        };
        let index = u32::try_from(i).unwrap_or(u32::MAX);
        let content = self.ctx.normalized.roles.get(i) == Some(&WordRole::Content);
        let in_number = self
            .ctx
            .normalized
            .lattices
            .iter()
            .any(|lattice| lattice.words.contains(&index));
        let folded = word.folded.as_str();
        content
            && !in_number
            && folded.chars().count() >= 2
            && folded.chars().all(char::is_alphabetic)
            && !NOT_NAMES.contains(&folded)
            && !HONORIFICS.contains(&folded)
            && !VERB_ENDINGS.iter().any(|ending| folded.ends_with(ending))
            && !self
                .ctx
                .hints
                .agent_names
                .iter()
                .any(|agent| agent == folded)
    }

    /// Up to `max` consecutive name-like words, starting at `start` after any
    /// fillers or interruptions (`adım ııı Barış`).
    fn name_words(&self, start: usize, max: usize) -> Range<usize> {
        let roles = &self.ctx.normalized.roles;
        let mut begin = start;
        while matches!(
            roles.get(begin),
            Some(WordRole::Filler | WordRole::Interruption)
        ) {
            begin += 1;
        }
        let mut end = begin;
        while end < begin + max && self.is_name_like(end) {
            end += 1;
            // Punctuation in the source ends a name: `ismim Can, borcumu ...`.
            if self.ends_with_punctuation(end - 1) {
                break;
            }
        }
        begin..end
    }

    fn ends_with_punctuation(&self, i: usize) -> bool {
        self.ctx
            .turn
            .words
            .get(i)
            .and_then(|word| word.text.chars().last())
            .is_some_and(|c| matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '…'))
    }

    fn candidate(
        &self,
        kind: EntityKind,
        words: Range<usize>,
        cue: &str,
        distance: u16,
    ) -> Candidate {
        let value = self.words()[words.clone()]
            .iter()
            .map(|w| w.folded.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let start = u32::try_from(words.start).unwrap_or(u32::MAX);
        let end = u32::try_from(words.end).unwrap_or(u32::MAX);
        Candidate {
            kind,
            detector: "tr.name",
            words: start..end,
            value: SecretValue::new(value),
            validator: ValidatorOutcome::NotApplicable,
            prior: 1.0,
            trace: None,
            cues: vec![CueHit {
                pattern: cue.to_owned(),
                distance,
                boost: CUE_BOOST,
                cross_turn: false,
            }],
            score_floor: None,
        }
    }
}

fn kind_for(words: &Range<usize>) -> EntityKind {
    if words.len() >= 2 {
        EntityKind::FullName
    } else {
        EntityKind::PersonName
    }
}

impl Detector for NameDetector {
    fn id(&self) -> &'static str {
        "tr.name"
    }

    fn kinds(&self) -> &'static [EntityKind] {
        &[
            EntityKind::PersonName,
            EntityKind::PersonSurname,
            EntityKind::FullName,
        ]
    }

    fn detect(&self, ctx: &DetectContext<'_>, out: &mut Vec<Candidate>) {
        let turn = Turn { ctx };
        let count = turn.words().len();
        let before = out.len();
        let mut i = 0;
        while i < count {
            let full = FULL_NAME_INTRODUCERS.iter().find(|phrase| {
                phrase
                    .iter()
                    .enumerate()
                    .all(|(k, expected)| turn.folded(i + k) == Some(*expected))
            });
            if let Some(phrase) = full {
                let words = turn.name_words(i + phrase.len(), 2);
                if !words.is_empty() {
                    out.push(turn.candidate(kind_for(&words), words.clone(), &phrase.join(" "), 0));
                    i = words.end;
                    continue;
                }
            }
            let word = turn.folded(i).unwrap_or_default();
            if NAME_INTRODUCERS.contains(&word) {
                let words = turn.name_words(i + 1, 2);
                if !words.is_empty() {
                    out.push(turn.candidate(kind_for(&words), words.clone(), word, 0));
                    i = words.end;
                    continue;
                }
            }
            if SURNAME_INTRODUCERS.contains(&word) {
                let words = turn.name_words(i + 1, 1);
                if !words.is_empty() {
                    out.push(turn.candidate(EntityKind::PersonSurname, words.clone(), word, 0));
                    i = words.end;
                    continue;
                }
            }
            if HONORIFICS.contains(&word) && i > 0 && turn.is_name_like(i - 1) {
                out.push(turn.candidate(EntityKind::PersonName, i - 1..i, word, 0));
            }
            i += 1;
        }

        // An assistant asked for a name and the caller answered with one.
        let asked = matches!(
            ctx.hints.expecting,
            Some(EntityKind::PersonName | EntityKind::FullName)
        );
        if asked && out.len() == before && ctx.turn.speaker == Speaker::Caller {
            let content: Vec<usize> = (0..count)
                .filter(|&k| ctx.normalized.roles.get(k) == Some(&WordRole::Content))
                .collect();
            if let Some(&first) = content.first() {
                let words = turn.name_words(first, 2);
                if !words.is_empty() && content.len() <= MAX_ANSWER_WORDS {
                    let distance = u16::try_from(first).unwrap_or(u16::MAX);
                    out.push(turn.candidate(
                        kind_for(&words),
                        words,
                        "<previous assistant turn>",
                        distance,
                    ));
                }
            }
        }
    }
}
