//! Entity kinds, validator outcomes, candidates and entities
//! (docs/systemdesign.md, section 5.3).

use std::ops::Range;

use ortu_normalize::NormalizationTrace;

use crate::model::SourceSpan;
use crate::score::ScoreBreakdown;
use crate::secret::SecretValue;

/// What kind of personal data an entity is.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntityKind {
    /// Turkish national identity number.
    Tckn,
    /// Turkish tax identification number.
    Vkn,
    /// IBAN (TR in v1).
    Iban,
    /// Phone number.
    Phone,
    /// Payment card number, expiry date or security code.
    CreditCard,
    /// Email address.
    Email,
    /// First name.
    PersonName,
    /// Surname.
    PersonSurname,
    /// First name and surname together.
    FullName,
    /// Postal address.
    Address,
    /// Vehicle license plate.
    Plate,
    /// Birth date.
    BirthDate,
    /// Passport number.
    PassportNo,
    /// An adopter-defined kind.
    Custom(&'static str),
}

impl EntityKind {
    /// Every built-in kind, in declaration order.
    pub const BUILT_IN: [Self; 13] = [
        Self::Tckn,
        Self::Vkn,
        Self::Iban,
        Self::Phone,
        Self::CreditCard,
        Self::Email,
        Self::PersonName,
        Self::PersonSurname,
        Self::FullName,
        Self::Address,
        Self::Plate,
        Self::BirthDate,
        Self::PassportNo,
    ];

    /// The name used in `ortu.toml` (`[policy.kinds.credit_card]`).
    #[must_use]
    pub fn config_name(self) -> &'static str {
        match self {
            Self::Tckn => "tckn",
            Self::Vkn => "vkn",
            Self::Iban => "iban",
            Self::Phone => "phone",
            Self::CreditCard => "credit_card",
            Self::Email => "email",
            Self::PersonName => "person_name",
            Self::PersonSurname => "person_surname",
            Self::FullName => "full_name",
            Self::Address => "address",
            Self::Plate => "plate",
            Self::BirthDate => "birth_date",
            Self::PassportNo => "passport_no",
            Self::Custom(name) => name,
        }
    }

    /// The built-in kind with the given `ortu.toml` name.
    #[must_use]
    pub fn from_config_name(name: &str) -> Option<Self> {
        Self::BUILT_IN
            .into_iter()
            .find(|kind| kind.config_name() == name)
    }

    /// The label used in tokens (`[TCKN:...]`, `[NAME:...]`); names, surnames
    /// and full names share `NAME` so that tokens reveal no more than needed.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Tckn => "TCKN".to_owned(),
            Self::Vkn => "VKN".to_owned(),
            Self::Iban => "IBAN".to_owned(),
            Self::Phone => "PHONE".to_owned(),
            Self::CreditCard => "CARD".to_owned(),
            Self::Email => "EMAIL".to_owned(),
            Self::PersonName | Self::PersonSurname | Self::FullName => "NAME".to_owned(),
            Self::Address => "ADDRESS".to_owned(),
            Self::Plate => "PLATE".to_owned(),
            Self::BirthDate => "BIRTHDATE".to_owned(),
            Self::PassportNo => "PASSPORT".to_owned(),
            Self::Custom(name) => name.to_ascii_uppercase(),
        }
    }

    /// Rank used by conflict resolution: longer, structurally richer
    /// identifiers contain shorter ones by accident far more often than the
    /// reverse (section 8.2, algorithm 11).
    #[must_use]
    pub fn specificity(self) -> u8 {
        match self {
            Self::Iban => 7,
            Self::CreditCard => 6,
            Self::Tckn => 5,
            Self::Vkn => 4,
            Self::Phone => 3,
            Self::Plate => 2,
            _ => 1,
        }
    }
}

/// Why a validator rejected a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FailureReason {
    /// Fewer characters than the kind requires.
    TooShort,
    /// More characters than the kind allows.
    TooLong,
    /// Right length, wrong checksum.
    Checksum,
    /// Right length, invalid structure (for example a leading zero).
    Format,
}

/// The outcome of validating a candidate value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidatorOutcome {
    /// The value passed every structural check.
    Passed,
    /// The value failed a check.
    Failed(FailureReason),
    /// The kind has no validator (names).
    NotApplicable,
}

