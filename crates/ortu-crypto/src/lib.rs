//! Cryptography for ortu.
//!
//! HMAC-based tokens with call or tenant scope, AES-256-GCM envelope
//! encryption, the `KeyProvider` and `VaultStore` traits, reveal and
//! crypto-shredding (docs/systemdesign.md, section 10; algorithms 13 and 14).
//!
//! All primitives come from the `RustCrypto` ecosystem; custom cryptographic
//! implementations are not allowed. Implementation lands in milestones M2
//! (tokens) and M5 (vault).
