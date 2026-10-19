//! Spoken-number lattice
//! ([algorithm 03](https://github.com/aral-by/ortu/blob/main/docs/algorithms/03-spoken-number-lattice.md)).
//!
//! Spoken Turkish numbers are ambiguous: `yüz yirmi` is `120` as one number or
//! `10020` as "one hundred, twenty". Instead of guessing, a beam search keeps
//! the best few readings with a prior, and validators downstream pick the one
//! that passes its checksum.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::WordRef;
use crate::canonical::NormWord;
use crate::lexicon::{FLATTENED_NUMBER_WORDS, NUMBER_WORDS, NumberWord, lookup};
use crate::roles::WordRole;

/// Cost of one forced split (a split where the grammar allowed extension), in
/// thousandths. Costs are integers so that ordering is exact and deterministic.
const SPLIT_COST: u32 = 1000;

/// Cost of a word recognized only through its flattened match key.
const FALLBACK_COST: u32 = 700;

/// Runs longer than this many number words are cut into consecutive runs. Real
/// identifiers need far fewer (a 26-character IBAN read in groups is about 15
/// words); the cap bounds the work on adversarial input.
const MAX_RUN_WORDS: usize = 64;

/// Longest digit token accepted as a literal group (`0532`, `12`).
const MAX_DIGIT_TOKEN: usize = 32;

/// Tuning of the lattice search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LatticeConfig {
    /// Hypotheses kept after each word (`B`).
    pub beam_width: usize,
    /// Distinct readings returned per run (`K`).
    pub max_readings: usize,
    /// Consecutive filler or interruption words a run may skip.
    pub max_gap_tokens: usize,
}

impl Default for LatticeConfig {
    fn default() -> Self {
        Self {
            beam_width: 32,
            max_readings: 8,
            max_gap_tokens: 4,
        }
    }
}

/// All readings of one run of number words.
#[derive(Debug, Clone, PartialEq)]
pub struct DigitLattice {
    /// Word indices covered by the run, from its first to its last number word.
    pub words: Range<u32>,
    /// Readings ordered by prior, best first. Never empty.
    pub readings: Vec<DigitRun>,
    /// Index (in the same turn) of the run this one corrects, when a correction
    /// marker (`yok yok`, `pardon`) separates them.
    pub overrides: Option<usize>,
}

/// One reading of a run.
#[derive(Debug, Clone, PartialEq)]
pub struct DigitRun {
    /// The digit string.
    pub digits: String,
    /// `source[i]` is the word that produced `digits[i]`.
    pub source: Vec<WordRef>,
    /// `exp(-(cost - best cost))`: 1.0 for the best reading, lower for others.
    pub prior: f32,
    /// How the reading was formed, for the audit trail.
    pub trace: NormalizationTrace,
}

/// How a reading was formed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NormalizationTrace {
    /// Word ranges of the groups, in order (`on iki` / `üç yüz kırk beş`).
    pub groups: Vec<Range<u32>>,
    /// Splits made where the grammar allowed the group to continue.
    pub forced_splits: u16,
    /// Words recognized only through their flattened match key.
    pub fallback_lookups: u16,
    /// Total cost in thousandths.
    pub cost: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Lexeme {
    Zero,
    Unit(u8),
    Ten(u8),
    Hundred,
    /// `bin`, `milyon`, `milyar`: the power of ten they scale by.
    Scale(u8),
    /// A digit token written by STT (`12`, `0532`, the stem of `12'de`).
    Digits(String),
}

impl From<NumberWord> for Lexeme {
    fn from(word: NumberWord) -> Self {
        match word {
            NumberWord::Zero => Self::Zero,
            NumberWord::Unit(u) => Self::Unit(u),
            NumberWord::Ten(t) => Self::Ten(t),
            NumberWord::Hundred => Self::Hundred,
            NumberWord::Scale(power) => Self::Scale(power),
        }
    }
}

