//! Randomized stress test for sessions (ADR-0005): seeded random calls through
//! Realtime, rehydration, the tool boundary and `PostCall`. Ignored by default;
//! run it with `cargo xtask fuzz`. `ORTU_STRESS_ITERATIONS` (turns) and
//! `ORTU_STRESS_SEED` work as in `ortu-normalize`.
//!
//! Invariants:
//! - `push` and `finish` never panic, and every part and span they report lies
//!   inside its turn;
//! - every token in Realtime output resolves in the session (`rehydrate`
//!   succeeds), so nothing the LLM receives is unresolvable;
//! - streaming rehydration at random chunk boundaries equals rehydrating the
//!   whole text;
//! - the tool boundary never panics on masked output.

#![allow(
    clippy::unwrap_used,
    reason = "a stress test fails on any setup error; allow-unwrap-in-tests covers #[test] functions only"
)]

use ortu_core::{Speaker, Turn};
use ortu_crypto::{SecretKey, StaticKeyProvider};
use ortu_stream::{Engine, EntityReport, Session};
use serde_json::json;

const DEFAULT_ITERATIONS: u64 = 1_000_000;
const DEFAULT_SEED: u64 = 0x6f72_7475; // "ortu"

/// Turns of one random call before it is finished and a new one starts.
const TURNS_PER_CALL: u64 = 8;

/// Spoken and written forms, cues, backchannels and questions.
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
    "kırk",
    "yüz",
    "0532",
    "123",
    "45",
    "67",
    "10000000146",
    "TC:10000000146",
    "+905321234567",
    "0532-123-45-67",
    "TR330006100519786457841326",
    "TR33",
    "te",
    "re",
    "TC'm",
    "kimlik",
    "numaram",
    "IBAN'ım",
    "telefonum",
    "adım",
    "Ahmet",
    "Yılmaz",
    "Bey",
    "evet",
    "hı hı",
    "tamam",
    "ııı",
    "bir saniye",
    "yok yok",
    "TC kimlik numaranız?",
    "adınız soyadınız?",
    ",",
    "[TCKN:00000000]",
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
    let len = rng.below(16);
    (0..len)
        .map(|_| VOCABULARY[rng.below(VOCABULARY.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

fn check_parts(entity: &EntityReport, turns: &[Turn]) {
    assert_ne!(entity.parts, []);
    for part in &entity.parts {
        let turn = turns.iter().find(|t| t.index == part.turn).unwrap();
        let count = u32::try_from(turn.words.len()).unwrap();
        assert!(part.words.start < part.words.end && part.words.end <= count);
    }
    assert!(entity.span.turn_start <= entity.span.turn_end);
    assert!((0.0..=1.0).contains(&entity.score));
    assert_ne!(entity.replacement, "");
}

fn streamed(session: &Session, text: &str, rng: &mut Rng) -> Option<String> {
    let mut rehydrator = session.rehydrator();
    let mut out = String::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut cut = rng.below(rest.len() + 1).max(1);
        while !rest.is_char_boundary(cut) {
            cut += 1;
        }
        out.push_str(rehydrator.feed(&rest[..cut]).ok()?.expose_secret());
        rest = &rest[cut..];
    }
    out.push_str(rehydrator.finish().ok()?.expose_secret());
    Some(out)
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[test]
#[ignore = "long-running; run with `cargo xtask fuzz`"]
fn randomized_sessions_never_break_invariants() {
    let iterations = env_u64("ORTU_STRESS_ITERATIONS", DEFAULT_ITERATIONS);
    let seed = env_u64("ORTU_STRESS_SEED", DEFAULT_SEED);
    let engine = Engine::builder()
        .key_provider(StaticKeyProvider::single(SecretKey::from_bytes([3; 32])))
        .build()
        .unwrap();
    let mut rng = Rng(seed);
    let mut session = engine.session("call-0", "tenant").unwrap();
    let mut turns: Vec<Turn> = Vec::new();
    for iteration in 0..iterations {
        let index = u32::try_from(iteration % TURNS_PER_CALL).unwrap();
        let speaker = if rng.below(3) == 0 {
            Speaker::Bot
        } else {
            Speaker::Caller
        };
        let turn = Turn::from_text(index, speaker, &random_text(&mut rng), 0, 1000);
        turns.push(turn.clone());
        let masked = session.push(turn).unwrap();
        for entity in &masked.entities {
            check_parts(entity, &turns);
        }
        // Tokens the session issued always resolve; only the planted unknown
        // token `[TCKN:00000000]` may fail, and then streaming fails too.
        let whole = session
            .rehydrate(&masked.text)
            .map(|r| r.text.expose_secret().to_owned());
        if !masked.text.contains("00000000") {
            assert!(whole.is_ok(), "unresolvable token in {:?}", masked.text);
        }
        assert_eq!(streamed(&session, &masked.text, &mut rng), whole.ok());
        let _ = session.guard_tool_args("tool", &json!({ "text": masked.text }));

        if index + 1 == u32::try_from(TURNS_PER_CALL).unwrap() {
            let finished = std::mem::replace(
                &mut session,
                engine
                    .session(format!("call-{iteration}"), "tenant")
                    .unwrap(),
            );
            let post = finished.finish();
            assert_eq!(post.turns.len(), turns.len());
            for entity in &post.entities {
                check_parts(entity, &turns);
            }
            turns.clear();
        }
    }
    println!("{iterations} iterations passed (seed {seed:#x})");
}
