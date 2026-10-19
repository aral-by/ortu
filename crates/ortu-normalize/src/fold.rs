//! Turkish case folding and match keys
//! ([algorithm 01](https://github.com/aral-by/ortu/blob/main/docs/algorithms/01-text-canonicalization.md)).

use unicode_normalization::{UnicodeNormalization, is_nfc};

/// U+0307, left behind when `İ` is lowercased with non-Turkish rules.
const COMBINING_DOT_ABOVE: char = '\u{0307}';

/// Apostrophe variants produced by STT engines and keyboards; all become `'`.
const APOSTROPHES: [char; 5] = ['\u{2019}', '\u{2018}', '\u{00B4}', '\u{0060}', '\u{02BC}'];

/// NFC-normalizes `text`, unifies apostrophes and lowercases it with Turkish
/// rules: `İ` becomes `i` and `I` becomes `ı`.
///
/// The result is idempotent: folding a folded string returns it unchanged.
#[must_use]
pub fn turkish_fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for c in text.nfc() {
        match c {
            'İ' => folded.push('i'),
            'I' => folded.push('ı'),
            // Repair `i` + U+0307 produced by upstream non-Turkish lowercasing.
            COMBINING_DOT_ABOVE if folded.ends_with('i') => {}
            c if APOSTROPHES.contains(&c) => folded.push('\''),
            c => folded.extend(c.to_lowercase()),
        }
    }
    // Lowercasing can create new compositions: `İ` + U+0308 becomes `i` + U+0308,
    // whose NFC form is `ï`. Composing again keeps the function idempotent.
    if is_nfc(&folded) {
        folded
    } else {
        folded.nfc().collect()
    }
}

/// Flattens a folded string into a lossy match key: `ı` becomes `i` and
/// diacritics are dropped (`ç` to `c`, `ş` to `s`, ...).
///
/// Used only for dictionary lookups of words that STT emitted without any
/// Turkish-specific letters, such as `uc` for `üç`.
#[must_use]
pub fn flatten(folded: &str) -> String {
    folded
        .chars()
        .map(|c| match c {
            'ı' | 'î' => 'i',
            'ç' => 'c',
            'ğ' => 'g',
            'ö' => 'o',
            'ş' => 's',
            'ü' | 'û' => 'u',
            'â' => 'a',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{flatten, turkish_fold};

    #[test]
    fn dotted_and_dotless_capitals_follow_turkish_rules() {
        assert_eq!(turkish_fold("İSTANBUL"), "istanbul");
        assert_eq!(turkish_fold("IŞIK"), "ışık");
        assert_eq!(turkish_fold("IBAN"), "ıban");
    }

    #[test]
    fn upstream_lowercasing_damage_is_repaired() {
        assert_eq!(turkish_fold("i\u{0307}stanbul"), "istanbul");
    }

    #[test]
    fn decomposed_input_is_composed() {
        assert_eq!(turkish_fold("u\u{0308}c\u{0327}"), "üç");
        assert_eq!(turkish_fold("I\u{0307}"), "i");
    }

    #[test]
    fn lowercasing_that_creates_a_composition_is_recomposed() {
        // Found by the stress test: `İ` + U+0308 folds to `i` + U+0308, which
        // NFC composes to `ï`. Without recomposition, folding twice differed.
        let once = turkish_fold("İ\u{0308}");
        assert_eq!(once, "ï");
        assert_eq!(turkish_fold(&once), once);
        let input = "ğ'İ\u{0308}ğğüİ9\u{0308}eü";
        assert_eq!(turkish_fold(&turkish_fold(input)), turkish_fold(input));
    }

    #[test]
    fn apostrophe_variants_are_unified() {
        assert_eq!(turkish_fold("TC’m"), "tc'm");
        assert_eq!(turkish_fold("TC´m"), "tc'm");
    }

    #[test]
    fn flatten_drops_turkish_specific_letters() {
        assert_eq!(flatten("üç"), "uc");
        assert_eq!(flatten("ıban'ım"), "iban'im");
        assert_eq!(flatten("şöğç"), "sogc");
    }
}
