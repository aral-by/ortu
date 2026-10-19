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
//! - backchannel recognition for cross-turn assembly
//!   ([algorithm 12](https://github.com/aral-by/ortu/blob/main/docs/algorithms/12-cross-turn-assembly.md))
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

impl NormalizedTurn {
    /// Index of the first content word (not a filler, interruption or
    /// correction marker).
    #[must_use]
    pub fn first_content(&self) -> Option<usize> {
        self.roles
            .iter()
            .position(|role| *role == WordRole::Content)
    }

    /// Index of the last content word.
    #[must_use]
    pub fn last_content(&self) -> Option<usize> {
        self.roles
            .iter()
            .rposition(|role| *role == WordRole::Content)
    }

    /// Whether the turn only acknowledges (`evet`, `hı hı`, `tamam`, fillers):
    /// an assistant saying this does not interrupt a value the caller is
    /// dictating. An empty turn counts as a backchannel.
    #[must_use]
    pub fn is_backchannel(&self) -> bool {
        self.words.iter().zip(&self.roles).all(|(word, role)| {
            *role != WordRole::Content || lexicon::BACKCHANNELS.contains(&word.folded.as_str())
        })
    }

    /// The lattice whose first word is the turn's first content word: the
    /// caller continues a number from an earlier turn.
    #[must_use]
    pub fn leading_lattice(&self) -> Option<&DigitLattice> {
        let first = u32::try_from(self.first_content()?).ok()?;
        self.lattices
            .iter()
            .find(|lattice| lattice.words.start == first)
    }

    /// The lattice whose last word is the turn's last content word: the
    /// caller may continue the number in the next turn.
    #[must_use]
    pub fn trailing_lattice(&self) -> Option<&DigitLattice> {
        let last = u32::try_from(self.last_content()?).ok()?;
        self.lattices
            .iter()
            .find(|lattice| lattice.words.end == last.saturating_add(1))
    }
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

#[cfg(test)]
mod tests {
    use super::{NormalizeConfig, normalize};

    fn turn(text: &str) -> super::NormalizedTurn {
        let words: Vec<&str> = text.split_whitespace().collect();
        normalize(&words, &NormalizeConfig::default())
    }

    #[test]
    fn backchannels_only_acknowledge() {
        for text in ["evet", "Hı hı", "tamam, devam edin", "ııı evet", ""] {
            assert!(turn(text).is_backchannel(), "{text:?}");
        }
        for text in ["evet, doğum tarihiniz?", "TC kimlik numaranız", "kırk altı"] {
            assert!(!turn(text).is_backchannel(), "{text:?}");
        }
    }

    #[test]
    fn leading_and_trailing_lattices_skip_fillers() {
        let continued = turn("ııı kırk altı tamam mı");
        assert_eq!(
            continued.leading_lattice().map(|l| l.words.clone()),
            Some(1..3)
        );
        assert!(continued.trailing_lattice().is_none());

        let open = turn("TC'm on iki üç ııı");
        assert_eq!(open.trailing_lattice().map(|l| l.words.clone()), Some(1..4));
        assert!(open.leading_lattice().is_none());
    }
}