impl ValidatorOutcome {
    /// The token marker for this outcome (section 10.2): empty for valid values,
    /// `_INCOMPLETE` for too few digits, `_INVALID` otherwise.
    #[must_use]
    pub fn token_marker(self) -> &'static str {
        match self {
            Self::Passed | Self::NotApplicable => "",
            Self::Failed(FailureReason::TooShort) => "_INCOMPLETE",
            Self::Failed(_) => "_INVALID",
        }
    }

    /// Right length but a failed checksum or structure: evidence against the
    /// candidate in the score model (section 8.1).
    #[must_use]
    pub fn is_exact_length_failure(self) -> bool {
        matches!(
            self,
            Self::Failed(FailureReason::Checksum | FailureReason::Format)
        )
    }
}

/// A context cue that supported a candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct CueHit {
    /// The cue pattern as written in the cue data (`tc`, `iban`, `adım`).
    pub pattern: String,
    /// Words between the cue and the candidate; 0 when adjacent.
    pub distance: u16,
    /// The weight of the cue (`w_cue` in section 8.1).
    pub boost: f32,
    /// The cue was in the previous assistant turn.
    pub cross_turn: bool,
}

/// What a detector proposes, before scoring.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The kind of personal data.
    pub kind: EntityKind,
    /// The detector that proposed it.
    pub detector: &'static str,
    /// Word indices covered, within the turn.
    pub words: Range<u32>,
    /// The canonical value (digits, E.164, folded name).
    pub value: SecretValue,
    /// The validator outcome.
    pub validator: ValidatorOutcome,
    /// Prior of the lattice reading the value came from; 1.0 otherwise.
    pub prior: f32,
    /// The lattice reading's trace, for numeric candidates.
    pub trace: Option<NormalizationTrace>,
    /// Cues the detector itself relied on (name patterns).
    pub cues: Vec<CueHit>,
    /// Minimum score from a partial-match policy (section 7.2).
    pub score_floor: Option<f32>,
}

/// Explainable evidence for an entity (section 5.3).
#[derive(Debug, Clone)]
pub struct Evidence {
    /// The detector that proposed the entity.
    pub detector: &'static str,
    /// The validator outcome.
    pub validator: ValidatorOutcome,
    /// Every cue that contributed to the score.
    pub cues: Vec<CueHit>,
    /// How the spoken number was read, for numeric entities.
    pub normalization: Option<NormalizationTrace>,
    /// Whether a cue from the previous assistant turn contributed.
    pub cross_turn: bool,
    /// The score, term by term.
    pub score: ScoreBreakdown,
}

/// Identifier of an entity, unique within its call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityId(pub u32);

/// A detected, masked piece of personal data.
#[derive(Debug, Clone)]
pub struct Entity {
    /// Unique within the call.
    pub id: EntityId,
    /// The kind.
    pub kind: EntityKind,
    /// Exact position in the source text.
    pub span: SourceSpan,
    /// Word indices covered, within the turn.
    pub words: Range<u32>,
    /// Final score in `0.0..=1.0`.
    pub score: f32,
    /// The score is between the review and mask thresholds: masked and flagged.
    pub review: bool,
    /// Why the entity was masked.
    pub evidence: Evidence,
    /// The raw (canonical) value.
    pub value: SecretValue,
}

#[cfg(test)]
mod tests {
    use super::{EntityKind, FailureReason, ValidatorOutcome};

    #[test]
    fn config_names_round_trip() {
        for kind in EntityKind::BUILT_IN {
            assert_eq!(EntityKind::from_config_name(kind.config_name()), Some(kind));
        }
        assert_eq!(EntityKind::from_config_name("unknown"), None);
    }

    #[test]
    fn names_share_one_token_label() {
        assert_eq!(EntityKind::PersonName.label(), "NAME");
        assert_eq!(EntityKind::FullName.label(), "NAME");
        assert_eq!(EntityKind::Custom("member_id").label(), "MEMBER_ID");
    }

    #[test]
    fn token_markers_follow_section_10_2() {
        assert_eq!(ValidatorOutcome::Passed.token_marker(), "");
        assert_eq!(
            ValidatorOutcome::Failed(FailureReason::TooShort).token_marker(),
            "_INCOMPLETE"
        );
        assert_eq!(
            ValidatorOutcome::Failed(FailureReason::Checksum).token_marker(),
            "_INVALID"
        );
    }
}
