//! Values outside the detection pipeline: values an adopter registers, values
//! assembled from digits spoken across turns
//! ([algorithm 12](https://github.com/aral-by/ortu/blob/main/docs/algorithms/12-cross-turn-assembly.md)),
//! and the kinds that tool fields hold
//! ([algorithm 15](https://github.com/aral-by/ortu/blob/main/docs/algorithms/15-rehydration-and-provenance.md)).

use ortu_core::{EntityKind, FailureReason, SecretValue, ValidatorOutcome};
use ortu_normalize::{flatten, turkish_fold};

use crate::data::FIELD_NAMES;
use crate::iban::validate_iban;
use crate::phone::{canonical_phone, validate_phone};
use crate::tckn::validate_tckn;

const TCKN_DIGITS: usize = 11;

/// Digits of a TR IBAN after the country code.
const IBAN_BODY_DIGITS: usize = 24;

/// A value in canonical form with its validator outcome.
#[derive(Debug, Clone)]
pub struct Assembled {
    /// Digits for a TCKN, E.164 for a phone, `TR` plus digits for an IBAN.
    pub value: SecretValue,
    /// The validator outcome; `Failed(TooShort)` while digits are missing.
    pub outcome: ValidatorOutcome,
}

impl Assembled {
    /// Whether more digits could still complete the value.
    #[must_use]
    pub fn needs_more(&self) -> bool {
        self.outcome == ValidatorOutcome::Failed(FailureReason::TooShort)
    }
}

/// Whether values of `kind` are dictated digit by digit and may be split
/// across turns.
#[must_use]
pub fn assembles(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::Tckn | EntityKind::Iban | EntityKind::Phone
    )
}

/// Interprets digits joined across turns as a value of `kind`. When the
/// digits run past the kind's length, a valid prefix is committed and the rest
/// is ignored. Returns `None` for kinds that are not assembled and for input
/// that is not all digits.
#[must_use]
pub fn assemble(kind: EntityKind, digits: &str) -> Option<Assembled> {
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match kind {
        EntityKind::Tckn => Some(fit(digits, TCKN_DIGITS, |d| {
            (validate_tckn(d) == ValidatorOutcome::Passed).then(|| d.to_owned())
        })),
        EntityKind::Iban => {
            let mut assembled = fit(digits, IBAN_BODY_DIGITS, |d| {
                let iban = format!("TR{d}");
                (validate_iban(&iban) == ValidatorOutcome::Passed).then_some(iban)
            });
            if assembled.outcome != ValidatorOutcome::Passed {
                assembled.value =
                    SecretValue::new(format!("TR{}", assembled.value.expose_secret()));
            }
            Some(assembled)
        }
        EntityKind::Phone => {
            // The prefix the caller started with decides how many digits follow.
            let needed = if digits.starts_with("00") {
                14
            } else if digits.starts_with('0') {
                11
            } else if digits.starts_with("90") {
                12
            } else {
                10
            };
            Some(fit(digits, needed, |d| {
                canonical_phone(d).map(|(e164, _)| e164)
            }))
        }
        _ => None,
    }
}

/// Fits digits to a length: shorter is incomplete, exact is validated, longer
/// commits a valid prefix of that length.
fn fit(digits: &str, len: usize, valid: impl Fn(&str) -> Option<String>) -> Assembled {
    let failed = |reason| Assembled {
        value: SecretValue::new(digits),
        outcome: ValidatorOutcome::Failed(reason),
    };
    if digits.len() < len {
        return failed(FailureReason::TooShort);
    }
    match digits.get(..len).and_then(valid) {
        Some(value) => Assembled {
            value: SecretValue::new(value),
            outcome: ValidatorOutcome::Passed,
        },
        None if digits.len() == len => failed(FailureReason::Checksum),
        None => failed(FailureReason::TooLong),
    }
}

/// The canonical form and validator outcome of a value supplied by an adopter
/// (`register`): digits for a TCKN, E.164 for a phone, compact uppercase for
/// an IBAN, the trimmed value otherwise. Two spellings of one value therefore
/// share a token.
#[must_use]
pub fn canonicalize(kind: EntityKind, value: &SecretValue) -> Assembled {
    let raw = value.expose_secret();
    let digits = || -> String { raw.chars().filter(char::is_ascii_digit).collect() };
    match kind {
        EntityKind::Tckn => {
            let digits = digits();
            let outcome = validate_tckn(&digits);
            Assembled {
                value: SecretValue::new(digits),
                outcome,
            }
        }
        EntityKind::Iban => {
            let compact: String = raw
                .chars()
                .filter(|c| !c.is_whitespace())
                .map(|c| c.to_ascii_uppercase())
                .collect();
            let outcome = validate_iban(&compact);
            Assembled {
                value: SecretValue::new(compact),
                outcome,
            }
        }
        EntityKind::Phone => {
            let digits = digits();
            if let Some((e164, _)) = canonical_phone(&digits) {
                Assembled {
                    value: SecretValue::new(e164),
                    outcome: ValidatorOutcome::Passed,
                }
            } else {
                let outcome = validate_phone(&digits);
                Assembled {
                    value: SecretValue::new(digits),
                    outcome,
                }
            }
        }
        _ => Assembled {
            value: SecretValue::new(raw.trim()),
            outcome: ValidatorOutcome::NotApplicable,
        },
    }
}

