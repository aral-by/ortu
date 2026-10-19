//! Turkish word lists used by normalization.
//!
//! Data only. Every list the normalization algorithms consult lives here, so a
//! reviewer can check or extend the Turkish data without reading the
//! algorithms, and the algorithms contain rules only.
//!
//! - Entries are written in folded form (`fold::turkish_fold`); a test enforces
//!   it, so `IBAN` and `iban` can never end up as two different entries.
//! - Diacritic-free variants are listed explicitly, and only where they do not
//!   collide with another word: `şöyle` ("like this") has no `soyle` variant
//!   because that would also match `söyle` ("say").
//! - These lists are small and grammatical, so they are compiled in and
//!   versioned with the code. Large curated datasets (name gazetteers,
//!   provinces and districts) live as data files in `ortu-tr`, and
//!   adopter-specific vocabulary is supplied at runtime (for example
//!   `Canonicalizer::with_stems`).

/// The meaning of a number word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberWord {
    Zero,
    Unit(u8),
    Ten(u8),
    Hundred,
    /// `bin`, `milyon`, `milyar`: the power of ten they scale by.
    Scale(u8),
}

/// Number words in their correct Turkish spelling.
pub(crate) const NUMBER_WORDS: &[(&str, NumberWord)] = &[
    ("sıfır", NumberWord::Zero),
    ("bir", NumberWord::Unit(1)),
    ("iki", NumberWord::Unit(2)),
    ("üç", NumberWord::Unit(3)),
    ("dört", NumberWord::Unit(4)),
    ("beş", NumberWord::Unit(5)),
    ("altı", NumberWord::Unit(6)),
    ("yedi", NumberWord::Unit(7)),
    ("sekiz", NumberWord::Unit(8)),
    ("dokuz", NumberWord::Unit(9)),
    ("on", NumberWord::Ten(1)),
    ("yirmi", NumberWord::Ten(2)),
    ("otuz", NumberWord::Ten(3)),
    ("kırk", NumberWord::Ten(4)),
    ("elli", NumberWord::Ten(5)),
    ("altmış", NumberWord::Ten(6)),
    ("yetmiş", NumberWord::Ten(7)),
    ("seksen", NumberWord::Ten(8)),
    ("doksan", NumberWord::Ten(9)),
    ("yüz", NumberWord::Hundred),
    ("bin", NumberWord::Scale(3)),
    ("milyon", NumberWord::Scale(6)),
    ("milyar", NumberWord::Scale(9)),
];

/// Number words as STT writes them without Turkish-specific letters, plus
/// observed garbles. Consulted only for words that contain no such letters.
pub(crate) const FLATTENED_NUMBER_WORDS: &[(&str, NumberWord)] = &[
    ("sifir", NumberWord::Zero),
    ("uc", NumberWord::Unit(3)),
    ("dort", NumberWord::Unit(4)),
    ("bes", NumberWord::Unit(5)),
    ("alti", NumberWord::Unit(6)),
    ("kirk", NumberWord::Ten(4)),
    ("altmis", NumberWord::Ten(6)),
    ("yetmis", NumberWord::Ten(7)),
    ("doksn", NumberWord::Ten(9)),
    ("yuz", NumberWord::Hundred),
];

/// Short suffixes accepted after a dictionary stem when a word has no
/// apostrophe, for example `numaram` = `numara` + `m`.
pub(crate) const SUFFIXES: &[&str] = &[
    "m", "ım", "im", "um", "üm", "n", "ın", "in", "un", "ün", "ı", "i", "u", "ü", "sı", "si", "su",
    "sü", "mız", "miz", "muz", "müz", "nız", "niz", "nuz", "nüz", "yı", "yi", "yu", "yü", "a", "e",
    "ya", "ye", "da", "de", "ta", "te", "dan", "den", "tan", "ten", "nın", "nin", "nun", "nün",
];

/// Single-word fillers. Elongated hesitations (`ııı`, `eee`, `hmm`) are
/// recognized by shape in `roles`, not listed.
pub(crate) const FILLERS: &[&str] = &["şey", "sey", "yani", "işte", "hmm", "hm"];

/// Interruptions: skippable gaps inside a digit run.
pub(crate) const INTERRUPTIONS: &[&[&str]] = &[
    &["bir", "saniye"],
    &["bir", "dakika"],
    &["dur"],
    &["şöyle"],
    &["bakayım"],
    &["bakayim"],
    &["bakalım"],
    &["bakalim"],
];

/// Correction markers: the digit run after them overrides the run before them.
pub(crate) const CORRECTIONS: &[&[&str]] = &[&["yok", "yok"], &["pardon"], &["hayır"], &["hayir"]];

