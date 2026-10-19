//! The tool boundary
//! ([algorithm 15](https://github.com/aral-by/ortu/blob/main/docs/algorithms/15-rehydration-and-provenance.md)):
//! context values and tool results enter the LLM as tokens, and tool
//! arguments leave it with real values only after the provenance check.

use ortu_core::{
    AuditAction, AuditEvent, Decision, EntityKind, SecretValue, Speaker, Turn, TurnAnalysis,
    ValidatorOutcome,
};
use ortu_crypto::Token;
use serde_json::{Map, Value};

use crate::error::GuardError;
use crate::map::{Entry, Lookup};
use crate::mask::{Found, Masker, render};
use crate::session::Session;

impl Session {
    /// Tokenizes a value the adopter already holds (caller ID, CRM field)
    /// before it is placed in a prompt. Spellings of one value share a token,
    /// so `0532 123 45 67` registered here and `sıfır beş yüz otuz iki ...`
    /// spoken later resolve to the same `[PHONE:...]`.
    pub fn register(&mut self, kind: EntityKind, value: &SecretValue) -> Token {
        let canonical = ortu_tr::canonicalize(kind, value);
        let token = self
            .tokenizer
            .token(kind, &canonical.value, canonical.outcome);
        self.map.insert(
            &token,
            Entry {
                kind,
                value: canonical.value,
                outcome: canonical.outcome,
            },
        );
        token
    }

    /// Masks a tool result before it re-enters the LLM context; the values
    /// found join the session map. Field names are read as cues (`tckn`,
    /// `phoneNumber`, `ad_soyad`), so `{"name": "Ahmet Yılmaz"}` is masked even
    /// without an introducer. A masked number becomes a string token. The
    /// tool name is reserved for per-tool rules and audit records.
    #[must_use]
    pub fn mask_tool_result(&mut self, _tool: &str, result: &Value) -> Value {
        self.mask_value(result, None)
    }

    /// Checks the arguments of a tool call the LLM made and returns them with
    /// real values in place of tokens.
    ///
    /// # Errors
    ///
    /// - [`GuardError::UnknownToken`] or [`GuardError::AmbiguousToken`]: a
    ///   token this session cannot resolve.
    /// - [`GuardError::InvalidValue`]: an `_INCOMPLETE` or `_INVALID` value for
    ///   a tool that does not accept it.
    /// - [`GuardError::UnknownValue`]: a raw identifier that did not come from
    ///   a token. The LLM never saw a raw value, so it invented or altered it.
    /// - [`GuardError::Audit`]: the release could not be audited.
    pub fn guard_tool_args(&self, tool: &str, args: &Value) -> Result<Value, GuardError> {
        let mut kinds = Vec::new();
        match self.guard_value(tool, args, None, &mut kinds) {
            Ok(released) => {
                self.audit(AuditAction::ToolArgsReleased, tool, kinds, None)
                    .map_err(GuardError::Audit)?;
                Ok(released)
            }
            Err(error) => {
                // The call is refused either way; a failing sink must not turn
                // the specific reason into a generic one.
                let kinds = error.kind().into_iter().collect();
                let _ = self.audit(
                    AuditAction::ToolCallRejected,
                    tool,
                    kinds,
                    Some(error.reason()),
                );
                Err(error)
            }
        }
    }

    fn audit(
        &self,
        action: AuditAction,
        tool: &str,
        kinds: Vec<EntityKind>,
        reason: Option<&'static str>,
    ) -> Result<(), ortu_core::AuditError> {
        self.inner.audit.record(&AuditEvent {
            call_id: self.call_id.clone(),
            tenant_id: self.tenant_id.clone(),
            action,
            tool: Some(tool.to_owned()),
            kinds,
            reason,
        })
    }