/// The kind of personal data a JSON field name announces (`tckn`,
/// `phoneNumber`, `ad_soyad`), if any.
#[must_use]
pub fn field_kind(field: &str) -> Option<EntityKind> {
    let key = snake_case(field);
    FIELD_NAMES
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, kind)| EntityKind::from_config_name(kind))
}

/// `phoneNumber`, `Phone-Number` and `phone number` all become `phone_number`,
/// in flattened form.
fn snake_case(field: &str) -> String {
    let mut out = String::with_capacity(field.len() + 4);
    let mut previous_lower = false;
    for c in field.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && previous_lower {
                out.push('_');
            }
            out.push(c);
            previous_lower = c.is_lowercase() || c.is_numeric();
        } else {
            if !out.is_empty() && !out.ends_with('_') {
                out.push('_');
            }
            previous_lower = false;
        }
    }
    flatten(&turkish_fold(out.trim_end_matches('_')))
}

#[cfg(test)]
mod tests {
    use ortu_core::{EntityKind, FailureReason, SecretValue, ValidatorOutcome};

    use super::{assemble, canonicalize, field_kind};

    fn assembled(kind: EntityKind, digits: &str) -> (String, ValidatorOutcome) {
        let a = assemble(kind, digits).unwrap();
        (a.value.expose_secret().to_owned(), a.outcome)
    }

    #[test]
    fn a_tckn_is_complete_at_eleven_valid_digits() {
        let short = ValidatorOutcome::Failed(FailureReason::TooShort);
        assert_eq!(
            assembled(EntityKind::Tckn, "100000"),
            ("100000".to_owned(), short)
        );
        assert_eq!(
            assembled(EntityKind::Tckn, "10000000146"),
            ("10000000146".to_owned(), ValidatorOutcome::Passed)
        );
        assert_eq!(
            assembled(EntityKind::Tckn, "100000001467"),
            ("10000000146".to_owned(), ValidatorOutcome::Passed),
            "an over-run commits the valid prefix"
        );
        assert_eq!(
            assembled(EntityKind::Tckn, "10000000147").1,
            ValidatorOutcome::Failed(FailureReason::Checksum)
        );
        assert!(assemble(EntityKind::Tckn, "1000a").is_none());
    }

    #[test]
    fn a_phone_needs_the_digits_its_prefix_announces() {
        let short = ValidatorOutcome::Failed(FailureReason::TooShort);
        assert_eq!(assembled(EntityKind::Phone, "0532123456").1, short);
        assert_eq!(
            assembled(EntityKind::Phone, "05321234567"),
            ("+905321234567".to_owned(), ValidatorOutcome::Passed)
        );
        assert_eq!(
            assembled(EntityKind::Phone, "5321234567"),
            ("+905321234567".to_owned(), ValidatorOutcome::Passed)
        );
    }

    #[test]
    fn an_iban_body_has_twenty_four_digits() {
        assert_eq!(
            assembled(EntityKind::Iban, "330006100519786457841326"),
            (
                "TR330006100519786457841326".to_owned(),
                ValidatorOutcome::Passed
            )
        );
        assert_eq!(
            assembled(EntityKind::Iban, "3300061005"),
            (
                "TR3300061005".to_owned(),
                ValidatorOutcome::Failed(FailureReason::TooShort)
            )
        );
        assert!(assemble(EntityKind::FullName, "12").is_none());
    }

    #[test]
    fn registered_spellings_share_a_canonical_form() {
        let phone = canonicalize(EntityKind::Phone, &SecretValue::new("0 (532) 123 45 67"));
        assert_eq!(phone.value.expose_secret(), "+905321234567");
        assert_eq!(phone.outcome, ValidatorOutcome::Passed);
        let iban = canonicalize(
            EntityKind::Iban,
            &SecretValue::new("tr33 0006 1005 1978 6457 8413 26"),
        );
        assert_eq!(iban.value.expose_secret(), "TR330006100519786457841326");
        let name = canonicalize(EntityKind::FullName, &SecretValue::new("  Ahmet Yılmaz "));
        assert_eq!(name.value.expose_secret(), "Ahmet Yılmaz");
        assert_eq!(name.outcome, ValidatorOutcome::NotApplicable);
    }

    #[test]
    fn field_names_in_any_casing_map_to_kinds() {
        assert_eq!(field_kind("tckn"), Some(EntityKind::Tckn));
        assert_eq!(field_kind("TC_Kimlik_No"), Some(EntityKind::Tckn));
        assert_eq!(field_kind("phoneNumber"), Some(EntityKind::Phone));
        assert_eq!(field_kind("Müşteri Adı"), Some(EntityKind::FullName));
        assert_eq!(field_kind("soyadı"), Some(EntityKind::PersonSurname));
        assert_eq!(field_kind("debt"), None);
    }
}