/// Words an assistant uses to acknowledge without taking the floor (`evet`,
/// `hı hı`, `tamam`, `devam edin`). A turn made only of these, fillers and
/// interruptions does not end a value the caller is still dictating
/// ([algorithm 12](https://github.com/aral-by/ortu/blob/main/docs/algorithms/12-cross-turn-assembly.md)).
/// Plain-ASCII spellings are listed because STT often drops the dots.
pub(crate) const BACKCHANNELS: &[&str] = &[
    "evet",
    "hı",
    "hıhı",
    "hi",
    "hihi",
    "tamam",
    "peki",
    "aynen",
    "olur",
    "anladım",
    "anladim",
    "dinliyorum",
    "devam",
    "edin",
    "ediniz",
    "buyurun",
    "buyrun",
    "aldım",
    "aldim",
    "alıyorum",
    "aliyorum",
];

/// Turkish letter names. Vowels are named by themselves and are handled with
/// single letters in `spell`.
pub(crate) const LETTER_NAMES: &[(&str, char)] = &[
    ("be", 'B'),
    ("ce", 'C'),
    ("çe", 'Ç'),
    ("de", 'D'),
    ("fe", 'F'),
    ("ge", 'G'),
    ("ğe", 'Ğ'),
    ("yumuşak", 'Ğ'),
    ("he", 'H'),
    ("ha", 'H'),
    ("je", 'J'),
    ("ke", 'K'),
    ("ka", 'K'),
    ("le", 'L'),
    ("me", 'M'),
    ("ne", 'N'),
    ("pe", 'P'),
    ("re", 'R'),
    ("se", 'S'),
    ("şe", 'Ş'),
    ("te", 'T'),
    ("ve", 'V'),
    ("ye", 'Y'),
    ("ze", 'Z'),
    ("kü", 'Q'),
    ("ku", 'Q'),
    ("iks", 'X'),
];

/// Spoken symbols used in emails and identifiers. Longer phrases are matched
/// first.
pub(crate) const SPOKEN_SYMBOLS: &[(&[&str], char)] = &[
    (&["alt", "çizgi"], '_'),
    (&["kuyruklu", "a"], '@'),
    (&["nokta"], '.'),
    (&["et"], '@'),
    (&["at"], '@'),
    (&["tire"], '-'),
];

/// Looks `key` up in a `(word, value)` table.
pub(crate) fn lookup<T: Copy>(table: &[(&str, T)], key: &str) -> Option<T> {
    table
        .iter()
        .find(|(word, _)| *word == key)
        .map(|(_, value)| *value)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        BACKCHANNELS, CORRECTIONS, FILLERS, FLATTENED_NUMBER_WORDS, INTERRUPTIONS, LETTER_NAMES,
        NUMBER_WORDS, SPOKEN_SYMBOLS, SUFFIXES,
    };
    use crate::fold::{flatten, turkish_fold};

    fn all_entries() -> Vec<&'static str> {
        let phrases = INTERRUPTIONS
            .iter()
            .chain(CORRECTIONS)
            .chain(SPOKEN_SYMBOLS.iter().map(|(phrase, _)| phrase))
            .flat_map(|phrase| phrase.iter().copied());
        NUMBER_WORDS
            .iter()
            .map(|(word, _)| *word)
            .chain(FLATTENED_NUMBER_WORDS.iter().map(|(word, _)| *word))
            .chain(LETTER_NAMES.iter().map(|(word, _)| *word))
            .chain(SUFFIXES.iter().copied())
            .chain(FILLERS.iter().copied())
            .chain(BACKCHANNELS.iter().copied())
            .chain(phrases)
            .collect()
    }

    #[test]
    fn every_entry_is_written_in_folded_form() {
        for entry in all_entries() {
            assert_eq!(turkish_fold(entry), entry, "entry `{entry}` is not folded");
        }
    }

    #[test]
    fn flattened_number_words_contain_no_turkish_letters() {
        for (word, _) in FLATTENED_NUMBER_WORDS {
            assert_eq!(flatten(word), *word, "`{word}` is not a flattened form");
        }
    }

    #[test]
    fn keyed_tables_have_no_duplicate_keys() {
        let tables: [Vec<&str>; 3] = [
            NUMBER_WORDS.iter().map(|(word, _)| *word).collect(),
            FLATTENED_NUMBER_WORDS
                .iter()
                .map(|(word, _)| *word)
                .collect(),
            LETTER_NAMES.iter().map(|(word, _)| *word).collect(),
        ];
        for keys in tables {
            let unique: HashSet<_> = keys.iter().collect();
            assert_eq!(unique.len(), keys.len(), "duplicate key in {keys:?}");
        }
    }
}
