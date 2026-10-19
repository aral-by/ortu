//! Errors of the session API.

use std::fmt;

use ortu_core::{AuditError, ConfigError, EntityKind};
use ortu_crypto::KeyError;

/// The engine or a session could not do what was asked; no text is released.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The engine configuration is unusable.
    Config(ConfigError),
    /// No usable token key for the tenant (section 10.3: the engine refuses to
    /// run without one).
    Key(KeyError),
    /// Turns must arrive in increasing `index` order.
    TurnOutOfOrder {
        /// The index of the last accepted turn.
        previous: u32,
        /// The index that was pushed.
        pushed: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => error.fmt(f),
            Self::Key(error) => error.fmt(f),
            Self::TurnOutOfOrder { previous, pushed } => {
                write!(f, "turn {pushed} pushed after turn {previous}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<ConfigError> for Error {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<KeyError> for Error {
    fn from(error: KeyError) -> Self {
        Self::Key(error)
    }
}

/// A value on its way out of the LLM was refused (section 10.8). The message
/// names kinds and labels only, never values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GuardError {
    /// A token-shaped string that this session never issued: the LLM corrupted
    /// or invented it. It is never passed through and never guessed.
    UnknownToken {
        /// The label as found (`TCKN`).
        label: String,
    },
    /// Two values of this session share the token; resolving it would be a
    /// guess.
    AmbiguousToken {
        /// The label as found.
        label: String,
    },
    /// A tool received an `_INCOMPLETE` or `_INVALID` value it does not accept.
    InvalidValue {
        /// The kind of the value.
        kind: EntityKind,
    },
    /// A raw value in tool arguments that did not come from a token: the LLM
    /// invented or altered it.
    UnknownValue {
        /// The detected kind.
        kind: EntityKind,
    },
    /// The audit sink failed, so nothing was released (fail-closed).
    Audit(AuditError),
}

impl GuardError {
    /// The reason recorded in audit events and metrics (`unknown_token`).
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::UnknownToken { .. } => "unknown_token",
            Self::AmbiguousToken { .. } => "ambiguous_token",
            Self::InvalidValue { .. } => "invalid_value",
            Self::UnknownValue { .. } => "unknown_value",
            Self::Audit(_) => "audit_failed",
        }
    }

    /// The kind involved, when known.
    #[must_use]
    pub fn kind(&self) -> Option<EntityKind> {
        match self {
            Self::InvalidValue { kind } | Self::UnknownValue { kind } => Some(*kind),
            _ => None,
        }
    }
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownToken { label } => write!(f, "unknown {label} token"),
            Self::AmbiguousToken { label } => write!(f, "ambiguous {label} token"),
            Self::InvalidValue { kind } => {
                write!(f, "incomplete or invalid {} value", kind.config_name())
            }
            Self::UnknownValue { kind } => write!(
                f,
                "raw {} value that did not come from this session",
                kind.config_name()
            ),
            Self::Audit(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for GuardError {}
