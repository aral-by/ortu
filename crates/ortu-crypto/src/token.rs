//! Keyed tokenization
//! ([algorithm 13](https://github.com/aral-by/ortu/blob/main/docs/algorithms/13-keyed-tokenization.md)).

use std::fmt;

use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use ortu_core::{EntityKind, SecretValue, TokenScope, ValidatorOutcome};
use ortu_normalize::turkish_fold;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::key::{KeyError, KeyProvider, SecretKey};

/// HKDF salt for call-scoped token keys. Changing it changes every token.
const TOKEN_SALT: &[u8] = b"ortu/token/v1";

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// A token such as `[TCKN:7f3a9c2e]` or `[TCKN_INCOMPLETE:b20c5d11]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Token(String);

impl Token {
    /// The token text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Issues tokens for one call (call scope) or one tenant (tenant scope).
#[derive(Clone)]
pub struct Tokenizer {
    mac: Hmac<Sha256>,
}

impl fmt::Debug for Tokenizer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Tokenizer(<keyed>)")
    }
}

impl Tokenizer {
    /// Derives the token key: the tenant key itself for tenant scope, or
    /// `HKDF-SHA256(tenant key, salt = "ortu/token/v1", info = call id)` for
    /// call scope, so that the same value in two calls yields two tokens.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider has no usable key for the tenant.
    pub fn new(
        provider: &dyn KeyProvider,
        tenant: &str,
        scope: TokenScope,
        call_id: &str,
    ) -> Result<Self, KeyError> {
        let tenant_key = provider.token_key(tenant)?;
        let key = match scope {
            TokenScope::Tenant => tenant_key,
            TokenScope::Call => derive_call_key(&tenant_key, call_id)?,
        };
        let mac = <Hmac<Sha256> as Mac>::new_from_slice(key.expose())
            .map_err(|_| KeyError::InvalidKey("HMAC rejected the key length".to_owned()))?;
        Ok(Self { mac })
    }

    /// The token for a value: `[<LABEL><MARKER>:<hex8>]`, where `hex8` is the
    /// first 32 bits of `HMAC-SHA256(key, label || 0x00 || canonical(value))`.
    #[must_use]
    pub fn token(
        &self,
        kind: EntityKind,
        value: &SecretValue,
        validity: ValidatorOutcome,
    ) -> Token {
        let label = kind.label();
        let canonical = Zeroizing::new(canonical(kind, value.expose_secret()));
        let mut mac = self.mac.clone();
        mac.update(label.as_bytes());
        mac.update(&[0]);
        mac.update(canonical.as_bytes());
        let digest = mac.finalize().into_bytes();
        let mut hex = String::with_capacity(8);
        for byte in &digest[..4] {
            hex.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
            hex.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
        }
        Token(format!("[{label}{}:{hex}]", validity.token_marker()))
    }
}

fn derive_call_key(tenant_key: &SecretKey, call_id: &str) -> Result<SecretKey, KeyError> {
    let hkdf = Hkdf::<Sha256>::new(Some(TOKEN_SALT), tenant_key.expose());
    let mut okm = Zeroizing::new([0u8; 32]);
    hkdf.expand(call_id.as_bytes(), okm.as_mut())
        .map_err(|_| KeyError::InvalidKey("HKDF output length rejected".to_owned()))?;
    Ok(SecretKey::from_bytes(*okm))
}

/// The canonical form of a value, so that every reading of the same value
/// yields the same token (algorithm 13). Idempotent.
fn canonical(kind: EntityKind, value: &str) -> String {
    match kind {
        EntityKind::Tckn | EntityKind::Vkn | EntityKind::CreditCard => {
            value.chars().filter(char::is_ascii_digit).collect()
        }
        EntityKind::Iban | EntityKind::Phone => value
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect(),
        EntityKind::Email => value.trim().to_lowercase(),
        EntityKind::Plate => value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_uppercase(),
        _ => turkish_fold(value)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use ortu_core::{EntityKind, FailureReason, SecretValue, TokenScope, ValidatorOutcome};

    use super::Tokenizer;
    use crate::key::{SecretKey, StaticKeyProvider};

    // Expected tokens were computed by an independent Python implementation
    // (hmac + hashlib, HKDF written from RFC 5869), never by this code.
    fn key() -> StaticKeyProvider {
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::try_from(i).unwrap();
        }
        StaticKeyProvider::single(SecretKey::from_bytes(bytes))
    }

    fn token(
        scope: TokenScope,
        call: &str,
        kind: EntityKind,
        value: &str,
        validity: ValidatorOutcome,
    ) -> String {
        Tokenizer::new(&key(), "tenant", scope, call)
            .unwrap()
            .token(kind, &SecretValue::new(value), validity)
            .to_string()
    }

    #[test]
    fn independent_vectors() {
        use EntityKind::{FullName, Iban, Phone, Tckn};
        use ValidatorOutcome::Passed;
        let call = TokenScope::Call;
        assert_eq!(
            token(TokenScope::Tenant, "", Tckn, "10000000146", Passed),
            "[TCKN:6bc297fb]"
        );
        assert_eq!(
            token(call, "call-42", Tckn, "10000000146", Passed),
            "[TCKN:df1c492e]"
        );
        assert_eq!(
            token(call, "call-42", Phone, "+905321234567", Passed),
            "[PHONE:ebfa358c]"
        );
        assert_eq!(
            token(call, "call-42", FullName, "ahmet yılmaz", Passed),
            "[NAME:55832c68]"
        );
        assert_eq!(
            token(call, "call-42", Iban, "TR330006100519786457841326", Passed),
            "[IBAN:53db1f0a]"
        );
        let short = ValidatorOutcome::Failed(FailureReason::TooShort);
        assert_eq!(
            token(call, "call-42", Tckn, "100000001", short),
            "[TCKN_INCOMPLETE:dbbe704b]"
        );
    }

    #[test]
    fn call_scope_unlinks_calls_and_tenant_scope_links_them() {
        use EntityKind::Tckn;
        use ValidatorOutcome::Passed;
        assert_eq!(
            token(TokenScope::Call, "call-43", Tckn, "10000000146", Passed),
            "[TCKN:4501f8c7]"
        );
        assert_eq!(
            token(TokenScope::Tenant, "call-1", Tckn, "10000000146", Passed),
            token(TokenScope::Tenant, "call-2", Tckn, "10000000146", Passed)
        );
    }

    #[test]
    fn canonicalization_makes_readings_of_one_value_share_a_token() {
        use ValidatorOutcome::Passed;
        let call = TokenScope::Call;
        assert_eq!(
            token(call, "c", EntityKind::FullName, "AHMET   Yılmaz", Passed),
            token(call, "c", EntityKind::FullName, "ahmet yılmaz", Passed)
        );
        assert_eq!(
            token(
                call,
                "c",
                EntityKind::Iban,
                "tr33 0006 1005 1978 6457 8413 26",
                Passed
            ),
            token(
                call,
                "c",
                EntityKind::Iban,
                "TR330006100519786457841326",
                Passed
            )
        );
        assert_ne!(
            token(call, "c", EntityKind::Phone, "+905321234567", Passed),
            token(call, "c", EntityKind::Vkn, "905321234567", Passed),
            "the kind is part of the MAC input"
        );
    }
}
