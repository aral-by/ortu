//! TCKN: the Turkish national identity number
//! ([algorithm 04](https://github.com/aral-by/ortu/blob/main/docs/algorithms/04-tckn-validation.md)).

use ortu_core::{
    Candidate, DetectContext, Detector, EntityKind, FailureReason, SecretValue, Validator,
    ValidatorOutcome,
};

use crate::digits::{aligned_windows, push_unique};

const LENGTH: usize = 11;

/// Score floor for a partial TCKN after a cue (section 7.2).
const PARTIAL_FLOOR: f32 = 0.7;

/// Validates an 11-digit TCKN: first digit non-zero, then the two check digits.
#[must_use]
pub fn validate_tckn(value: &str) -> ValidatorOutcome {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return ValidatorOutcome::Failed(FailureReason::Format);
    }
    match value.len() {
        len if len < LENGTH => return ValidatorOutcome::Failed(FailureReason::TooShort),
        len if len > LENGTH => return ValidatorOutcome::Failed(FailureReason::TooLong),
        _ => {}
    }
    let d: Vec<i32> = value.bytes().map(|b| i32::from(b - b'0')).collect();
    if d[0] == 0 {
        return ValidatorOutcome::Failed(FailureReason::Format);
    }
    // The intermediate value can be negative; `rem_euclid` keeps the result in 0..10.
    let tenth =
        ((d[0] + d[2] + d[4] + d[6] + d[8]) * 7 - (d[1] + d[3] + d[5] + d[7])).rem_euclid(10);
    let eleventh = d[..10].iter().sum::<i32>().rem_euclid(10);
    if d[9] == tenth && d[10] == eleventh {
        ValidatorOutcome::Passed
    } else {
        ValidatorOutcome::Failed(FailureReason::Checksum)
    }
}

/// [`Validator`] for [`EntityKind::Tckn`].
#[derive(Debug, Clone, Copy, Default)]
pub struct TcknValidator;

impl Validator for TcknValidator {
    fn validate(&self, _kind: EntityKind, value: &str) -> ValidatorOutcome {
        validate_tckn(value)
    }
}

/// Finds TCKNs in digit lattices: every 11-digit word-aligned window of every
/// reading that validates, and, after a cue, a 9 to 12 digit run that does not
/// (STT dropped or added a digit; the caller still meant to disclose it).
#[derive(Debug, Clone, Copy, Default)]
pub struct TcknDetector;

impl Detector for TcknDetector {
    fn id(&self) -> &'static str {
        "tr.tckn"
    }

    fn kinds(&self) -> &'static [EntityKind] {
        &[EntityKind::Tckn]
    }

    fn detect(&self, ctx: &DetectContext<'_>, out: &mut Vec<Candidate>) {
        for lattice in &ctx.normalized.lattices {
            let mut found = false;
            for reading in &lattice.readings {
                for window in aligned_windows(reading, LENGTH) {
                    if validate_tckn(window.digits) == ValidatorOutcome::Passed {
                        found = true;
                        push_unique(
                            out,
                            Candidate {
                                kind: EntityKind::Tckn,
                                detector: self.id(),
                                words: window.words,
                                value: SecretValue::new(window.digits),
                                validator: ValidatorOutcome::Passed,
                                prior: reading.prior,
                                trace: Some(reading.trace.clone()),
                                cues: Vec::new(),
                                score_floor: None,
                            },
                        );
                    }
                }
            }
            let Some(best) = lattice.readings.first() else {
                continue;
            };
            let partial = !found
                && (LENGTH - 2..=LENGTH + 1).contains(&best.digits.len())
                && ctx.has_cue(EntityKind::Tckn, &lattice.words);
            if partial {
                out.push(Candidate {
                    kind: EntityKind::Tckn,
                    detector: self.id(),
                    words: lattice.words.clone(),
                    value: SecretValue::new(best.digits.clone()),
                    validator: validate_tckn(&best.digits),
                    prior: best.prior,
                    trace: Some(best.trace.clone()),
                    cues: Vec::new(),
                    score_floor: Some(PARTIAL_FLOOR),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ortu_core::{FailureReason, ValidatorOutcome};

    use super::validate_tckn;

    // Generated and checked by an independent Python implementation of the
    // published algorithm (see the M2 commit); never derived from this code.
    const VALID: [&str; 22] = [
        "10000000146",
        "19090909018", // negative intermediate value (-29) before rem_euclid
        "61378440508",
        "30847485166",
        "26437126540",
        "18542915904",
        "19605579484",
        "94549127786",
        "23360549316",
        "19364150818",
        "72994818480",
        "45715249580",
        "44964050800",
        "68728167498",
        "83943531084",
        "62144432376",
        "68232780770",
        "22954005108",
        "21016720874",
        "88695887458",
        "37352725824",
        "69112098398",
    ];

    const INVALID: [(&str, ValidatorOutcome); 20] = [
        (
            "61378440509",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "30847485167",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "26437126541",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "18542915905",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "19605579485",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "94549127787",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "23360549317",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "19364150819",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "72994818481",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "45715249581",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "44964050830",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "68728167428",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "83943531014",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "62144432306",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "68232780700",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
        (
            "02954005108",
            ValidatorOutcome::Failed(FailureReason::Format),
        ),
        (
            "2101672087",
            ValidatorOutcome::Failed(FailureReason::TooShort),
        ),
        (
            "886958874581",
            ValidatorOutcome::Failed(FailureReason::TooLong),
        ),
        (
            "00000000000",
            ValidatorOutcome::Failed(FailureReason::Format),
        ),
        (
            "12345678901",
            ValidatorOutcome::Failed(FailureReason::Checksum),
        ),
    ];

    #[test]
    fn independent_vectors_validate() {
        for value in VALID {
            assert_eq!(validate_tckn(value), ValidatorOutcome::Passed, "{value}");
        }
    }

    #[test]
    fn independent_negative_vectors_fail_with_the_right_reason() {
        for (value, expected) in INVALID {
            assert_eq!(validate_tckn(value), expected, "{value}");
        }
        assert_eq!(
            validate_tckn("1000000014a"),
            ValidatorOutcome::Failed(FailureReason::Format)
        );
    }

    #[test]
    fn any_single_digit_change_breaks_a_valid_tckn() {
        for value in VALID {
            for position in 1..11 {
                let mut digits: Vec<u8> = value.bytes().collect();
                digits[position] = b'0' + (digits[position] - b'0' + 1) % 10;
                let changed = String::from_utf8(digits).unwrap();
                assert_ne!(
                    validate_tckn(&changed),
                    ValidatorOutcome::Passed,
                    "{changed}"
                );
            }
        }
    }
}
