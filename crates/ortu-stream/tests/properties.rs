//! The properties of algorithm 15 (docs/algorithms/15, "Tests and
//! invariants"):
//!
//! 1. A corrupted token resolves to its own value or fails; never to another.
//! 2. A valid TCKN, IBAN or phone number that is not in the session map is
//!    rejected in tool arguments.
//! 3. Streaming rehydration over any chunking equals rehydrating the whole
//!    text.
//!
//! Identifier generators below are written from the published algorithms,
//! independently of `ortu-tr`.

#![allow(
    clippy::unwrap_used,
    reason = "test helpers fail the test on setup errors; allow-unwrap-in-tests covers #[test] functions only"
)]

use ortu_core::{EntityKind, SecretValue};
use ortu_crypto::{SecretKey, StaticKeyProvider};
use ortu_stream::{Engine, GuardError, Session};
use proptest::prelude::*;
use serde_json::json;

const VALUES: [(EntityKind, &str); 4] = [
    (EntityKind::Tckn, "10000000146"),
    (EntityKind::Phone, "+905321234567"),
    (EntityKind::FullName, "Ahmet Yılmaz"),
    (EntityKind::Iban, "TR330006100519786457841326"),
];

fn session() -> (Session, Vec<String>) {
    let engine = Engine::builder()
        .key_provider(StaticKeyProvider::single(SecretKey::from_bytes([9; 32])))
        .build()
        .unwrap();
    let mut session = engine.session("call", "tenant").unwrap();
    let tokens = VALUES
        .iter()
        .map(|(kind, value)| {
            session
                .register(*kind, &SecretValue::new(*value))
                .to_string()
        })
        .collect();
    (session, tokens)
}

/// A token with the corruptions LLMs produce: case changes, a dropped
/// bracket, spaces around the colon, and possibly one altered digest digit.
#[derive(Debug, Clone)]
struct Corruption {
    upper: bool,
    brackets: (bool, bool),
    spaces: (usize, usize),
    altered: Option<(usize, u8)>,
}

fn corruption() -> impl Strategy<Value = Corruption> {
    (
        any::<bool>(),
        (any::<bool>(), any::<bool>()),
        (0usize..=3, 0usize..=3),
        prop::option::of((0usize..8, 1u8..16)),
    )
        .prop_map(|(upper, brackets, spaces, altered)| Corruption {
            upper,
            brackets,
            spaces,
            altered,
        })
}

fn corrupt(token: &str, c: &Corruption) -> (String, bool) {
    let inner = token.trim_start_matches('[').trim_end_matches(']');
    let (label, digest) = inner.split_once(':').unwrap();
    let mut digest: Vec<u8> = digest.bytes().collect();
    let mut changed = false;
    if let Some((position, delta)) = c.altered {
        let value = u8::try_from(char::from(digest[position]).to_digit(16).unwrap()).unwrap();
        let new = (value + delta) % 16;
        digest[position] = b"0123456789abcdef"[usize::from(new)];
        changed = true;
    }
    let mut digest = String::from_utf8(digest).unwrap();
    let label = if c.upper {
        digest = digest.to_ascii_uppercase();
        label.to_ascii_lowercase()
    } else {
        label.to_owned()
    };
    let text = format!(
        "{}{label}{}:{}{digest}{}",
        if c.brackets.0 { "[" } else { "" },
        " ".repeat(c.spaces.0),
        " ".repeat(c.spaces.1),
        if c.brackets.1 { "]" } else { "" },
    );
    (text, changed)
}

fn tckn(first9: &[u8]) -> String {
    let d: Vec<i32> = first9.iter().map(|&b| i32::from(b)).collect();
    let tenth =
        ((d[0] + d[2] + d[4] + d[6] + d[8]) * 7 - (d[1] + d[3] + d[5] + d[7])).rem_euclid(10);
    let eleventh = (d.iter().sum::<i32>() + tenth).rem_euclid(10);
    let mut out: String = first9.iter().map(|d| char::from(b'0' + d)).collect();
    out.push(char::from(b'0' + u8::try_from(tenth).unwrap()));
    out.push(char::from(b'0' + u8::try_from(eleventh).unwrap()));
    out
}

