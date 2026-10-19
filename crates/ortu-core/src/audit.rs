//! Audit events (docs/systemdesign.md, section 12).
//!
//! Events carry kinds, tokens and identifiers, never raw values. Adopters
//! deliver them to their own store through [`AuditSink`]; the sink adds the
//! timestamp and the actor, which ortu, being free of clocks and I/O, does not
//! know.

use std::fmt;

use crate::entity::EntityKind;

/// What an audited operation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditAction {
    /// Tool arguments passed the provenance check and their tokens were
    /// replaced by real values.
    ToolArgsReleased,
    /// Tool arguments were rejected (section 10.8).
    ToolCallRejected,
}

impl AuditAction {
    /// The action name used in audit records (`tool_args_released`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ToolArgsReleased => "tool_args_released",
            Self::ToolCallRejected => "tool_call_rejected",
        }
    }
}

/// One audit record. Contains no raw value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    /// The call the event belongs to.
    pub call_id: String,
    /// The tenant.
    pub tenant_id: String,
    /// What happened.
    pub action: AuditAction,
    /// The tool, for tool-boundary events.
    pub tool: Option<String>,
    /// The kinds of the values involved, in order of appearance.
    pub kinds: Vec<EntityKind>,
    /// Why an operation was rejected (`unknown_token`, `unknown_value`).
    pub reason: Option<&'static str>,
}

/// The audit sink could not record an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditError {
    message: String,
}

impl AuditError {
    /// An error with the given message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "audit event not recorded: {}", self.message)
    }
}

impl std::error::Error for AuditError {}

/// Receives audit events; implemented by the adopter (database, log pipeline,
/// SIEM). Operations that release raw values fail when the sink fails
/// (fail-closed).
pub trait AuditSink: Send + Sync {
    /// Records one event.
    ///
    /// # Errors
    ///
    /// Returns an error if the event could not be stored.
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError>;
}

/// A sink that drops every event, for adopters without an audit store.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiscardAuditSink;

impl AuditSink for DiscardAuditSink {
    fn record(&self, _event: &AuditEvent) -> Result<(), AuditError> {
        Ok(())
    }
}
