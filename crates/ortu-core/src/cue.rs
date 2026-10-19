//! Context cues (docs/systemdesign.md, section 7.3; algorithm 10).
//!
//! Cue words are matched on the flattened stem of each word, so `IBAN'ım`,
//! `iban` and `ıban` all match the pattern `iban`. Cues only add evidence to a
//! candidate's score; unlike number words they never become values, so the
//! diacritic collisions that algorithm 01 guards against do not matter here.

use std::ops::Range;

use ortu_normalize::{NormWord, flatten, turkish_fold};

use crate::entity::{CueHit, EntityKind};

/// One cue pattern: one or more words, written in folded Turkish spelling. A
/// trailing `*` makes the last word a prefix (`ulaş*` matches
/// `ulaşabilirsiniz`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuePattern {
    text: String,
    folded: Vec<String>,
    keys: Vec<String>,
    prefix: bool,
}

impl CuePattern {
    /// Parses a pattern such as `kimlik`, `te ce` or `numara*`.
    #[must_use]
    pub fn parse(spec: &str) -> Self {
        let text = spec.trim();
        let (body, prefix) = match text.strip_suffix('*') {
            Some(body) => (body, true),
            None => (text, false),
        };
        let folded: Vec<String> = body.split_whitespace().map(turkish_fold).collect();
        let keys = folded.iter().map(|word| flatten(word)).collect();
        Self {
            text: text.to_owned(),
            folded,
            keys,
            prefix,
        }
    }

    /// The pattern as written.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The pattern's words in folded form, used as stems for suffix splitting
    /// (`numaram` = `numara` + `m`).
    pub fn stems(&self) -> impl Iterator<Item = &str> {
        self.folded.iter().map(String::as_str)
    }

    fn len(&self) -> usize {
        self.keys.len()
    }

    fn matches_at(&self, keys: &[String], start: usize) -> bool {
        let Some(window) = keys.get(start..start + self.len()) else {
            return false;
        };
        let last = self.len().saturating_sub(1);
        window
            .iter()
            .zip(&self.keys)
            .enumerate()
            .all(|(i, (key, expected))| {
                if self.prefix && i == last {
                    key.starts_with(expected.as_str())
                } else {
                    key == expected
                }
            })
    }
}

/// A rule that ties cue patterns to an entity kind.
#[derive(Debug, Clone, PartialEq)]
pub struct CueRule {
    /// The kind the cue supports.
    pub kind: EntityKind,
    /// Patterns, any of which triggers the rule.
    pub patterns: Vec<CuePattern>,
    /// How many words before a candidate the cue may be.
    pub window_before: u16,
    /// How many words after a candidate the cue may be.
    pub window_after: u16,
    /// The cue's weight in the score model (`w_cue`).
    pub boost: f32,
    /// Whether the cue in an assistant turn opens the caller's next turn
    /// (`TC kimlik numaranızı alabilir miyim?`).
    pub cross_turn: bool,
}

#[derive(Debug, Clone)]
struct Occurrence {
    kind: EntityKind,
    words: Range<u32>,
    pattern: String,
    boost: f32,
    window_before: u16,
    window_after: u16,
    cross_turn: bool,
}

/// Every cue occurrence in one turn.
#[derive(Debug, Clone, Default)]
pub struct CueIndex {
    occurrences: Vec<Occurrence>,
}

impl CueIndex {
    /// Finds every cue occurrence in a turn.
    #[must_use]
    pub fn build(rules: &[CueRule], words: &[NormWord]) -> Self {
        let keys: Vec<String> = words.iter().map(|word| flatten(word.stem())).collect();
        let mut occurrences = Vec::new();
        for rule in rules {
            for pattern in &rule.patterns {
                for start in 0..keys.len() {
                    if !pattern.matches_at(&keys, start) {
                        continue;
                    }
                    let begin = u32::try_from(start).unwrap_or(u32::MAX);
                    let end = u32::try_from(start + pattern.len()).unwrap_or(u32::MAX);
                    occurrences.push(Occurrence {
                        kind: rule.kind,
                        words: begin..end,
                        pattern: pattern.text().to_owned(),
                        boost: rule.boost,
                        window_before: rule.window_before,
                        window_after: rule.window_after,
                        cross_turn: rule.cross_turn,
                    });
                }
            }
        }
        occurrences.sort_by_key(|o| (o.words.start, o.words.end));
        Self { occurrences }
    }