/// The lexeme of a word and whether it was found only through the fallback.
fn lexeme(word: &NormWord) -> Option<(Lexeme, bool)> {
    let stem = word.stem();
    if !stem.is_empty() && stem.len() <= MAX_DIGIT_TOKEN && stem.bytes().all(|b| b.is_ascii_digit())
    {
        return Some((Lexeme::Digits(stem.to_owned()), false));
    }
    if let Some(number) = lookup(NUMBER_WORDS, stem) {
        return Some((number.into(), false));
    }
    // `uç` ("tip") is a different word from `üç` ("three"); only a word written
    // entirely without Turkish letters (`uc`) may be a flattened number word.
    if word.is_plain_ascii_form() {
        return lookup(FLATTENED_NUMBER_WORDS, stem).map(|number| (number.into(), true));
    }
    None
}

/// Whether the word is a number word or a digit token.
pub(crate) fn is_number_word(word: &NormWord) -> bool {
    lexeme(word).is_some()
}

#[derive(Debug, Clone)]
struct Item {
    lexeme: Lexeme,
    word: u32,
    fallback: bool,
}

/// One digit position of a spoken group: the digit and the word that set it.
type Slot = Option<(u8, u32)>;

/// Hundreds, tens and units of one three-digit segment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Segment {
    hundreds: Slot,
    tens: Slot,
    units: Slot,
}

