//! The session map: token to raw value, in memory only, for one call
//! (docs/systemdesign.md, section 10.8).
//!
//! Every path into the LLM context writes to it (`push`, `register`,
//! `mask_tool_result`); every path out toward an external system reads from it
//! (`guard_tool_args`, `rehydrate`). It is never serialized, logged or shared;
//! its values are zeroized when the session is dropped.

use std::collections::HashMap;

use ortu_core::{EntityKind, SecretValue, ValidatorOutcome};
use ortu_crypto::{Token, canonical_form};

/// One value behind a token.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    /// The kind the value was tokenized as.
    pub(crate) kind: EntityKind,
    /// The value released to tools and read back.
    pub(crate) value: SecretValue,
    /// The validator outcome the token carries.
    pub(crate) outcome: ValidatorOutcome,
}

#[derive(Debug)]
enum Slot {
    One(Entry),
    /// Two different values produced the same label and digest. Resolving the
    /// token would be a guess, so it fails (fail-closed).
    Ambiguous,
}

/// Why a token could not be resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lookup {
    /// No value was ever tokenized to it in this session.
    Unknown,
    /// Two values share it.
    Ambiguous,
}

/// `(label, digest)` to value.
#[derive(Debug, Default)]
pub(crate) struct SessionMap {
    slots: HashMap<String, Slot>,
}

impl SessionMap {
    /// Records the value behind a token. A second, different value under the
    /// same token makes the token ambiguous; the same value again is a no-op.
    pub(crate) fn insert(&mut self, token: &Token, entry: Entry) {
        let key = key(&entry.kind.label(), token.digest());
        match self.slots.get_mut(&key) {
            None => {
                self.slots.insert(key, Slot::One(entry));
            }
            Some(Slot::One(existing)) => {
                let same = canonical_form(existing.kind, &existing.value).expose_secret()
                    == canonical_form(entry.kind, &entry.value).expose_secret();
                if !same {
                    self.slots.insert(key, Slot::Ambiguous);
                }
            }
            Some(Slot::Ambiguous) => {}
        }
    }

    /// The value behind `label` and `digest` as found in text.
    pub(crate) fn get(&self, label: &str, digest: &str) -> Result<&Entry, Lookup> {
        match self.slots.get(&key(label, digest)) {
            Some(Slot::One(entry)) => Ok(entry),
            Some(Slot::Ambiguous) => Err(Lookup::Ambiguous),
            None => Err(Lookup::Unknown),
        }
    }

    /// Number of tokens recorded.
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }
}

fn key(label: &str, digest: &str) -> String {
    format!(
        "{}:{}",
        label.to_ascii_uppercase(),
        digest.to_ascii_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use ortu_core::{EntityKind, SecretValue, TokenScope, ValidatorOutcome};
    use ortu_crypto::{SecretKey, StaticKeyProvider, Tokenizer};

    use super::{Entry, Lookup, SessionMap};

    fn tokenizer() -> Tokenizer {
        let provider = StaticKeyProvider::single(SecretKey::from_bytes([7; 32]));
        Tokenizer::new(&provider, "t", TokenScope::Call, "c").unwrap()
    }

    fn entry(kind: EntityKind, value: &str) -> Entry {
        Entry {
            kind,
            value: SecretValue::new(value),
            outcome: ValidatorOutcome::Passed,
        }
    }

    #[test]
    fn values_resolve_by_label_and_digest_in_any_case() {
        let tokenizer = tokenizer();
        let mut map = SessionMap::default();
        let value = SecretValue::new("10000000146");
        let token = tokenizer.token(EntityKind::Tckn, &value, ValidatorOutcome::Passed);
        map.insert(&token, entry(EntityKind::Tckn, "10000000146"));
        let digest = token.digest().to_ascii_uppercase();
        assert_eq!(
            map.get("tckn", &digest).unwrap().value.expose_secret(),
            "10000000146"
        );
        assert_eq!(map.get("TCKN", "00000000").unwrap_err(), Lookup::Unknown);
        assert_eq!(
            map.get("PHONE", token.digest()).unwrap_err(),
            Lookup::Unknown
        );
    }

    #[test]
    fn spellings_of_one_value_share_a_slot_and_different_values_make_it_ambiguous() {
        let tokenizer = tokenizer();
        let mut map = SessionMap::default();
        let token = tokenizer.token(
            EntityKind::FullName,
            &SecretValue::new("Ahmet Yılmaz"),
            ValidatorOutcome::NotApplicable,
        );
        map.insert(&token, entry(EntityKind::FullName, "Ahmet Yılmaz"));
        map.insert(&token, entry(EntityKind::FullName, "ahmet yılmaz"));
        assert_eq!(map.len(), 1);
        assert!(map.get("NAME", token.digest()).is_ok());

        // A forced collision: a different value under the same token.
        map.insert(&token, entry(EntityKind::FullName, "Mehmet Kaya"));
        assert_eq!(
            map.get("NAME", token.digest()).unwrap_err(),
            Lookup::Ambiguous
        );
    }
}
