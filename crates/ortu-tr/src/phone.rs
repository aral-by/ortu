//! Turkish phone numbers
//! ([algorithm 08](https://github.com/aral-by/ortu/blob/main/docs/algorithms/08-phone-and-plate-detection.md)).

use ortu_core::{
    Candidate, DetectContext, Detector, EntityKind, FailureReason, SecretValue, Validator,
    ValidatorOutcome,
};

use crate::data::AREA_CODES;
use crate::digits::aligned_windows;

/// National number length after the prefix.
const NATIONAL: usize = 10;

/// What a national number belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneClass {
    /// Any `5xx`. The 2018 plan assigns 501, 505-507, 530-549, 551-555 and
    /// 559 to mobile operators, 510, 516 and 524 to MVNOs and 592 to satellite
    /// mobile; accepting the whole `5xx` block also covers later allocations
    /// (recall-first). Non-personal ranges (800, 811, 812, 850, 888, 898, 900)
    /// are not accepted.
    Mobile,
    /// A geographic area code.
    Landline,
    /// `444 xx xx`: organizations, not people.
    Corporate,
}

/// Strips a `0090`, `90` or `0` prefix and classifies the national number.
/// Returns the canonical E.164 form (`+905321234567`) and the class.
#[must_use]
pub fn canonical_phone(digits: &str) -> Option<(String, PhoneClass)> {
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let national = match digits.len() {
        14 => digits.strip_prefix("0090")?,
        12 => digits.strip_prefix("90")?,
        11 => digits.strip_prefix('0')?,
        NATIONAL => digits,
        _ => return None,
    };
    let class = if national.starts_with('5') {
        PhoneClass::Mobile
    } else if national.starts_with("444") {
        PhoneClass::Corporate
    } else if national
        .get(..3)
        .and_then(|code| code.parse::<u16>().ok())
        .is_some_and(|code| AREA_CODES.contains(&code))
    {
        PhoneClass::Landline
    } else {
        return None;
    };
    Some((format!("+90{national}"), class))
}

/// Validates a phone number given as digits or in E.164 form.
#[must_use]
pub fn validate_phone(value: &str) -> ValidatorOutcome {
    let digits = value.strip_prefix('+').unwrap_or(value);
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return ValidatorOutcome::Failed(FailureReason::Format);
    }
    match digits.len() {
        len if len < NATIONAL => ValidatorOutcome::Failed(FailureReason::TooShort),
        len if len > NATIONAL + 4 => ValidatorOutcome::Failed(FailureReason::TooLong),
        _ if canonical_phone(digits).is_some() => ValidatorOutcome::Passed,
        _ => ValidatorOutcome::Failed(FailureReason::Format),
    }
}

/// [`Validator`] for [`EntityKind::Phone`].
#[derive(Debug, Clone, Copy, Default)]
pub struct PhoneValidator;

impl Validator for PhoneValidator {
    fn validate(&self, _kind: EntityKind, value: &str) -> ValidatorOutcome {
        validate_phone(value)
    }
}

/// Finds mobile and landline numbers in digit lattices. Corporate `444`
/// numbers are not personal data and are skipped unless configured otherwise.
#[derive(Debug, Clone, Copy, Default)]
pub struct PhoneDetector {
    /// Also mask `444 xx xx` numbers.
    pub mask_corporate: bool,
}

impl Detector for PhoneDetector {
    fn id(&self) -> &'static str {
        "tr.phone"
    }

    fn kinds(&self) -> &'static [EntityKind] {
        &[EntityKind::Phone]
    }

    fn detect(&self, ctx: &DetectContext<'_>, out: &mut Vec<Candidate>) {
        for lattice in &ctx.normalized.lattices {
            let mut found: Vec<Candidate> = Vec::new();
            for reading in &lattice.readings {
                for len in [14, 12, 11, NATIONAL] {
                    for window in aligned_windows(reading, len) {
                        let Some((value, class)) = canonical_phone(window.digits) else {
                            continue;
                        };
                        if class == PhoneClass::Corporate && !self.mask_corporate {
                            continue;
                        }
                        // The same number with and without its prefix: keep the wider span.
                        let same = found.iter().position(|c| {
                            c.value.expose_secret() == value
                                && c.words.start < window.words.end
                                && window.words.start < c.words.end
                        });
                        if let Some(i) = same {
                            let wider = window.words.end - window.words.start
                                > found[i].words.end - found[i].words.start;
                            if !wider {
                                continue;
                            }
                            found.remove(i);
                        }
                        found.push(Candidate {
                            kind: EntityKind::Phone,
                            detector: self.id(),
                            words: window.words,
                            value: SecretValue::new(value),
                            validator: ValidatorOutcome::Passed,
                            prior: reading.prior,
                            trace: Some(reading.trace.clone()),
                            cues: Vec::new(),
                            score_floor: None,
                        });
                    }
                }
            }
            out.extend(found);
        }
    }
}

#[cfg(test)]
mod tests {
    use ortu_core::ValidatorOutcome;

    use super::{PhoneClass, canonical_phone, validate_phone};

    #[test]
    fn every_prefix_style_canonicalizes_to_e164() {
        for digits in [
            "5321234567",
            "05321234567",
            "905321234567",
            "00905321234567",
        ] {
            assert_eq!(
                canonical_phone(digits),
                Some(("+905321234567".to_owned(), PhoneClass::Mobile)),
                "{digits}"
            );
        }
        assert_eq!(
            canonical_phone("03121234567"),
            Some(("+903121234567".to_owned(), PhoneClass::Landline))
        );
        assert_eq!(
            canonical_phone("4441234"),
            None,
            "seven digits are not a national number"
        );
        assert_eq!(
            canonical_phone("4440123456").map(|(_, class)| class),
            Some(PhoneClass::Corporate)
        );
    }

    #[test]
    fn ranges_of_the_2018_numbering_plan() {
        // Mobile, MVNO and satellite mobile ranges are personal numbers.
        for ndc in [
            "501", "530", "549", "551", "559", "510", "516", "524", "592",
        ] {
            let digits = format!("{ndc}1234567");
            assert_eq!(
                canonical_phone(&digits).map(|(_, class)| class),
                Some(PhoneClass::Mobile),
                "{ndc}"
            );
        }
        // Freephone, two-stage access, nomadic, value-added and Northern Cyprus
        // numbers are not geographic or mobile numbers of Turkey's plan.
        for ndc in ["800", "811", "812", "850", "888", "898", "900", "392"] {
            assert_eq!(canonical_phone(&format!("{ndc}1234567")), None, "{ndc}");
        }
    }

    #[test]
    fn unknown_area_codes_and_odd_lengths_are_rejected() {
        for digits in [
            "09991234567",
            "1234567890",
            "532123456",
            "053212345678",
            "+905321234567",
        ] {
            assert_eq!(canonical_phone(digits), None, "{digits}");
        }
        assert_eq!(validate_phone("+905321234567"), ValidatorOutcome::Passed);
        assert_ne!(validate_phone("+909991234567"), ValidatorOutcome::Passed);
    }
}
