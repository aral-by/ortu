//! `SecretValue`: the only type that holds a raw personal value
//! (docs/systemdesign.md, section 5.4).

use std::fmt;

use zeroize::Zeroizing;

/// A raw personal value: zeroized on drop, never printed, never serialized.
///
/// There is deliberately no `Display`, `Serialize` or `PartialEq`
/// implementation: a value cannot reach a log line, a JSON document or a
/// timing-dependent comparison by accident. Reading it requires an explicit
/// [`SecretValue::expose_secret`] call, which is easy to audit with a search.
#[derive(Clone)]
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    /// Wraps a raw value.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    /// The raw value. Callers are limited to tokenization, encryption, the
    /// tool boundary and read-back; every call site is reviewed.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<redacted>)")
    }
}

#[cfg(test)]
mod tests {
    use super::SecretValue;

    #[test]
    fn debug_output_never_contains_the_value() {
        let secret = SecretValue::new("10000000146");
        assert_eq!(format!("{secret:?}"), "SecretValue(<redacted>)");
        assert_eq!(secret.expose_secret(), "10000000146");
    }
}
