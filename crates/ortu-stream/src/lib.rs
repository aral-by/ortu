//! Sessions for ortu.
//!
//! The session API used by voice agents and LLM applications: `Realtime` and
//! `PostCall` processing modes, cross-turn assembly, the in-memory session map,
//! rehydration, streaming rehydration and the tool-argument provenance check
//! (docs/systemdesign.md, sections 9 and 10.8; algorithms 12 and 15).
//!
//! This is the only crate that holds state across turns. Implementation lands
//! in milestone M3.
