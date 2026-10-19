//! Text normalization for ortu.
//!
//! Turns the raw words of one STT turn into views that detectors can match
//! reliably, without ever modifying the source text:
//!
//! - Turkish case folding and canonical keys
//!   ([algorithm 01](https://github.com/aral-by/ortu/blob/main/docs/algorithms/01-text-canonicalization.md))
//! - filler, interruption and correction marking
//!   ([algorithm 02](https://github.com/aral-by/ortu/blob/main/docs/algorithms/02-filler-and-correction-marking.md))
//! - the spoken-number lattice with a digit-to-word map
//!   ([algorithm 03](https://github.com/aral-by/ortu/blob/main/docs/algorithms/03-spoken-number-lattice.md))
//! - spelled-out letters and symbols (docs/systemdesign.md, section 6.5)
//!
//! The crate is pure: no I/O and no global state.
//!
//! ```
//! use ortu_normalize::{NormalizeConfig, normalize};
//!
//! let words = ["TC'm", "on", "iki", "üç", "yüz", "kırk", "beş"];
//! let turn = normalize(&words, &NormalizeConfig::default());
//!
//! let best = &turn.lattices[0].readings[0];
//! assert_eq!(best.digits, "12345");
//! ```

mod canonical;
mod fold;
mod lexicon;
mod number;
mod roles;
mod spell;

pub use canonical::{Canonicalizer, NormWord};
pub use fold::{flatten, turkish_fold};
pub use number::{DigitLattice, DigitRun, LatticeConfig, NormalizationTrace, build_lattices};
pub use roles::{WordRole, mark_roles};
pub use spell::{SpelledRun, spelled_runs, spoken_symbol};

/// Index of a word within its turn. `ortu-core` maps it to turn and byte
/// coordinates (docs/systemdesign.md, section 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WordRef(pub u32);

/// Configuration of the whole normalization step.
#[derive(Debug, Clone, Default)]
pub struct NormalizeConfig {
    /// Suffix splitting.
    pub canonicalizer: Canonicalizer,
    /// Spoken-number lattice search.
    pub lattice: LatticeConfig,
}

/// Everything normalization produces for one turn.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedTurn {
    /// One canonical view per source word, in order.
    pub words: Vec<NormWord>,
    /// One role per source word, in order.
    pub roles: Vec<WordRole>,
    /// One lattice per run of number words, in order.
    pub lattices: Vec<DigitLattice>,
    /// Runs of spelled letters, in order.
    pub spelled: Vec<SpelledRun>,
}

/// Normalizes the words of one turn.
#[must_use]
pub fn normalize<S: AsRef<str>>(words: &[S], config: &NormalizeConfig) -> NormalizedTurn {
    let words = config.canonicalizer.canonicalize(words);
    let roles = mark_roles(&words);
    let lattices = build_lattices(&words, &roles, &config.lattice);
    let spelled = spelled_runs(&words, &roles);
    NormalizedTurn {
        words,
        roles,
        lattices,
        spelled,
    }
}
