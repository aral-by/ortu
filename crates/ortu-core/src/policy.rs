//! Decision policy (docs/systemdesign.md, sections 3.3, 8.3 and 10.2).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::ConfigError;
use crate::entity::EntityKind;

/// A preset for the two thresholds when the policy does not set them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Mask at 0.50, review from 0.30. A leak costs more than a false positive.
    #[default]
    RecallFirst,
    /// Mask at 0.60, review from 0.40.
    Balanced,
    /// Mask at 0.75, review from 0.60.
    PrecisionFirst,
}

impl Mode {
    fn thresholds(self) -> (f32, f32) {
        match self {
            Self::RecallFirst => (0.50, 0.30),
            Self::Balanced => (0.60, 0.40),
            Self::PrecisionFirst => (0.75, 0.60),
        }
    }
}

/// What happens when processing fails (section 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailMode {
    /// Return an error; no text is released.
    #[default]
    Block,
    /// Release the text with every digit sequence and name candidate redacted.
    RedactAll,
}

/// How a masked value is replaced (section 10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    /// `[TCKN]`; irreversible.
    Redact,
    /// `*******8901`; irreversible.
    Mask,
    /// `[TCKN:7f3a9c2e]`; keyed, deterministic, irreversible without the key.
    #[default]
    Token,
    /// A token plus an encrypted vault record (M5).
    Encrypt,
}

/// Which key derives tokens (section 10.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenScope {
    /// One key per call: the same person cannot be linked across calls.
    #[default]
    Call,
    /// One key per tenant: tokens link across calls, for analytics.
    Tenant,
}

/// Per-kind settings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindPolicy {
    /// Overrides the default transform.
    pub transform: Option<Transform>,
    /// Address levels kept in clear (`province`, `district`).
    #[serde(default)]
    pub keep_levels: Vec<String>,
}

/// The outcome of the decision step for one score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Masked.
    Mask,
    /// Masked and flagged for review.
    MaskAndReview,
    /// Left in clear.
    Pass,
}

/// The decision policy, usually read from `ortu.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// The preset the thresholds came from.
    pub mode: Mode,
    /// Scores at or above this are masked.
    pub mask_threshold: f32,
    /// Scores at or above this and below `mask_threshold` are masked and flagged.
    pub review_threshold: f32,
    /// Behavior on processing errors.
    pub fail_mode: FailMode,
    /// Token key scope.
    pub token_scope: TokenScope,
    /// Per-kind settings, keyed by the kind's `config_name`.
    pub kinds: BTreeMap<String, KindPolicy>,
}

