//! Text normalization for ortu.
//!
//! Turns raw STT words into views that detectors can match reliably, without
//! ever modifying the source text:
//!
//! - Turkish case folding and canonical keys
//!   ([docs/algorithms/01](https://github.com/aral-by/ortu/blob/main/docs/algorithms/01-text-canonicalization.md))
//! - filler, interruption and correction marking
//!   ([02](https://github.com/aral-by/ortu/blob/main/docs/algorithms/02-filler-and-correction-marking.md))
//! - the spoken-number lattice with a digit-to-word offset map
//!   ([03](https://github.com/aral-by/ortu/blob/main/docs/algorithms/03-spoken-number-lattice.md))
//!
//! The crate is pure: no I/O, no global state and no dependencies.
//! Implementation lands in milestone M1.
