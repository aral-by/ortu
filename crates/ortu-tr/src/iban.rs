//! TR IBAN
//! ([algorithm 06](https://github.com/aral-by/ortu/blob/main/docs/algorithms/06-iban-validation.md)).

use ortu_core::{
    Candidate, DetectContext, Detector, EntityKind, FailureReason, SecretValue, Validator,
    ValidatorOutcome,
};
use ortu_normalize::DigitLattice;

use crate::digits::{aligned_windows, push_unique};

/// `TR` + 2 check digits + 5-digit bank code + 1 reserved digit + 16-digit account.
const LENGTH: usize = 26;
const BODY_DIGITS: usize = LENGTH - 2;

/// With a cue, at least this many digits are masked even without a valid
/// checksum (section 7.2).
const PARTIAL_MIN_DIGITS: usize = 16;
const PARTIAL_FLOOR: f32 = 0.7;

const ID: &str = "tr.iban";

/// Validates a TR IBAN with the ISO 13616 mod-97 check. Spaces and case are
/// ignored.
#[must_use]
pub fn validate_iban(value: &str) -> ValidatorOutcome {
    let compact: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if !compact.starts_with("TR") || !compact.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return ValidatorOutcome::Failed(FailureReason::Format);
    }
    match compact.len() {
        len if len < LENGTH => return ValidatorOutcome::Failed(FailureReason::TooShort),
        len if len > LENGTH => return ValidatorOutcome::Failed(FailureReason::TooLong),
        _ => {}
    }
    if !compact.bytes().skip(2).all(|b| b.is_ascii_digit()) {
        return ValidatorOutcome::Failed(FailureReason::Format);
    }
    if mod97(&compact) == 1 {
        ValidatorOutcome::Passed
    } else {
        ValidatorOutcome::Failed(FailureReason::Checksum)
    }
}

/// The ISO 13616 remainder: the first four characters move to the end and
/// letters count as two digits (A = 10 ... Z = 35). Computed digit by digit,
/// so no big integer is needed.
fn mod97(iban: &str) -> u32 {
    let (head, tail) = iban.split_at(4);
    tail.bytes().chain(head.bytes()).fold(0, |remainder, b| {
        if b.is_ascii_digit() {
            (remainder * 10 + u32::from(b - b'0')) % 97
        } else {
            (remainder * 100 + u32::from(b - b'A') + 10) % 97
        }
    })
}

/// [`Validator`] for [`EntityKind::Iban`].
#[derive(Debug, Clone, Copy, Default)]
pub struct IbanValidator;

impl Validator for IbanValidator {
    fn validate(&self, _kind: EntityKind, value: &str) -> ValidatorOutcome {
        validate_iban(value)
    }
}

/// Finds TR IBANs read aloud. Callers usually omit `TR`: a 24-digit body is
/// accepted after a spelled `TR` (`te re`) or near an IBAN cue.
#[derive(Debug, Clone, Copy, Default)]
pub struct IbanDetector;

/// A spoken or written `TR` right before a digit lattice: `te re`, `TR`, or
/// `TR33` with the check digits attached.
struct Prefix {
    /// First word of the prefix.
    start: u32,
    /// Digits written inside the prefix word (`33` in `TR33`).
    digits: String,
}

impl IbanDetector {
    fn prefix(ctx: &DetectContext<'_>, lattice: &DigitLattice) -> Option<Prefix> {
        let start = lattice.words.start;
        if let Some(run) = ctx
            .normalized
            .spelled
            .iter()
            .find(|run| run.words.end == start && run.text == "TR")
        {
            return Some(Prefix {
                start: run.words.start,
                digits: String::new(),
            });
        }
        let previous = usize::try_from(start.checked_sub(1)?).ok()?;
        let written = ctx.normalized.words.get(previous)?.written_value()?;
        let rest = written.strip_prefix("tr")?;
        (rest.len() < BODY_DIGITS && rest.bytes().all(|b| b.is_ascii_digit())).then(|| Prefix {
            start: start - 1,
            digits: rest.to_owned(),
        })
    }

    /// IBANs written as one word: `TR330006100519786457841326`,
    /// `IBAN:TR33-0006-1005-1978-6457-8413-26`. The shape alone is strong
    /// evidence, so a failed checksum is still masked, as `_INVALID`.
    fn written(ctx: &DetectContext<'_>, out: &mut Vec<Candidate>) {
        for (index, word) in ctx.normalized.words.iter().enumerate() {
            let Some(written) = word.written_value() else {
                continue;
            };
            let Some(body) = written.strip_prefix("tr") else {
                continue;
            };
            if body.len() != BODY_DIGITS || !body.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let value = format!("TR{body}");
            let validator = validate_iban(&value);
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            out.push(Candidate {
                kind: EntityKind::Iban,
                detector: ID,
                words: index..index.saturating_add(1),
                validator,
                value: SecretValue::new(value),
                prior: 1.0,
                trace: None,
                cues: Vec::new(),
                score_floor: (validator != ValidatorOutcome::Passed).then_some(PARTIAL_FLOOR),
            });
        }
    }

