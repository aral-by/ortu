//! Sessions for ortu: the API that voice agents and LLM applications use
//! (docs/systemdesign.md, sections 9 and 10.8; algorithms 12 and 15).
//!
//! - [`Engine`]: detection, policy, keys and the LLM-loop settings, shared by
//!   every session.
//! - [`Session`]: one live call. [`Session::push`] masks each final turn in
//!   Realtime mode without hold-back and assembles values spread over turns;
//!   [`Session::finish`] reprocesses the call in `PostCall` mode for persistent
//!   systems. [`Engine::redact`] runs `PostCall` on a whole transcript.
//! - The LLM boundaries: [`Session::register`] and
//!   [`Session::mask_tool_result`] tokenize what enters the LLM;
//!   [`Session::guard_tool_args`] and [`Session::rehydrate`] (or the streaming
//!   [`Rehydrator`]) resolve what leaves it, through the in-memory session map.
//!
//! This is the only crate that holds state across turns.
//!
//! ```
//! use ortu_core::{EntityKind, SecretValue, Speaker, Turn};
//! use ortu_crypto::{SecretKey, StaticKeyProvider};
//! use ortu_stream::Engine;
//!
//! let engine = Engine::builder()
//!     .key_provider(StaticKeyProvider::single(SecretKey::from_bytes([7; 32])))
//!     .build()?;
//! let mut session = engine.session("call-1", "tenant")?;
//!
//! let bot = Turn::from_text(0, Speaker::Bot, "TC kimlik numaranızı alabilir miyim?", 0, 2000);
//! session.push(bot)?;
//! let caller = Turn::from_text(1, Speaker::Caller, "on sıfır sıfır sıfır sıfır", 2000, 4000);
//! let first = session.push(caller)?;
//! assert!(first.text.starts_with("[TCKN_INCOMPLETE:"));
//!
//! let caller = Turn::from_text(2, Speaker::Caller, "sıfır sıfır yüz kırk altı", 4000, 6000);
//! let second = session.push(caller)?;
//! let token = second.entities[0].replacement.clone();
//! assert!(token.starts_with("[TCKN:"));
//!
//! // The LLM calls a tool with the token; the tool receives the real value.
//! let args = serde_json::json!({ "tckn": token });
//! let released = session.guard_tool_args("query_debt", &args)?;
//! assert_eq!(released["tckn"], "10000000146");
//!
//! // A number the LLM made up never reaches the tool.
//! let invented = serde_json::json!({ "tckn": "10000000164" });
//! assert!(session.guard_tool_args("query_debt", &invented).is_err());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod engine;
mod error;
mod fragment;
mod map;
mod mask;
mod matcher;
mod postcall;
mod rehydrate;
mod session;
mod tools;

pub use engine::{DEFAULT_MAX_OPEN_TURNS, Engine, EngineBuilder, ReadBack};
pub use error::{Error, GuardError};
pub use mask::{EntityReport, MaskedTurn, PostCallResult, RedactedTurn, WordSpan};
pub use rehydrate::{RehydrateStream, Rehydrated, Rehydrator};
pub use session::Session;
