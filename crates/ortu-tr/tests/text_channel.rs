//! Written text: chat messages, forms and tool payloads. Values arrive as
//! digits, compact IBANs and formatted phone numbers rather than spoken words,
//! often glued to punctuation or a label.

use ortu_core::{
    Analyzer, EntityKind, FailureReason, Policy, SessionHints, Speaker, Turn, ValidatorOutcome,
    Weights,
};

fn values(text: &str, kind: EntityKind) -> Vec<String> {
    let analyzer = Analyzer::new(
        ortu_tr::default_detectors(),
        ortu_tr::default_cue_rules(),
        Weights::default(),
        Policy::default(),
    );
    let turn = Turn::from_text(0, Speaker::Unknown, text, 0, 1000);
    analyzer
        .analyze_turn(&turn, &SessionHints::default(), 0)
        .entities
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.value.expose_secret().to_owned())
        .collect()
}

/// Records `text` unless exactly `expected` is found.
fn check(text: &'static str, kind: EntityKind, expected: &str, missed: &mut Vec<&'static str>) {
    if values(text, kind) != [expected] {
        missed.push(text);
    }
}

#[test]
fn written_tckns() {
    let mut missed = Vec::new();
    for text in [
        "10000000146",
        "TC: 10000000146",
        "TC:10000000146",
        "tckn=10000000146",
        "kimlik no 10000000146.",
        "(10000000146)",
        "\"10000000146\"",
        "TC'm 10000000146'dır",
    ] {
        check(text, EntityKind::Tckn, "10000000146", &mut missed);
    }
    assert!(missed.is_empty(), "missed: {missed:#?}");
}

#[test]
fn written_ibans() {
    let mut missed = Vec::new();
    for text in [
        "TR330006100519786457841326",
        "tr330006100519786457841326",
        "IBAN: TR330006100519786457841326",
        "IBAN:TR330006100519786457841326",
        "TR33 0006 1005 1978 6457 8413 26",
        "IBAN TR33 0006 1005 1978 6457 8413 26.",
        "hesap: TR33-0006-1005-1978-6457-8413-26",
        "TR330006100519786457841326'ya gönderin",
    ] {
        check(
            text,
            EntityKind::Iban,
            "TR330006100519786457841326",
            &mut missed,
        );
    }
    assert!(missed.is_empty(), "missed: {missed:#?}");
}

#[test]
fn a_written_iban_with_a_wrong_check_digit_is_masked_as_invalid() {
    let analyzer = Analyzer::new(
        ortu_tr::default_detectors(),
        ortu_tr::default_cue_rules(),
        Weights::default(),
        Policy::default(),
    );
    let turn = Turn::from_text(0, Speaker::Unknown, "TR340006100519786457841326", 0, 1000);
    let analysis = analyzer.analyze_turn(&turn, &SessionHints::default(), 0);
    assert_eq!(analysis.entities.len(), 1);
    assert_eq!(analysis.entities[0].kind, EntityKind::Iban);
    assert_eq!(
        analysis.entities[0].evidence.validator,
        ValidatorOutcome::Failed(FailureReason::Checksum)
    );
}

#[test]
fn written_phones() {
    let mut missed = Vec::new();
    for text in [
        "+905321234567",
        "+90 532 123 45 67",
        "0532 123 45 67",
        "05321234567",
        "(0532) 123 45 67",
        "0532-123-45-67",
        "tel: 0532 123 4567",
        "telefon:05321234567",
    ] {
        check(text, EntityKind::Phone, "+905321234567", &mut missed);
    }
    assert!(missed.is_empty(), "missed: {missed:#?}");
}