fn iban(bban: &str) -> String {
    // ISO 13616: move "TR00" to the end, letters as numbers (T = 29, R = 27).
    let remainder = format!("{bban}292700")
        .bytes()
        .fold(0u32, |r, b| (r * 10 + u32::from(b - b'0')) % 97);
    format!("TR{:02}{bban}", 98 - remainder)
}

fn identifier() -> impl Strategy<Value = (EntityKind, String)> {
    prop_oneof![
        (1u8..10, prop::collection::vec(0u8..10, 8)).prop_map(|(first, rest)| {
            let mut digits = vec![first];
            digits.extend(rest);
            (EntityKind::Tckn, tckn(&digits))
        }),
        "[0-9]{24}".prop_map(|body| (EntityKind::Iban, iban(&body[2..]))),
        "5[0-9]{9}".prop_map(|national| (EntityKind::Phone, format!("0{national}"))),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn corrupted_tokens_resolve_to_their_own_value_or_fail(
        which in 0usize..VALUES.len(),
        c in corruption(),
    ) {
        let (session, tokens) = session();
        let (text, changed) = corrupt(&tokens[which], &c);
        match session.guard_tool_args("t", &json!({ "v": text })) {
            Ok(released) => {
                prop_assert!(!changed, "an altered digest resolved: {text}");
                prop_assert_eq!(released["v"].as_str(), Some(VALUES[which].1));
            }
            Err(error) => {
                prop_assert!(changed, "an intact token failed: {text}: {error}");
                let is_unknown = matches!(error, GuardError::UnknownToken { .. });
                prop_assert!(is_unknown, "{}", error);
            }
        }
    }

    #[test]
    fn identifiers_not_in_the_map_are_rejected((kind, value) in identifier()) {
        let (session, _) = session();
        prop_assume!(value != "10000000146" && value != "05321234567");
        let result = session.guard_tool_args("t", &json!({ "arg": value }));
        prop_assert_eq!(result, Err(GuardError::UnknownValue { kind }));
    }

    #[test]
    fn streaming_equals_whole_text_rehydration(
        pieces in prop::collection::vec(0usize..16, 0..12),
        cuts in prop::collection::vec(0usize..200, 0..8),
    ) {
        let (session, tokens) = session();
        // Tokens as issued and without brackets, an unknown token, prefixes of
        // tokens, and a letter that glues to an unbracketed token.
        let bare: Vec<String> = tokens
            .iter()
            .map(|t| t.trim_start_matches('[').trim_end_matches(']').replace(':', " "))
            .collect();
        let words = [
            "Merhaba ", "Sayın ", ", ", "[", "TCK", " teşekkürler.", "[NAME", "ADDRESS ", "x",
            "[PHONE:00000000]",
        ];
        let text: String = pieces
            .iter()
            .map(|&p| match p {
                0..4 => tokens[p].clone(),
                4..6 => bare[p - 4].clone(),
                _ => words[p - 6].to_owned(),
            })
            .collect();
        let whole = session.rehydrate(&text).map(|r| r.text.expose_secret().to_owned());

        let mut boundaries: Vec<usize> = cuts
            .iter()
            .filter_map(|&cut| text.char_indices().map(|(i, _)| i).nth(cut))
            .collect();
        boundaries.sort_unstable();
        boundaries.dedup();
        let mut rehydrator = session.rehydrator();
        let mut streamed = String::new();
        let mut failed = false;
        let mut last = 0;
        for &cut in boundaries.iter().chain(std::iter::once(&text.len())) {
            if let Ok(out) = rehydrator.feed(&text[last..cut]) {
                streamed.push_str(out.expose_secret());
            } else {
                failed = true;
                break;
            }
            last = cut;
        }
        if !failed {
            match rehydrator.finish() {
                Ok(out) => streamed.push_str(out.expose_secret()),
                Err(_) => failed = true,
            }
        }
        match whole {
            Ok(expected) => {
                prop_assert!(!failed, "streaming failed on {text:?}");
                prop_assert_eq!(streamed, expected);
            }
            Err(_) => prop_assert!(failed, "streaming succeeded on {text:?}"),
        }
    }
}

#[test]
fn the_generators_produce_valid_identifiers() {
    assert_eq!(tckn(&[1, 0, 0, 0, 0, 0, 0, 0, 1]), "10000000146");
    assert_eq!(iban("0006100519786457841326"), "TR330006100519786457841326");
}
