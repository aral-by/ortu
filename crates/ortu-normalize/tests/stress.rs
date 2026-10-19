//! Randomized stress test for normalization (ADR-0005).
//!
//! Coverage-guided fuzzing (`fuzz/`, cargo-fuzz) does not link on Windows with
//! the MSVC toolchain. This test runs the same input generators and the same
//! invariants as the fuzz targets, driven by a seeded pseudo-random generator
//! instead of coverage feedback. It is ignored by default because of its
//! length; run it with `cargo xtask fuzz`.
//!
//! `ORTU_STRESS_ITERATIONS` overrides the iteration count (default 1,000,000).
//! `ORTU_STRESS_SEED` overrides the seed so a failure can be reproduced.

use ortu_normalize::{NormalizeConfig, NormalizedTurn, normalize, turkish_fold};

const DEFAULT_ITERATIONS: u64 = 1_000_000;
const DEFAULT_SEED: u64 = 0x6f72_7475; // "ortu"

/// Same vocabulary as `fuzz/fuzz_targets/parse_spoken_number.rs`.
const VOCABULARY: &[&str] = &[
    "sıfır",
    "bir",
    "iki",
    "üç",
    "dört",
    "beş",
    "altı",
    "yedi",
    "sekiz",
    "dokuz",
    "on",
    "yirmi",
    "otuz",
    "kırk",
    "elli",
    "altmış",
    "yetmiş",
    "seksen",
    "doksan",
    "yüz",
    "bin",
    "milyon",
    "milyar",
    "uc",
    "dort",
    "bes",
    "kirk",
    "yuz",
    "0",
    "12",
    "0532",
    "12'de",
    "ııı",
    "şey",
    "bir saniye",
    "yok yok",
    "pardon",
    "hayır",
    "te",
    "re",
    "a",
    "be",
    "kapı",
];

/// Characters that stress Turkish folding, normalization and trimming.
const ALPHABET: &[char] = &[
    'a', 'e', 'i', 'ı', 'I', 'İ', 'ç', 'Ç', 'ğ', 'ş', 'ö', 'ü', '\u{0307}', '\u{0308}', '\u{0327}',
    '0', '1', '5', '9', '\'', '\u{2019}', '`', '.', ',', '…', '"', '(', ')', '@', '-', '_', ' ',
    ' ', ' ', '\t', '\u{00A0}', '\u{FFFD}', '😀',
];

/// `SplitMix64`: tiny, deterministic and good enough to drive input generation.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).unwrap_or(u64::MAX).max(1);
        usize::try_from(self.next() % bound).unwrap_or(0)
    }
}

fn random_bytes(rng: &mut Rng) -> String {
    let len = rng.below(512);
    let bytes: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn random_vocabulary(rng: &mut Rng) -> String {
    let len = rng.below(96);
    (0..len)
        .map(|_| VOCABULARY[rng.below(VOCABULARY.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

fn random_alphabet(rng: &mut Rng) -> String {
    let len = rng.below(128);
    (0..len)
        .map(|_| ALPHABET[rng.below(ALPHABET.len())])
        .collect()
}

fn check(words: &[&str], turn: &NormalizedTurn) {
    let word_count = u32::try_from(words.len()).unwrap_or(u32::MAX);
    assert_eq!(turn.words.len(), words.len());
    assert_eq!(turn.roles.len(), words.len());
    for (raw, word) in words.iter().zip(&turn.words) {
        assert_eq!(
            turkish_fold(&word.folded),
            word.folded,
            "fold not idempotent for {raw:?}"
        );
    }
    for lattice in &turn.lattices {
        assert!(lattice.words.start < lattice.words.end && lattice.words.end <= word_count);
        assert!(!lattice.readings.is_empty() && lattice.readings.len() <= 8);
        for reading in &lattice.readings {
            assert_eq!(reading.digits.len(), reading.source.len());
            assert!(reading.source.iter().all(|w| lattice.words.contains(&w.0)));
            assert!(reading.prior > 0.0 && reading.prior <= 1.0);
        }
    }
    for run in &turn.spelled {
        assert_eq!(run.text.chars().count(), run.source.len());
        assert!(run.words.end <= word_count);
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "long-running; run with `cargo xtask fuzz`"]
fn randomized_inputs_never_break_invariants() {
    let iterations = env_u64("ORTU_STRESS_ITERATIONS", DEFAULT_ITERATIONS);
    let seed = env_u64("ORTU_STRESS_SEED", DEFAULT_SEED);
    let config = NormalizeConfig::default();
    let mut rng = Rng(seed);

    for iteration in 0..iterations {
        let text = match iteration % 3 {
            0 => random_bytes(&mut rng),
            1 => random_vocabulary(&mut rng),
            _ => random_alphabet(&mut rng),
        };
        let words: Vec<&str> = text.split_whitespace().collect();
        let turn = normalize(&words, &config);
        check(&words, &turn);
    }
    println!("{iterations} iterations passed (seed {seed:#x})");
}