    /// Cues for `kind` within reach of the candidate words, one per pattern at
    /// its smallest distance.
    #[must_use]
    pub fn hits(&self, kind: EntityKind, words: &Range<u32>) -> Vec<CueHit> {
        let mut hits: Vec<CueHit> = Vec::new();
        for occurrence in self.occurrences.iter().filter(|o| o.kind == kind) {
            let Some(distance) = distance(&occurrence.words, words, occurrence) else {
                continue;
            };
            match hits
                .iter_mut()
                .find(|hit| hit.pattern == occurrence.pattern)
            {
                Some(hit) if hit.distance <= distance => {}
                Some(hit) => hit.distance = distance,
                None => hits.push(CueHit {
                    pattern: occurrence.pattern.clone(),
                    distance,
                    boost: occurrence.boost,
                    cross_turn: false,
                }),
            }
        }
        hits
    }

    /// The first kind with a cross-turn cue in this turn: an assistant asking
    /// for it means the caller's next turn probably contains it.
    #[must_use]
    pub fn first_cross_turn_kind(&self) -> Option<EntityKind> {
        self.occurrences
            .iter()
            .find(|o| o.cross_turn)
            .map(|o| o.kind)
    }
}

/// Words between a cue and a candidate, if the cue is within its window.
fn distance(cue: &Range<u32>, candidate: &Range<u32>, occurrence: &Occurrence) -> Option<u16> {
    let gap = if cue.end <= candidate.start {
        let gap = candidate.start - cue.end;
        (gap <= u32::from(occurrence.window_before)).then_some(gap)?
    } else if cue.start >= candidate.end {
        let gap = cue.start - candidate.end;
        (gap <= u32::from(occurrence.window_after)).then_some(gap)?
    } else {
        0
    };
    u16::try_from(gap).ok()
}

#[cfg(test)]
mod tests {
    use ortu_normalize::Canonicalizer;

    use super::{CueIndex, CuePattern, CueRule};
    use crate::entity::EntityKind;

    fn rule(kind: EntityKind, patterns: &[&str]) -> CueRule {
        CueRule {
            kind,
            patterns: patterns.iter().map(|p| CuePattern::parse(p)).collect(),
            window_before: 6,
            window_after: 6,
            boost: 2.0,
            cross_turn: true,
        }
    }

    fn index(rules: &[CueRule], text: &str) -> CueIndex {
        let stems: Vec<&str> = rules
            .iter()
            .flat_map(|r| r.patterns.iter().flat_map(CuePattern::stems))
            .collect();
        let words: Vec<&str> = text.split_whitespace().collect();
        CueIndex::build(
            rules,
            &Canonicalizer::with_stems(stems).canonicalize(&words),
        )
    }

    #[test]
    fn cues_match_through_suffixes_case_and_diacritics() {
        let rules = [
            rule(EntityKind::Iban, &["iban"]),
            rule(EntityKind::Tckn, &["numara"]),
        ];
        let idx = index(&rules, "IBAN'ım ve numaram şu");
        assert_eq!(idx.hits(EntityKind::Iban, &(3..4))[0].distance, 2);
        assert_eq!(idx.hits(EntityKind::Tckn, &(3..4))[0].distance, 0);
    }

    #[test]
    fn multi_word_and_prefix_patterns() {
        let rules = [
            rule(EntityKind::Tckn, &["te ce"]),
            rule(EntityKind::Phone, &["ulaş*"]),
        ];
        let idx = index(&rules, "te ce bir iki ulaşabilirsiniz");
        assert_eq!(idx.hits(EntityKind::Tckn, &(2..4)).len(), 1);
        assert_eq!(idx.hits(EntityKind::Phone, &(2..4))[0].distance, 0);
    }

    #[test]
    fn cues_outside_the_window_do_not_count() {
        let mut far = rule(EntityKind::Tckn, &["tc"]);
        far.window_before = 1;
        let idx = index(&[far], "tc a b c bir");
        assert_eq!(idx.hits(EntityKind::Tckn, &(4..5)), Vec::new());
    }

    #[test]
    fn assistant_question_opens_the_next_turn() {
        let rules = [rule(EntityKind::Tckn, &["kimlik"])];
        let idx = index(&rules, "kimlik numaranızı alabilir miyim");
        assert_eq!(idx.first_cross_turn_kind(), Some(EntityKind::Tckn));
    }
}