    fn candidate(
        value: String,
        words: std::ops::Range<u32>,
        reading: &ortu_normalize::DigitRun,
    ) -> Candidate {
        Candidate {
            kind: EntityKind::Iban,
            detector: ID,
            words,
            validator: validate_iban(&value),
            value: SecretValue::new(value),
            prior: reading.prior,
            trace: Some(reading.trace.clone()),
            cues: Vec::new(),
            score_floor: None,
        }
    }
}

impl Detector for IbanDetector {
    fn id(&self) -> &'static str {
        ID
    }

    fn kinds(&self) -> &'static [EntityKind] {
        &[EntityKind::Iban]
    }

    fn detect(&self, ctx: &DetectContext<'_>, out: &mut Vec<Candidate>) {
        Self::written(ctx, out);
        for lattice in &ctx.normalized.lattices {
            let prefix = Self::prefix(ctx, lattice);
            let cue = ctx.has_cue(EntityKind::Iban, &lattice.words);
            if prefix.is_none() && !cue {
                continue;
            }
            let lead = prefix.as_ref().map_or("", |p| p.digits.as_str());
            let mut found = false;
            for reading in &lattice.readings {
                let mut attempts: Vec<(String, std::ops::Range<u32>)> = Vec::new();
                // The body completes the prefix: `TR33` + 22 digits, `te re` + 24 digits.
                if let Some(p) = &prefix {
                    if let Some(window) = aligned_windows(reading, BODY_DIGITS - lead.len())
                        .find(|window| window.offset == 0)
                    {
                        attempts.push((
                            format!("TR{lead}{}", window.digits),
                            p.start..window.words.end,
                        ));
                    }
                }
                // After a cue, any 24-digit body (callers usually omit `TR`).
                if cue {
                    for window in aligned_windows(reading, BODY_DIGITS) {
                        attempts.push((format!("TR{}", window.digits), window.words));
                    }
                }
                for (value, words) in attempts {
                    if validate_iban(&value) == ValidatorOutcome::Passed {
                        found = true;
                        push_unique(out, Self::candidate(value, words, reading));
                    }
                }
            }
            let Some(best) = lattice.readings.first() else {
                continue;
            };
            if !found && cue && lead.len() + best.digits.len() >= PARTIAL_MIN_DIGITS {
                let start = prefix.as_ref().map_or(lattice.words.start, |p| p.start);
                let mut partial = Self::candidate(
                    format!("TR{lead}{}", best.digits),
                    start..lattice.words.end,
                    best,
                );
                partial.score_floor = Some(PARTIAL_FLOOR);
                out.push(partial);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use ortu_core::{FailureReason, ValidatorOutcome};

    use super::validate_iban;

    /// The Turkey example of the SWIFT IBAN registry.
    const REGISTRY_EXAMPLE: &str = "TR33 0006 1005 1978 6457 8413 26";

    #[test]
    fn the_registry_example_validates() {
        assert_eq!(validate_iban(REGISTRY_EXAMPLE), ValidatorOutcome::Passed);
        assert_eq!(
            validate_iban("tr330006100519786457841326"),
            ValidatorOutcome::Passed
        );
        // Worked example of docs/algorithms/06.
        assert_eq!(
            validate_iban("TR960006100000000000000001"),
            ValidatorOutcome::Passed
        );
    }

    #[test]
    fn single_substitutions_and_transpositions_fail() {
        let compact: Vec<u8> = REGISTRY_EXAMPLE.bytes().filter(|b| *b != b' ').collect();
        for position in 2..compact.len() {
            let mut changed = compact.clone();
            changed[position] = b'0' + (changed[position] - b'0' + 1) % 10;
            let value = String::from_utf8(changed).unwrap();
            assert_ne!(validate_iban(&value), ValidatorOutcome::Passed, "{value}");
        }
        for position in 4..compact.len() - 1 {
            let mut swapped = compact.clone();
            if swapped[position] != swapped[position + 1] {
                swapped.swap(position, position + 1);
                let value = String::from_utf8(swapped).unwrap();
                assert_ne!(validate_iban(&value), ValidatorOutcome::Passed, "{value}");
            }
        }
    }

    #[test]
    fn structural_failures_have_the_right_reason() {
        use FailureReason::{Format, TooLong, TooShort};
        for (value, reason) in [
            ("DE89370400440532013000", Format),
            ("TR33000610051978645784132", TooShort),
            ("TR3300061005197864578413261", TooLong),
            ("TR33000610051978645784132X", Format),
        ] {
            assert_eq!(
                validate_iban(value),
                ValidatorOutcome::Failed(reason),
                "{value}"
            );
        }
    }
}
