//! Turkish detectors and validators for ortu.
//!
//! M2 ships the core detectors (docs/systemdesign.md, section 7.2; algorithms
//! 04, 06, 08 and 09):
//!
//! - [`TcknDetector`] and [`validate_tckn`]: national identity numbers
//! - [`IbanDetector`] and [`validate_iban`]: TR IBANs
//! - [`PhoneDetector`] and [`canonical_phone`]: mobile and landline numbers
//! - [`NameDetector`]: cue-based person names
//!
//! plus the cue rules they rely on ([`default_cue_rules`]), and, for the
//! session API, [`canonicalize`], [`assemble`] and [`field_kind`]: canonical
//! forms of registered values, values assembled across turns, and the kinds
//! that tool fields hold. Detectors are pure
//! functions of their input; session state lives in `ortu-stream`. VKN, cards,
//! email, plates, birth dates, passports, addresses and name gazetteers follow
//! in M4.
//!
//! ```
//! use ortu_core::{Analyzer, Policy, SessionHints, Speaker, Turn, Weights};
//!
//! let analyzer = Analyzer::new(
//!     ortu_tr::default_detectors(),
//!     ortu_tr::default_cue_rules(),
//!     Weights::default(),
//!     Policy::default(),
//! );
//! let turn = Turn::from_text(0, Speaker::Caller, "TC'm bir sıfır sıfır sıfır sıfır sıfır sıfır sıfır bir dört altı", 0, 4000);
//! let analysis = analyzer.analyze_turn(&turn, &SessionHints::default(), 0);
//!
//! assert_eq!(analysis.entities.len(), 1);
//! assert_eq!(analysis.entities[0].kind, ortu_core::EntityKind::Tckn);
//! ```

mod data;
mod digits;
mod iban;
mod name;
mod phone;
mod tckn;
mod values;

use ortu_core::{CuePattern, CueRule, Detector, EntityKind};

pub use iban::{IbanDetector, IbanValidator, validate_iban};
pub use name::NameDetector;
pub use phone::{PhoneClass, PhoneDetector, PhoneValidator, canonical_phone, validate_phone};
pub use tckn::{TcknDetector, TcknValidator, validate_tckn};
pub use values::{Assembled, assemble, assembles, canonicalize, field_kind};

/// The M2 detectors with their default settings.
#[must_use]
pub fn default_detectors() -> Vec<Box<dyn Detector>> {
    vec![
        Box::new(TcknDetector),
        Box::new(IbanDetector),
        Box::new(PhoneDetector::default()),
        Box::new(NameDetector),
    ]
}

/// The cue rules for the M2 kinds.
#[must_use]
pub fn default_cue_rules() -> Vec<CueRule> {
    data::CUE_RULES
        .iter()
        .filter_map(|rule| {
            Some(CueRule {
                kind: EntityKind::from_config_name(rule.kind)?,
                patterns: rule.patterns.iter().map(|p| CuePattern::parse(p)).collect(),
                window_before: rule.window_before,
                window_after: rule.window_after,
                boost: rule.boost,
                cross_turn: rule.cross_turn,
            })
        })
        .collect()
}
