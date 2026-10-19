//! Randomized stress test for detection (ADR-0005): the validators and the
//! full analyzer on seeded random input. Ignored by default; run it with
//! `cargo xtask fuzz`. `ORTU_STRESS_ITERATIONS` and `ORTU_STRESS_SEED` work as
//! in `ortu-normalize`.

use ortu_core::{Analyzer, Policy, SessionHints, Speaker, Turn, TurnAnalysis, Weights};
use ortu_tr::{canonical_phone, validate_iban, validate_phone, validate_tckn};

const DEFAULT_ITERATIONS: u64 = 1_000_000;
const DEFAULT_SEED: u64 = 0x6f72_7475; // "ortu"

/// Words that reach every detector: digits, number words, cues, introducers,
/// honorifics, letters, fillers and corrections.
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
    "yüz",
    "bin",
    "0532",
    "123",
    "45",
    "67",
    "0",
    "90",
    "0090",
    "444",
    "TR33",
    "TR",
    "te",
    "re",
    "a",
    "be",
    "TC'm",
    "kimlik",
    "numaram",
    "IBAN'ım",
    "hesap",
    "telefonum",
    "cep",
    "adım",
    "ismim",
    "soyadım",
    "ad",
    "soyad",
    "Ahmet",
    "Yılmaz",
    "Bey",
    "Hanım",
    "Müdür",
    "ııı",
    "şey",
    "bir saniye",
    "yok yok",
    "pardon",
    ",",
    "...",
    "ve",
];

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

fn random_text(rng: &mut Rng) -> String {
    let len = rng.below(48);
    (0..len)
        .map(|_| VOCABULARY[rng.below(VOCABULARY.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

fn random_digits(rng: &mut Rng) -> String {
    let len = rng.below(30);
    (0..len)
        .map(|_| char::from(b'0' + u8::try_from(rng.below(10)).unwrap_or(0)))
        .collect()
}

fn check(turn: &Turn, analysis: &TurnAnalysis) {
    let word_count = u32::try_from(turn.words.len()).unwrap_or(u32::MAX);
    let text_len = u32::try_from(turn.text().len()).unwrap_or(u32::MAX);
    for entity in &analysis.entities {
        assert!(entity.words.start < entity.words.end && entity.words.end <= word_count);
        assert!(entity.span.byte_start <= entity.span.byte_end && entity.span.byte_end <= text_len);
        assert!((0.0..=1.0).contains(&entity.score));
        assert_ne!(entity.value.expose_secret(), "");
        for word in entity.words.clone() {
            assert!(
                analysis.masked_words.contains(&word),
                "entity word not masked"
            );
        }
    }
    for pair in analysis.entities.windows(2) {
        assert!(pair[0].words.end <= pair[1].words.start, "entities overlap");
    }
    assert!(analysis.masked_words.iter().all(|w| *w < word_count));
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "long-running; run with `cargo xtask fuzz`"]
fn randomized_detection_never_breaks_invariants() {
    let iterations = env_u64("ORTU_STRESS_ITERATIONS", DEFAULT_ITERATIONS);
    let seed = env_u64("ORTU_STRESS_SEED", DEFAULT_SEED);
    let analyzer = Analyzer::new(
        ortu_tr::default_detectors(),
        ortu_tr::default_cue_rules(),
        Weights::default(),
        Policy::default(),
    );
    let mut rng = Rng(seed);
    for iteration in 0..iterations {
        if iteration % 2 == 0 {
            let digits = random_digits(&mut rng);
            let _ = validate_tckn(&digits);
            let _ = validate_iban(&format!("TR{digits}"));
            let _ = validate_phone(&digits);
            if let Some((e164, _)) = canonical_phone(&digits) {
                assert!(e164.starts_with("+90") && e164.len() == 13);
            }
            continue;
        }
        let speaker = if rng.below(4) == 0 {
            Speaker::Bot
        } else {
            Speaker::Caller
        };
        let turn = Turn::from_text(0, speaker, &random_text(&mut rng), 0, 1000);
        let hints = SessionHints {
            expecting: (rng.below(3) == 0).then_some(ortu_core::EntityKind::Tckn),
            ..SessionHints::default()
        };
        let analysis = analyzer.analyze_turn(&turn, &hints, 0);
        check(&turn, &analysis);
    }
    println!("{iterations} iterations passed (seed {seed:#x})");
}