    fn mask_value(&mut self, value: &Value, field: Option<&str>) -> Value {
        match value {
            Value::String(text) => Value::String(self.mask_text(field, text)),
            Value::Number(number) => {
                let text = number.to_string();
                let masked = self.mask_text(field, &text);
                if masked == text {
                    value.clone()
                } else {
                    Value::String(masked)
                }
            }
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .map(|item| self.mask_value(item, field))
                    .collect(),
            ),
            Value::Object(object) => Value::Object(
                object
                    .iter()
                    .map(|(key, item)| (key.clone(), self.mask_value(item, Some(key))))
                    .collect::<Map<String, Value>>(),
            ),
            Value::Bool(_) | Value::Null => value.clone(),
        }
    }

    fn mask_text(&mut self, field: Option<&str>, text: &str) -> String {
        // The field name works like an assistant question: `name` asks for a
        // name, so a short answer is read as one.
        let hint = field.and_then(ortu_tr::field_kind);
        let turn = Turn::from_text(0, Speaker::Caller, text, 0, 0);
        let analysis = self.text_analysis(&turn, hint);
        if analysis.masked_words.is_empty() {
            return text.to_owned();
        }
        let found: Vec<Found> = analysis
            .entities
            .into_iter()
            .map(|entity| Found::detected(&turn, entity))
            .collect();
        let mut masker = Masker {
            inner: &self.inner,
            tokenizer: &self.tokenizer,
            map: Some(&mut self.map),
        };
        let mut ids = 0;
        let reports = masker.reports(found, &mut ids, |_| Some(&turn));
        render(&turn, &analysis.masked_words, &reports)
    }

    fn text_analysis(&self, turn: &Turn, hint: Option<EntityKind>) -> TurnAnalysis {
        self.inner
            .analyzer
            .analyze_turn(turn, &self.inner.hints(hint), 0)
    }

    fn guard_value(
        &self,
        tool: &str,
        value: &Value,
        field: Option<&str>,
        kinds: &mut Vec<EntityKind>,
    ) -> Result<Value, GuardError> {
        match value {
            Value::String(text) => self.guard_text(tool, field, text, kinds).map(Value::String),
            Value::Number(number) => {
                self.reject_raw(field, &number.to_string())?;
                Ok(value.clone())
            }
            Value::Array(items) => items
                .iter()
                .map(|item| self.guard_value(tool, item, field, kinds))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(object) => object
                .iter()
                .map(|(key, item)| {
                    self.guard_value(tool, item, Some(key), kinds)
                        .map(|item| (key.clone(), item))
                })
                .collect::<Result<Map<String, Value>, _>>()
                .map(Value::Object),
            Value::Bool(_) | Value::Null => Ok(value.clone()),
        }
    }

    /// Resolves the tokens of one string and checks what is left for raw
    /// identifiers.
    fn guard_text(
        &self,
        tool: &str,
        field: Option<&str>,
        text: &str,
        kinds: &mut Vec<EntityKind>,
    ) -> Result<String, GuardError> {
        let mut released = String::with_capacity(text.len());
        let mut remainder = String::with_capacity(text.len());
        let mut last = 0;
        for found in self.inner.matcher.find_all(text, None, None) {
            let entry =
                self.map
                    .get(&found.label, &found.digest)
                    .map_err(|lookup| match lookup {
                        Lookup::Unknown => GuardError::UnknownToken {
                            label: found.label.clone(),
                        },
                        Lookup::Ambiguous => GuardError::AmbiguousToken {
                            label: found.label.clone(),
                        },
                    })?;
            let valid = matches!(
                entry.outcome,
                ValidatorOutcome::Passed | ValidatorOutcome::NotApplicable
            );
            if !valid && !self.inner.accepts_invalid(tool, entry.kind) {
                return Err(GuardError::InvalidValue { kind: entry.kind });
            }
            kinds.push(entry.kind);
            let before = &text[last..found.range.start];
            released.push_str(before);
            released.push_str(entry.value.expose_secret());
            remainder.push_str(before);
            remainder.push(' ');
            last = found.range.end;
        }
        released.push_str(&text[last..]);
        remainder.push_str(&text[last..]);
        self.reject_raw(field, &remainder)?;
        Ok(released)
    }

    /// Fails if `text` contains an identifier at or above the mask threshold.
    /// Field names only strengthen identifier kinds here: a raw name in an
    /// argument is not a lookup key into another person's record.
    fn reject_raw(&self, field: Option<&str>, text: &str) -> Result<(), GuardError> {
        let hint = field
            .and_then(ortu_tr::field_kind)
            .filter(|kind| ortu_tr::assembles(*kind));
        let turn = Turn::from_text(0, Speaker::Unknown, text, 0, 0);
        let analysis = self.text_analysis(&turn, hint);
        let policy = self.inner.policy();
        match analysis
            .entities
            .iter()
            .find(|entity| policy.decide(entity.score) == Decision::Mask)
        {
            Some(entity) => Err(GuardError::UnknownValue { kind: entity.kind }),
            None => Ok(()),
        }
    }
}
