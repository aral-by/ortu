//! Key material and key providers (docs/systemdesign.md, section 10.4).

use std::collections::HashMap;
use std::fmt;

use zeroize::Zeroizing;

/// A 256-bit secret key, zeroized on drop and never printed.
#[derive(Clone)]
pub struct SecretKey(Zeroizing<[u8; 32]>);

impl SecretKey {
    /// Wraps raw key bytes.
    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Parses 64 hexadecimal characters.
    ///
    /// # Errors
    ///
    /// Returns [`KeyError::InvalidKey`] if the input is not exactly 64
    /// hexadecimal characters.
    pub fn from_hex(hex: &str) -> Result<Self, KeyError> {
        let hex = hex.trim();
        if hex.len() != 64 || !hex.is_ascii() {
            return Err(KeyError::InvalidKey(
                "expected 64 hexadecimal characters".to_owned(),
            ));
        }
        let mut bytes = Zeroizing::new([0u8; 32]);
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                .map_err(|_| KeyError::InvalidKey("expected hexadecimal characters".to_owned()))?;
        }
        Ok(Self(bytes))
    }

    pub(crate) fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

/// A key could not be obtained or used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// No key is configured for the tenant.
    MissingKey {
        /// The tenant.
        tenant: String,
    },
    /// Key material is malformed.
    InvalidKey(String),
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingKey { tenant } => {
                write!(f, "no token key configured for tenant `{tenant}`")
            }
            Self::InvalidKey(reason) => write!(f, "invalid key: {reason}"),
        }
    }
}

impl std::error::Error for KeyError {}

/// Supplies keys per tenant. Infrastructure-neutral: adapters for a KMS or
/// Vault Transit implement it. The data key for the vault (`data_key`) is
/// added with the vault in M5.
pub trait KeyProvider: Send + Sync {
    /// The tenant's token key (section 10.2).
    ///
    /// # Errors
    ///
    /// Returns an error if the tenant has no key or the key is malformed.
    fn token_key(&self, tenant: &str) -> Result<SecretKey, KeyError>;
}

/// Keys held in memory: tests and embedded setups that load keys themselves.
#[derive(Debug, Clone, Default)]
pub struct StaticKeyProvider {
    keys: HashMap<String, SecretKey>,
    fallback: Option<SecretKey>,
}

impl StaticKeyProvider {
    /// A provider that uses `key` for every tenant.
    #[must_use]
    pub fn single(key: SecretKey) -> Self {
        Self {
            keys: HashMap::new(),
            fallback: Some(key),
        }
    }

    /// Adds a key for one tenant.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>, key: SecretKey) -> Self {
        self.keys.insert(tenant.into(), key);
        self
    }
}

impl KeyProvider for StaticKeyProvider {
    fn token_key(&self, tenant: &str) -> Result<SecretKey, KeyError> {
        self.keys
            .get(tenant)
            .or(self.fallback.as_ref())
            .cloned()
            .ok_or_else(|| KeyError::MissingKey {
                tenant: tenant.to_owned(),
            })
    }
}

/// Development provider: reads `ORTU_TOKEN_KEY_<TENANT>` (tenant uppercased,
/// non-alphanumerics as `_`), falling back to `ORTU_TOKEN_KEY`. Values are 64
/// hexadecimal characters. Not for production: environment variables leak into
/// process listings and crash reports.
#[derive(Debug, Clone, Copy, Default)]
pub struct EnvKeyProvider;

impl EnvKeyProvider {
    fn variable(tenant: &str) -> String {
        let suffix: String = tenant
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        format!("ORTU_TOKEN_KEY_{suffix}")
    }
}

impl KeyProvider for EnvKeyProvider {
    fn token_key(&self, tenant: &str) -> Result<SecretKey, KeyError> {
        let value = std::env::var(Self::variable(tenant))
            .or_else(|_| std::env::var("ORTU_TOKEN_KEY"))
            .map_err(|_| KeyError::MissingKey {
                tenant: tenant.to_owned(),
            })?;
        SecretKey::from_hex(&Zeroizing::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::{EnvKeyProvider, KeyError, KeyProvider, SecretKey, StaticKeyProvider};

    #[test]
    fn hex_keys_parse_and_never_print() {
        let hex = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
        let key = SecretKey::from_hex(hex).unwrap();
        assert_eq!(key.expose()[31], 0x1f);
        assert_eq!(format!("{key:?}"), "SecretKey(<redacted>)");
        assert!(SecretKey::from_hex("abc").is_err());
        assert!(SecretKey::from_hex(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn static_provider_prefers_the_tenant_key() {
        let a = SecretKey::from_bytes([1; 32]);
        let b = SecretKey::from_bytes([2; 32]);
        let provider = StaticKeyProvider::single(a).with_tenant("istanbul", b);
        assert_eq!(provider.token_key("istanbul").unwrap().expose()[0], 2);
        assert_eq!(provider.token_key("ankara").unwrap().expose()[0], 1);
        let empty = StaticKeyProvider::default();
        assert_eq!(
            empty.token_key("x").unwrap_err(),
            KeyError::MissingKey {
                tenant: "x".to_owned()
            }
        );
    }

    #[test]
    fn env_variable_names_are_sanitized() {
        assert_eq!(
            EnvKeyProvider::variable("belediye-1"),
            "ORTU_TOKEN_KEY_BELEDIYE_1"
        );
    }
}
