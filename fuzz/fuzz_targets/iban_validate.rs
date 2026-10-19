//! Arbitrary strings through the IBAN validator: no panic, deterministic, and
//! a passed IBAN always has the structure of a TR IBAN.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ortu_core::ValidatorOutcome;
use ortu_tr::validate_iban;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let outcome = validate_iban(&text);
    assert_eq!(outcome, validate_iban(&text));
    if outcome == ValidatorOutcome::Passed {
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(compact.len(), 26);
        assert!(compact.to_ascii_uppercase().starts_with("TR"));
    }
});