impl Segment {
    fn is_empty(&self) -> bool {
        self.hundreds.is_none() && self.tens.is_none() && self.units.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Group {
    /// `sıfır` or a digit token; never extended.
    Literal { digits: String, word: u32 },
    /// A spoken number such as `iki bin üç yüz kırk beş`.
    Spoken {
        /// Completed segments and the power of ten they were scaled by.
        scaled: Vec<(u8, Segment)>,
        current: Segment,
        last_scale: Option<u8>,
    },
}

impl Group {
    fn open(item: &Item) -> Self {
        let word = item.word;
        let spoken = |current| Self::Spoken {
            scaled: Vec::new(),
            current,
            last_scale: None,
        };
        match &item.lexeme {
            Lexeme::Zero => Self::Literal {
                digits: "0".to_owned(),
                word,
            },
            Lexeme::Digits(digits) => Self::Literal {
                digits: digits.clone(),
                word,
            },
            Lexeme::Unit(u) => spoken(Segment {
                units: Some((*u, word)),
                ..Segment::default()
            }),
            Lexeme::Ten(t) => spoken(Segment {
                tens: Some((*t, word)),
                ..Segment::default()
            }),
            Lexeme::Hundred => spoken(Segment {
                hundreds: Some((1, word)),
                ..Segment::default()
            }),
            // `bin` on its own is one thousand.
            Lexeme::Scale(power) => Self::Spoken {
                scaled: vec![(
                    *power,
                    Segment {
                        units: Some((1, word)),
                        ..Segment::default()
                    },
                )],
                current: Segment::default(),
                last_scale: Some(*power),
            },
        }
    }

    /// Whether `lexeme` continues this group: Turkish numbers compose from the
    /// largest magnitude to the smallest.
    fn can_extend(&self, lexeme: &Lexeme) -> bool {
        let Self::Spoken {
            scaled,
            current,
            last_scale,
        } = self
        else {
            return false;
        };
        let after_scale = !scaled.is_empty();
        match lexeme {
            Lexeme::Zero | Lexeme::Digits(_) => false,
            Lexeme::Unit(_) => {
                current.units.is_none()
                    && (current.tens.is_some() || current.hundreds.is_some() || after_scale)
            }
            Lexeme::Ten(_) => {
                current.units.is_none()
                    && current.tens.is_none()
                    && (current.hundreds.is_some() || after_scale)
            }
            // Turkish says `yüz` and `bin`, never `bir yüz` or `bir bin`, so a lone
            // `bir` before them is a separate digit. `bir milyon` is correct Turkish.
            Lexeme::Hundred => {
                current.hundreds.is_none()
                    && current.tens.is_none()
                    && (current.units.is_some_and(|(unit, _)| unit >= 2)
                        || (after_scale && current.is_empty()))
            }
            Lexeme::Scale(power) => {
                let lone_one = current.hundreds.is_none()
                    && current.tens.is_none()
                    && current.units.is_some_and(|(unit, _)| unit == 1);
                !current.is_empty()
                    && (*power != 3 || !lone_one)
                    && last_scale.is_none_or(|last| *power < last)
            }
        }
    }

    fn extend(&mut self, item: &Item) {
        let Self::Spoken {
            scaled,
            current,
            last_scale,
        } = self
        else {
            return;
        };
        let word = item.word;
        match &item.lexeme {
            Lexeme::Unit(u) => current.units = Some((*u, word)),
            Lexeme::Ten(t) => current.tens = Some((*t, word)),
            // `üç yüz`: the unit becomes the hundreds digit; `yüz` alone is one hundred.
            Lexeme::Hundred => current.hundreds = Some(current.units.take().unwrap_or((1, word))),
            Lexeme::Scale(power) => {
                scaled.push((*power, std::mem::take(current)));
                *last_scale = Some(*power);
            }
            Lexeme::Zero | Lexeme::Digits(_) => {}
        }
    }

    /// Appends the group's digits. A position no word set (the zeros of
    /// `üç yüz`) is attributed to the nearest word setting a higher position.
    fn render(&self, digits: &mut String, sources: &mut Vec<WordRef>) {
        match self {
            Self::Literal {
                digits: literal,
                word,
            } => {
                digits.push_str(literal);
                sources.extend(std::iter::repeat_n(WordRef(*word), literal.len()));
            }
            Self::Spoken {
                scaled, current, ..
            } => {
                let mut positions: BTreeMap<u8, (u8, u32)> = BTreeMap::new();
                let segments = scaled
                    .iter()
                    .map(|(power, segment)| (*power, segment))
                    .chain(std::iter::once((0, current)));
                for (power, segment) in segments {
                    for (offset, slot) in
                        [(2, segment.hundreds), (1, segment.tens), (0, segment.units)]
                    {
                        if let Some(value) = slot {
                            positions.insert(power + offset, value);
                        }
                    }
                }
                let Some((&top, &(_, mut word))) = positions.last_key_value() else {
                    return;
                };
                for power in (0..=top).rev() {
                    let digit = match positions.get(&power) {
                        Some(&(digit, setter)) => {
                            word = setter;
                            digit
                        }
                        None => 0,
                    };
                    digits.push(char::from(b'0' + digit));
                    sources.push(WordRef(word));
                }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Hypothesis {
    digits: String,
    sources: Vec<WordRef>,
    /// The open group and the index of its first word.
    group: Option<(Group, u32)>,
    last_word: u32,
    groups: Vec<Range<u32>>,
    cost: u32,
    forced_splits: u16,
    fallbacks: u16,
}

impl Hypothesis {
    fn close(&mut self) {
        if let Some((group, first)) = self.group.take() {
            group.render(&mut self.digits, &mut self.sources);
            let end = self.last_word.saturating_add(1);
            self.groups.push(first..end);
        }
    }

    fn open(&mut self, item: &Item) {
        self.group = Some((Group::open(item), item.word));
        self.last_word = item.word;
    }

    fn same_state(&self, other: &Self) -> bool {
        self.digits == other.digits && self.group == other.group
    }
}

/// Expands every hypothesis with the next word: extend the open group, or
/// close it and start a new one.
fn advance(beam: &[Hypothesis], item: &Item) -> Vec<Hypothesis> {
    let mut next = Vec::with_capacity(beam.len() * 2);
    for hypothesis in beam {
        let mut base = hypothesis.clone();
        if item.fallback {
            base.cost = base.cost.saturating_add(FALLBACK_COST);
            base.fallbacks = base.fallbacks.saturating_add(1);
        }
        let extendable = base
            .group
            .as_ref()
            .is_some_and(|(group, _)| group.can_extend(&item.lexeme));
        if extendable {
            let mut extended = base.clone();
            if let Some((group, _)) = extended.group.as_mut() {
                group.extend(item);
            }
            extended.last_word = item.word;
            next.push(extended);

            base.cost = base.cost.saturating_add(SPLIT_COST);
            base.forced_splits = base.forced_splits.saturating_add(1);
        }
        base.close();
        base.open(item);
        next.push(base);
    }
    next
}

/// Keeps the `width` cheapest distinct hypotheses. The sort is stable, so ties
/// keep generation order and the result is deterministic.
fn prune(mut hypotheses: Vec<Hypothesis>, width: usize) -> Vec<Hypothesis> {
    hypotheses.sort_by_key(|h| h.cost);
    let mut kept: Vec<Hypothesis> = Vec::with_capacity(width);
    for hypothesis in hypotheses {
        if kept.len() == width {
            break;
        }
        if !kept.iter().any(|k| k.same_state(&hypothesis)) {
            kept.push(hypothesis);
        }
    }
    kept
}

// Priors are in (0, 1]; single precision is more than enough for them.
#[allow(clippy::cast_possible_truncation)]
fn prior_from_cost(delta: u32) -> f32 {
    (-f64::from(delta) / f64::from(SPLIT_COST)).exp() as f32
}

fn parse_run(items: &[Item], config: &LatticeConfig) -> Vec<DigitRun> {
    let mut beam = vec![Hypothesis::default()];
    for item in items {
        beam = prune(advance(&beam, item), config.beam_width.max(1));
    }
    let mut finals: Vec<Hypothesis> = beam
        .into_iter()
        .map(|mut hypothesis| {
            hypothesis.close();
            hypothesis
        })
        .collect();
    finals.sort_by_key(|h| h.cost);

    let best = finals.first().map_or(0, |h| h.cost);
    let mut readings: Vec<DigitRun> = Vec::new();
    for hypothesis in finals {
        if readings.len() == config.max_readings.max(1) {
            break;
        }
        if readings.iter().any(|r| r.digits == hypothesis.digits) {
            continue;
        }
        readings.push(DigitRun {
            prior: prior_from_cost(hypothesis.cost - best),
            trace: NormalizationTrace {
                groups: hypothesis.groups,
                forced_splits: hypothesis.forced_splits,
                fallback_lookups: hypothesis.fallbacks,
                cost: hypothesis.cost,
            },
            digits: hypothesis.digits,
            source: hypothesis.sources,
        });
    }
    readings
}

/// Splits a turn into runs of number words and builds a lattice for each.
///
/// Fillers and interruptions inside a run are skipped (at most
/// `max_gap_tokens` in a row); any other word ends the run. A correction marker
/// ends the run and links the next run to it through
/// [`DigitLattice::overrides`].
#[must_use]
pub fn build_lattices(
    words: &[NormWord],
    roles: &[WordRole],
    config: &LatticeConfig,
) -> Vec<DigitLattice> {
    let mut builder = RunBuilder {
        config,
        lattices: Vec::new(),
        items: Vec::new(),
        gap: 0,
        pending_override: None,
    };
    for (index, word) in words.iter().enumerate() {
        match roles.get(index).copied().unwrap_or(WordRole::Content) {
            WordRole::Filler | WordRole::Interruption => builder.gap_word(),
            WordRole::Correction => builder.correction(),
            WordRole::Content => match lexeme(word) {
                Some((lexeme, fallback)) => builder.push(Item {
                    lexeme,
                    word: word.word.0,
                    fallback,
                }),
                None => builder.content_word(),
            },
        }
    }
    builder.flush();
    builder.lattices
}

struct RunBuilder<'a> {
    config: &'a LatticeConfig,
    lattices: Vec<DigitLattice>,
    items: Vec<Item>,
    gap: usize,
    pending_override: Option<usize>,
}

impl RunBuilder<'_> {
    fn push(&mut self, item: Item) {
        if self.items.len() == MAX_RUN_WORDS {
            self.flush();
        }
        self.items.push(item);
        self.gap = 0;
    }

    fn gap_word(&mut self) {
        if self.items.is_empty() {
            return;
        }
        self.gap += 1;
        if self.gap > self.config.max_gap_tokens {
            self.flush();
        }
    }

    fn correction(&mut self) {
        self.flush();
        if !self.lattices.is_empty() {
            self.pending_override = Some(self.lattices.len() - 1);
        }
    }

    fn content_word(&mut self) {
        self.flush();
        self.pending_override = None;
    }

    fn flush(&mut self) {
        let (Some(first), Some(last)) = (self.items.first(), self.items.last()) else {
            return;
        };
        let words = first.word..last.word.saturating_add(1);
        let readings = parse_run(&self.items, self.config);
        self.lattices.push(DigitLattice {
            words,
            readings,
            overrides: self.pending_override.take(),
        });
        self.items.clear();
        self.gap = 0;
    }
}
