//! Canonical per-word views
//! ([algorithm 01](https://github.com/aral-by/ortu/blob/main/docs/algorithms/01-text-canonicalization.md)).

use std::collections::HashSet;
use std::ops::Range;

use crate::WordRef;
use crate::fold::{flatten, turkish_fold};
use crate::lexicon::SUFFIXES;

/// Punctuation trimmed from the edges of a word. Internal punctuation is kept
/// because emails, dates and plates depend on it.
fn is_edge_punctuation(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | ';'
            | ':'
            | '!'
            | '?'
            | '"'
            | '\''
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '…'
            | '«'
            | '»'
            | '“'
            | '”'
    )
}

/// The canonical view of one source word. The source text itself is never
/// modified; every field can be traced back to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormWord {
    /// Position of the word in its turn.
    pub word: WordRef,
    /// Byte range of the raw word in the turn text, where the turn text is the
    /// words joined by single spaces (docs/systemdesign.md, section 5.2).
    pub bytes: Range<u32>,
    /// Linguistically correct Turkish lowercase, edge punctuation removed.
    pub folded: String,
    /// `folded` with `ı` and diacritics flattened, for dictionary lookups.
    pub match_key: String,
    /// Byte length of the stem inside `folded` when a suffix was split off.
    pub stem_len: Option<u16>,
}

impl NormWord {
    /// The stem of the word: the part before an apostrophe or a recognized
    /// suffix, or the whole folded word.
    #[must_use]
    pub fn stem(&self) -> &str {
        self.stem_len
            .and_then(|len| self.folded.get(..usize::from(len)))
            .unwrap_or(&self.folded)
    }

    /// Whether STT emitted the word without any Turkish-specific letters, which
    /// is the only case where `match_key` lookups are trusted.
    #[must_use]
    pub fn is_plain_ascii_form(&self) -> bool {
        self.folded == self.match_key
    }
}

/// Builds [`NormWord`] views. Holds the stem dictionary used to split suffixes
/// from words without an apostrophe.
#[derive(Debug, Clone, Default)]
pub struct Canonicalizer {
    stems: HashSet<String>,
}

impl Canonicalizer {
    /// A canonicalizer that splits suffixes after the given stems. Stems are
    /// folded with the same function as the input, so they may be written in
    /// any case.
    #[must_use]
    pub fn with_stems<I, S>(stems: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            stems: stems
                .into_iter()
                .map(|s| turkish_fold(s.as_ref()))
                .collect(),
        }
    }

    /// Canonicalizes the words of one turn.
    #[must_use]
    pub fn canonicalize<S: AsRef<str>>(&self, words: &[S]) -> Vec<NormWord> {
        let mut offset: u32 = 0;
        let mut out = Vec::with_capacity(words.len());
        for (index, raw) in words.iter().enumerate() {
            let raw = raw.as_ref();
            let start = offset;
            let end = start.saturating_add(u32::try_from(raw.len()).unwrap_or(u32::MAX));
            offset = end.saturating_add(1);

            let folded = turkish_fold(raw)
                .trim_matches(is_edge_punctuation)
                .to_owned();
            out.push(NormWord {
                word: WordRef(u32::try_from(index).unwrap_or(u32::MAX)),
                bytes: start..end,
                match_key: flatten(&folded),
                stem_len: self.split_suffix(&folded),
                folded,
            });
        }
        out
    }

    fn split_suffix(&self, folded: &str) -> Option<u16> {
        if let Some(position) = folded.find('\'') {
            return u16::try_from(position).ok().filter(|&p| p > 0);
        }
        folded
            .char_indices()
            .map(|(i, _)| i)
            .filter(|&i| i > 0)
            .rev()
            .find(|&i| self.stems.contains(&folded[..i]) && SUFFIXES.contains(&&folded[i..]))
            .and_then(|i| u16::try_from(i).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::Canonicalizer;

    fn view(raw: &str) -> (String, String, String) {
        let canonicalizer = Canonicalizer::with_stems(["numara"]);
        let word = canonicalizer.canonicalize(&[raw]).remove(0);
        (
            word.folded.clone(),
            word.match_key.clone(),
            word.stem().to_owned(),
        )
    }

    #[test]
    fn examples_from_algorithm_01() {
        assert_eq!(
            view("İSTANBUL"),
            ("istanbul".into(), "istanbul".into(), "istanbul".into())
        );
        assert_eq!(
            view("IBAN’ım"),
            ("ıban'ım".into(), "iban'im".into(), "ıban".into())
        );
        assert_eq!(view("TC’m"), ("tc'm".into(), "tc'm".into(), "tc".into()));
        assert_eq!(
            view("numaram"),
            ("numaram".into(), "numaram".into(), "numara".into())
        );
        assert_eq!(view("Üç"), ("üç".into(), "uc".into(), "üç".into()));
        assert_eq!(view("uc"), ("uc".into(), "uc".into(), "uc".into()));
    }

    #[test]
    fn edge_punctuation_is_trimmed_but_internal_punctuation_kept() {
        assert_eq!(view("beş,").0, "beş");
        assert_eq!(view("(ahmet.yilmaz@ornek.com)").0, "ahmet.yilmaz@ornek.com");
        assert_eq!(view("12'de").2, "12");
    }

    #[test]
    fn byte_ranges_follow_the_single_space_joined_turn_text() {
        let words = Canonicalizer::default().canonicalize(&["TC'm", "üç", "beş"]);
        let text = "TC'm üç beş";
        let slices: Vec<&str> = words
            .iter()
            .map(|w| &text[w.bytes.start as usize..w.bytes.end as usize])
            .collect();
        assert_eq!(slices, ["TC'm", "üç", "beş"]);
    }
}
