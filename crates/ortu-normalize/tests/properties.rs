//! Property-based tests for normalization (docs/systemdesign.md, section 16;
//! docs/algorithms/01 and 03).

use ortu_normalize::{NormalizeConfig, normalize, turkish_fold};
use proptest::prelude::*;

const UNITS: [&str; 10] = [
    "sıfır", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz",
];
const TENS: [&str; 10] = [
    "", "on", "yirmi", "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan",
];

/// Words that exercise every code path: number words, fallbacks, fillers,
/// interruptions, corrections, letters, digit tokens and punctuation.
const VOCABULARY: &[&str] = &[
    "sıfır", "bir", "iki", "üç", "on", "yirmi", "yüz", "bin", "milyon", "milyar", "uc", "dort",
    "ııı", "şey", "bir", "saniye", "yok", "pardon", "hayır", "te", "re", "a", "be", "0532",
    "12'de", "TC'm", "İBAN", "...", "ve", "ön", "uç",
];

/// Reads a single digit.
fn unit(d: u8) -> Vec<&'static str> {
    vec![UNITS[usize::from(d)]]
}

/// Reads two digits as a number, with a leading zero read separately.
fn pair(a: u8, b: u8) -> Vec<&'static str> {
    match (a, b) {
        (0, _) => [unit(0), unit(b)].concat(),
        (_, 0) => vec![TENS[usize::from(a)]],
        _ => vec![TENS[usize::from(a)], UNITS[usize::from(b)]],
    }
}

/// Reads three digits as a number, with a leading zero read separately.
fn triple(a: u8, b: u8, c: u8) -> Vec<&'static str> {
    if a == 0 {
        return [unit(0), pair(b, c)].concat();
    }
    let mut words = if a == 1 {
        vec!["yüz"]
    } else {
        vec![UNITS[usize::from(a)], "yüz"]
    };
    if b > 0 {
        words.push(TENS[usize::from(b)]);
    }
    if c > 0 {
        words.push(UNITS[usize::from(c)]);
    }
    words
}

#[derive(Debug, Clone, Copy)]
enum Style {
    DigitByDigit,
    Pairs,
    Triples,
}

/// Reads `digits` in `style`. Pair style requires an even length. Triple style
/// requires every non-final group to end in a non-zero digit, and a group read
/// as a lone digit (`0 0 c`) not to be followed by a bare `yüz`. Under those
/// conditions the reading is unambiguous, and the natural grouping must
/// reproduce `digits` exactly.
fn read(digits: &[u8], style: Style) -> Vec<&'static str> {
    match style {
        Style::DigitByDigit => digits.iter().flat_map(|d| unit(*d)).collect(),
        Style::Pairs => digits.chunks(2).flat_map(|p| pair(p[0], p[1])).collect(),
        Style::Triples => digits
            .chunks(3)
            .flat_map(|chunk| match *chunk {
                [a, b, c] => triple(a, b, c),
                [a, b] => pair(a, b),
                [a] => unit(a),
                _ => Vec::new(),
            })
            .collect(),
    }
}

fn unambiguous_digits() -> impl Strategy<Value = (Vec<u8>, Style)> {
    let style = prop_oneof![
        Just(Style::DigitByDigit),
        Just(Style::Pairs),
        Just(Style::Triples)
    ];
    (prop::collection::vec(0u8..10, 1..=24), style).prop_map(|(mut digits, style)| {
        match style {
            Style::Pairs if digits.len() % 2 == 1 => digits.push(1),
            Style::Triples => {
                let groups = digits.len().div_ceil(3);
                for group in 0..groups.saturating_sub(1) {
                    let start = group * 3;
                    // A trailing zero would let the next group's first word extend this one.
                    if digits[start + 2] == 0 {
                        digits[start + 2] = 1;
                    }
                    // `0 0 c` ends in a lone unit; a following bare `yüz` (a group starting
                    // with 1) would be heard as "c yüz". That is genuine spoken ambiguity,
                    // which the lattice exists for, so it is excluded here.
                    let next_is_full = start + 6 <= digits.len();
                    if digits[start] == 0
                        && digits[start + 1] == 0
                        && next_is_full
                        && digits[start + 3] == 1
                    {
                        digits[start + 3] = 2;
                    }
                }
            }
            _ => {}
        }
        (digits, style)
    })
}

proptest! {
    #[test]
    fn turkish_fold_is_idempotent(text in "\\PC{0,24}") {
        let once = turkish_fold(&text);
        prop_assert_eq!(turkish_fold(&once), once);
    }

    #[test]
    fn unambiguous_readings_reproduce_the_digits((digits, style) in unambiguous_digits()) {
        let words = read(&digits, style);
        let turn = normalize(&words, &NormalizeConfig::default());
        let expected: String = digits.iter().map(|d| char::from(b'0' + d)).collect();

        prop_assert_eq!(turn.lattices.len(), 1, "words: {:?}", words);
        prop_assert_eq!(&turn.lattices[0].readings[0].digits, &expected, "words: {:?}", words);
    }

    #[test]
    fn offsets_always_point_into_the_source(
        words in prop::collection::vec(
            prop_oneof![prop::sample::select(VOCABULARY).prop_map(str::to_owned), "\\PC{1,8}"],
            0..48,
        )
    ) {
        let turn = normalize(&words, &NormalizeConfig::default());
        let word_count = u32::try_from(words.len()).unwrap();
        let text_len = u32::try_from(words.iter().map(String::len).sum::<usize>() + words.len().saturating_sub(1)).unwrap();

        prop_assert_eq!(turn.words.len(), words.len());
        prop_assert_eq!(turn.roles.len(), words.len());
        for word in &turn.words {
            prop_assert!(word.bytes.start <= word.bytes.end && word.bytes.end <= text_len);
        }
        for lattice in &turn.lattices {
            prop_assert!(lattice.words.start < lattice.words.end && lattice.words.end <= word_count);
            prop_assert!(!lattice.readings.is_empty() && lattice.readings.len() <= 8);
            prop_assert!((lattice.readings[0].prior - 1.0).abs() < f32::EPSILON);
            for reading in &lattice.readings {
                prop_assert_eq!(reading.digits.len(), reading.source.len());
                prop_assert!(reading.source.iter().all(|w| lattice.words.contains(&w.0)));
                prop_assert!(reading.prior > 0.0 && reading.prior <= 1.0);
            }
        }
        for run in &turn.spelled {
            prop_assert_eq!(run.text.chars().count(), run.source.len());
            prop_assert!(run.words.end <= word_count);
        }
    }
}
