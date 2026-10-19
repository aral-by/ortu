//! Filler, interruption and correction marking
//! ([algorithm 02](https://github.com/aral-by/ortu/blob/main/docs/algorithms/02-filler-and-correction-marking.md)).
//!
//! The word lists live in `lexicon` and match folded forms.

use crate::canonical::NormWord;
use crate::lexicon::{CORRECTIONS, FILLERS, INTERRUPTIONS};
use crate::number::is_number_word;

/// The conversational role of a word. Words are marked, never removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WordRole {
    /// Ordinary content.
    Content,
    /// Hesitation or filler (`ııı`, `şey`); a skippable gap inside a digit run.
    Filler,
    /// Interruption (`bir saniye`, `dur`); a skippable gap inside a digit run.
    Interruption,
    /// Correction marker between two digit runs (`yok yok`, `pardon`, `hayır`);
    /// the run after it overrides the run before it.
    Correction,
}

/// How far (in words) a correction marker may be from the digit runs it
/// connects.
const CORRECTION_WINDOW: usize = 2;

/// Marks the role of every word in a turn.
#[must_use]
pub fn mark_roles(words: &[NormWord]) -> Vec<WordRole> {
    let mut roles = vec![WordRole::Content; words.len()];
    let mut i = 0;
    while i < words.len() {
        if let Some((len, role)) = longest_phrase(words, i) {
            roles[i..i + len].fill(role);
            i += len;
        } else {
            if is_filler(&words[i]) {
                roles[i] = WordRole::Filler;
            }
            i += 1;
        }
    }
    gate_corrections(words, &mut roles);
    roles
}

fn longest_phrase(words: &[NormWord], start: usize) -> Option<(usize, WordRole)> {
    let candidates = INTERRUPTIONS
        .iter()
        .map(|p| (*p, WordRole::Interruption))
        .chain(CORRECTIONS.iter().map(|p| (*p, WordRole::Correction)));
    candidates
        .filter(|(phrase, _)| {
            words
                .get(start..start + phrase.len())
                .is_some_and(|window| {
                    window
                        .iter()
                        .zip(phrase.iter())
                        .all(|(word, expected)| word.folded == *expected)
                })
        })
        .max_by_key(|(phrase, _)| phrase.len())
        .map(|(phrase, role)| (phrase.len(), role))
}

fn is_filler(word: &NormWord) -> bool {
    word.folded.is_empty()
        || FILLERS.contains(&word.folded.as_str())
        || is_hesitation(&word.match_key)
}

/// Elongated hesitation sounds on the match key: `ıı(ı)`, `ee(e)`, `aa(a)`,
/// `mm(m)` and `hm(m)`. Single letters (`e`, `a`) are words, not hesitations.
fn is_hesitation(key: &str) -> bool {
    if let Some(hum) = key.strip_prefix('h') {
        return !hum.is_empty() && hum.chars().all(|c| c == 'm');
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(first @ ('i' | 'e' | 'a' | 'm')) => {
            key.chars().count() >= 2 && chars.all(|c| c == first)
        }
        _ => false,
    }
}

/// A correction marker only counts as one between two digit runs; elsewhere
/// (`hayır` as a plain "no") it is ordinary content.
fn gate_corrections(words: &[NormWord], roles: &mut [WordRole]) {
    let mut i = 0;
    while i < roles.len() {
        if roles[i] != WordRole::Correction {
            i += 1;
            continue;
        }
        let start = i;
        while i < roles.len() && roles[i] == WordRole::Correction {
            i += 1;
        }
        let before = &words[start.saturating_sub(CORRECTION_WINDOW)..start];
        let after = &words[i..(i + CORRECTION_WINDOW).min(words.len())];
        let connects_runs = before.iter().any(is_number_word) && after.iter().any(is_number_word);
        if !connects_runs {
            roles[start..i].fill(WordRole::Content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WordRole, mark_roles};
    use crate::canonical::Canonicalizer;

    fn roles(text: &str) -> Vec<WordRole> {
        let words: Vec<&str> = text.split_whitespace().collect();
        mark_roles(&Canonicalizer::default().canonicalize(&words))
    }

    #[test]
    fn interruption_phrase_claims_bir_before_it_becomes_a_digit() {
        use WordRole::{Content, Filler, Interruption};
        assert_eq!(
            roles("on iki ııı bir saniye kırk beş"),
            [
                Content,
                Content,
                Filler,
                Interruption,
                Interruption,
                Content,
                Content
            ]
        );
    }

    #[test]
    fn correction_between_digit_runs_is_kept() {
        use WordRole::{Content, Correction};
        assert_eq!(
            roles("beş üç iki yok yok beş üç üç"),
            [
                Content, Content, Content, Correction, Correction, Content, Content, Content
            ]
        );
    }

    #[test]
    fn correction_word_outside_digit_runs_is_content() {
        assert!(
            roles("hayır teşekkürler")
                .iter()
                .all(|r| *r == WordRole::Content)
        );
    }

    #[test]
    fn hesitations_and_punctuation_only_tokens_are_fillers() {
        assert!(
            roles("ıııı eee hmmm mmm ...")
                .iter()
                .all(|r| *r == WordRole::Filler)
        );
        assert_eq!(roles("ha ee"), [WordRole::Content, WordRole::Filler]);
    }
}
