//! Cryptography for ortu.
//!
//! M2 ships keyed tokenization (docs/systemdesign.md, section 10.2;
//! algorithm 13): [`Tokenizer`] issues `[TCKN:7f3a9c2e]`-style tokens with an
//! HMAC-SHA256 under a tenant key, derived per call with HKDF by default, and
//! [`KeyProvider`] supplies the keys. Envelope encryption, the vault, reveal
//! and crypto-shredding follow in M5.
//!
//! All primitives come from the `RustCrypto` ecosystem; custom cryptographic
//! implementations are not allowed.

mod key;
mod token;

pub use key::{EnvKeyProvider, KeyError, KeyProvider, SecretKey, StaticKeyProvider};
pub use token::{Token, Tokenizer};
