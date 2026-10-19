//! Core of ortu: the data model and the pipeline that every other crate
//! plugs into.
//!
//! Owns the transcript and span model, `SecretValue`, the `Detector` and
//! `Validator` traits, cue scoring, conflict resolution and policy decisions
//! (docs/systemdesign.md, sections 5, 7.1 and 8).
//!
//! Like `ortu-normalize`, this crate performs no I/O and holds no global state.
//! Implementation lands in milestone M2.