impl Default for Policy {
    fn default() -> Self {
        let (mask_threshold, review_threshold) = Mode::default().thresholds();
        Self {
            mode: Mode::default(),
            mask_threshold,
            review_threshold,
            fail_mode: FailMode::default(),
            token_scope: TokenScope::default(),
            kinds: BTreeMap::new(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    policy: PolicySection,
    #[serde(default)]
    tokens: TokensSection,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicySection {
    #[serde(default)]
    mode: Mode,
    mask_threshold: Option<f32>,
    review_threshold: Option<f32>,
    #[serde(default)]
    fail_mode: FailMode,
    #[serde(default)]
    kinds: BTreeMap<String, KindPolicy>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokensSection {
    #[serde(default)]
    scope: TokenScope,
}

impl Policy {
    /// Parses and validates an `ortu.toml` document.
    ///
    /// # Errors
    ///
    /// Returns an error if the document is not valid TOML, contains an unknown
    /// key or kind, has thresholds outside `0 <= review <= mask <= 1`, or sets
    /// a transform other than `redact` for credit cards (decision D13).
    pub fn from_toml_str(document: &str) -> Result<Self, ConfigError> {
        let file: PolicyFile =
            toml::from_str(document).map_err(|error| ConfigError::from_toml(&error))?;
        let section = file.policy;
        let (default_mask, default_review) = section.mode.thresholds();
        let policy = Self {
            mode: section.mode,
            mask_threshold: section.mask_threshold.unwrap_or(default_mask),
            review_threshold: section.review_threshold.unwrap_or(default_review),
            fail_mode: section.fail_mode,
            token_scope: file.tokens.scope,
            kinds: section.kinds,
        };
        policy.validate()?;
        Ok(policy)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let ordered = (0.0..=1.0).contains(&self.review_threshold)
            && (0.0..=1.0).contains(&self.mask_threshold)
            && self.review_threshold <= self.mask_threshold;
        if !ordered {
            return Err(ConfigError::new(
                "thresholds must satisfy 0 <= review_threshold <= mask_threshold <= 1",
            ));
        }
        if let Some(unknown) = self
            .kinds
            .keys()
            .find(|name| EntityKind::from_config_name(name).is_none())
        {
            return Err(ConfigError::new(format!(
                "unknown kind `{unknown}` in policy.kinds"
            )));
        }
        let card = self.kinds.get(EntityKind::CreditCard.config_name());
        if card
            .and_then(|k| k.transform)
            .is_some_and(|t| t != Transform::Redact)
        {
            return Err(ConfigError::new(
                "credit_card can only use transform = \"redact\" (decision D13)",
            ));
        }
        Ok(())
    }

    /// The decision for a score.
    #[must_use]
    pub fn decide(&self, score: f32) -> Decision {
        if score >= self.mask_threshold {
            Decision::Mask
        } else if score >= self.review_threshold {
            Decision::MaskAndReview
        } else {
            Decision::Pass
        }
    }

    /// The transform for a kind. Credit cards are always redacted.
    #[must_use]
    pub fn transform_for(&self, kind: EntityKind) -> Transform {
        if kind == EntityKind::CreditCard {
            return Transform::Redact;
        }
        self.kinds
            .get(kind.config_name())
            .and_then(|k| k.transform)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, FailMode, Policy, TokenScope, Transform};
    use crate::entity::EntityKind;

    const SECTION_8_3: &str = r#"
        [policy]
        mode = "recall_first"
        mask_threshold = 0.50
        review_threshold = 0.30
        fail_mode = "block"

        [policy.kinds.credit_card]
        transform = "redact"

        [policy.kinds.address]
        keep_levels = ["province", "district"]
    "#;

    #[test]
    fn the_example_from_section_8_3_parses() {
        let policy = Policy::from_toml_str(SECTION_8_3).unwrap();
        assert_eq!(
            policy,
            Policy {
                kinds: policy.kinds.clone(),
                ..Policy::default()
            }
        );
        assert_eq!(
            policy.kinds["address"].keep_levels,
            ["province", "district"]
        );
        assert_eq!(policy.fail_mode, FailMode::Block);
        assert_eq!(policy.token_scope, TokenScope::Call);
    }

    #[test]
    fn decisions_follow_the_thresholds() {
        let policy = Policy::default();
        assert_eq!(policy.decide(0.5), Decision::Mask);
        assert_eq!(policy.decide(0.3), Decision::MaskAndReview);
        assert_eq!(policy.decide(0.29), Decision::Pass);
    }

    #[test]
    fn credit_cards_are_always_redacted() {
        let bad = "[policy]\n[policy.kinds.credit_card]\ntransform = \"token\"\n";
        assert!(Policy::from_toml_str(bad).is_err());
        assert_eq!(
            Policy::default().transform_for(EntityKind::CreditCard),
            Transform::Redact
        );
        assert_eq!(
            Policy::default().transform_for(EntityKind::Tckn),
            Transform::Token
        );
    }

    #[test]
    fn invalid_documents_are_rejected() {
        for document in [
            "[policy]\nmask_threshold = 0.2\nreview_threshold = 0.4\n",
            "[policy]\n[policy.kinds.shoe_size]\ntransform = \"mask\"\n",
            "[policy]\nunknown_key = 1\n",
            "[policy]\nmode = \"yolo\"\n",
            "not toml",
        ] {
            assert!(Policy::from_toml_str(document).is_err(), "{document}");
        }
    }

    #[test]
    fn mode_presets_apply_when_thresholds_are_omitted() {
        let policy = Policy::from_toml_str("[policy]\nmode = \"precision_first\"\n").unwrap();
        assert!((policy.mask_threshold - 0.75).abs() < f32::EPSILON);
        assert!((policy.review_threshold - 0.60).abs() < f32::EPSILON);
    }
}
