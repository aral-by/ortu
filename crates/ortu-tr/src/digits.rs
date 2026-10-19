//! Shared helpers for detectors that read digit lattices.

use std::ops::Range;

use ortu_core::Candidate;
use ortu_normalize::DigitRun;

/// A contiguous slice of a reading that starts and ends on word boundaries.
pub(crate) struct Window<'a> {
    pub(crate) digits: &'a str,
    pub(crate) words: Range<u32>,
    /// Offset of the first digit in the reading.
    pub(crate) offset: usize,
}

/// Windows of exactly `len` digits that never split a word: `üç yüz` produces
/// `300`, and no window may start or end inside those three digits.
pub(crate) fn aligned_windows(run: &DigitRun, len: usize) -> impl Iterator<Item = Window<'_>> {
    let total = run.digits.len();
    let starts = if len == 0 || len > total {
        0..0
    } else {
        0..total - len + 1
    };
    starts.filter_map(move |start| {
        let end = start + len;
        let source = &run.source;
        let starts_word = start == 0 || source[start - 1] != source[start];
        let ends_word = end == total || source[end] != source[end - 1];
        (starts_word && ends_word).then(|| Window {
            digits: &run.digits[start..end],
            words: source[start].0..source[end - 1].0 + 1,
            offset: start,
        })
    })
}

/// Adds a candidate unless an identical one (same kind, words and value) is
/// already present; in that case the higher prior is kept.
pub(crate) fn push_unique(out: &mut Vec<Candidate>, candidate: Candidate) {
    let existing = out.iter_mut().find(|c| {
        c.kind == candidate.kind
            && c.words == candidate.words
            && c.value.expose_secret() == candidate.value.expose_secret()
    });
    match existing {
        Some(c) if c.prior >= candidate.prior => {}
        Some(c) => *c = candidate,
        None => out.push(candidate),
    }
}
