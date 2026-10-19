//! Spelled-out input (docs/systemdesign.md, section 6.5).
//!
//! Letters read one by one (`te re` for `TR`, `a be ce` for a plate) and spoken
//! symbols (`nokta`, `et`) used by IBAN, plate and email detectors.

use std::ops::Range;

use crate::WordRef;
use crate::canonical::NormWord;
use crate::lexicon::{LETTER_NAMES, SPOKEN_SYMBOLS, lookup};
use crate::number::is_number_word;
use crate::roles::WordRole;

/// Consecutive spelled letters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpelledRun {
    /// The letters, uppercase (`TR`, `ABC`).
    pub text: String,
    /// `source[i]` is the word that produced the `i`-th letter.
    pub source: Vec<WordRef>,
    /// Word indices covered by the run.
    pub words: Range<u32>,
}

/// The letter a word spells, by its Turkish letter name (`be`, `ce`, `ka`) or
/// as a single letter written by STT (`t`).
fn letter(folded: &str) -> Option<char> {
    lookup(LETTER_NAMES, folded).or_else(|| single_letter(folded))
}

/// Vowels are named by themselves (`a`, `ı`, `ü`), and STT sometimes writes any
/// letter as itself (`t r`).
fn single_letter(folded: &str) -> Option<char> {
    let mut chars = folded.chars();
    match (chars.next(), chars.next()) {
        (Some('i'), None) => Some('İ'),
        (Some('ı'), None) => Some('I'),
        (Some(c), None) if c.is_alphabetic() => c.to_uppercase().next(),
        _ => None,
    }
}

/// Finds runs of spelled letters.
///
/// Many letter names are also ordinary words (`ve`, `de`, `o`), so a run needs
/// at least two letters, or a single letter between two number words (the
/// letter group of a plate such as `34 A 1234`). Downstream detectors validate
/// every run; this step only exposes candidates.
#[must_use]
pub fn spelled_runs(words: &[NormWord], roles: &[WordRole]) -> Vec<SpelledRun> {
    let is_letter = |i: usize| {
        roles.get(i).copied().unwrap_or(WordRole::Content) == WordRole::Content
            && letter(&words[i].folded).is_some()
    };
    let mut runs = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if !is_letter(i) {
            i += 1;
            continue;
        }
        let start = i;
        while i < words.len() && is_letter(i) {
            i += 1;
        }
        let between_numbers = i - start == 1
            && start > 0
            && i < words.len()
            && is_number_word(&words[start - 1])
            && is_number_word(&words[i]);
        if i - start >= 2 || between_numbers {
            runs.push(build_run(&words[start..i]));
        }
    }
    runs
}

fn build_run(words: &[NormWord]) -> SpelledRun {
    let text = words.iter().filter_map(|w| letter(&w.folded)).collect();
    let source = words.iter().map(|w| w.word).collect();
    let first = words.first().map_or(0, |w| w.word.0);
    let end = words.last().map_or(0, |w| w.word.0.saturating_add(1));
    SpelledRun {
        text,
        source,
        words: first..end,
    }
}

/// The symbol spoken at `words[index]`, if any, and how many words it spans:
/// `nokta` is `.`, `et` or `at` is `@`, `tire` is `-`, `alt çizgi` is `_`,
/// `kuyruklu a` is `@`.
///
/// `et` and `at` are also ordinary words; email detection (M4) only consults
/// this inside an email-shaped context.
#[must_use]
pub fn spoken_symbol(words: &[NormWord], index: usize) -> Option<(char, usize)> {
    SPOKEN_SYMBOLS
        .iter()
        .filter(|(phrase, _)| {
            words
                .get(index..index + phrase.len())
                .is_some_and(|window| {
                    window
                        .iter()
                        .zip(phrase.iter())
                        .all(|(w, p)| w.folded == *p)
                })
        })
        .max_by_key(|(phrase, _)| phrase.len())
        .map(|(phrase, symbol)| (*symbol, phrase.len()))
}

#[cfg(test)]
mod tests {
    use super::{spelled_runs, spoken_symbol};
    use crate::canonical::Canonicalizer;
    use crate::roles::mark_roles;

    fn runs(text: &str) -> Vec<String> {
        let raw: Vec<&str> = text.split_whitespace().collect();
        let words = Canonicalizer::default().canonicalize(&raw);
        spelled_runs(&words, &mark_roles(&words))
            .into_iter()
            .map(|run| run.text)
            .collect()
    }

    #[test]
    fn iban_prefix_and_plate_letters() {
        assert_eq!(runs("te re otuz üç"), ["TR"]);
        assert_eq!(runs("t r otuz üç"), ["TR"]);
        assert_eq!(runs("otuz dört a be ce yüz yirmi üç"), ["ABC"]);
    }

    #[test]
    fn single_letter_counts_only_between_numbers() {
        assert_eq!(runs("otuz dört a bin iki yüz"), ["A"]);
        assert_eq!(runs("bu bir o kadar"), Vec::<String>::new());
    }

    #[test]
    fn spoken_symbols() {
        let raw = ["ahmet", "nokta", "yılmaz", "et", "alt", "çizgi"];
        let words = Canonicalizer::default().canonicalize(&raw);
        assert_eq!(spoken_symbol(&words, 1), Some(('.', 1)));
        assert_eq!(spoken_symbol(&words, 3), Some(('@', 1)));
        assert_eq!(spoken_symbol(&words, 4), Some(('_', 2)));
        assert_eq!(spoken_symbol(&words, 0), None);
    }
}
