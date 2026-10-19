//! Conflict resolution
//! ([algorithm 11](https://github.com/aral-by/ortu/blob/main/docs/algorithms/11-conflict-resolution.md)).
//!
//! Choosing a label and choosing what to mask are separate decisions: the
//! label goes to the best-ranked candidate, while the masked words are the
//! union of every qualifying candidate. A wrong label can therefore never
//! cause a leak.

use std::cmp::{Ordering, Reverse};
use std::collections::BTreeSet;
use std::ops::Range;

use crate::entity::EntityKind;

/// What conflict resolution needs to know about a scored candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolveInput {
    /// Word indices covered.
    pub words: Range<u32>,
    /// The kind.
    pub kind: EntityKind,
    /// Whether the validator passed.
    pub validated: bool,
    /// The final score.
    pub score: f32,
}

/// The outcome of conflict resolution. Indices refer to the input slice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Resolution {
    /// Candidates that carry a label, in order of their first word.
    pub winners: Vec<usize>,
    /// Every word covered by a qualifying candidate.
    pub masked_words: BTreeSet<u32>,
    /// `(candidate, winner)`: candidates that overlapped a winner, for the
    /// audit trail.
    pub suppressed: Vec<(usize, usize)>,
}

fn overlaps(a: &Range<u32>, b: &Range<u32>) -> bool {
    a.start < b.end && b.start < a.end
}

/// Ranking within a cluster: validated first, then the more specific kind,
/// then the higher score, then the longer span, then the earlier start. The
/// last two make the result independent of input order.
fn rank(a: &ResolveInput, b: &ResolveInput) -> Ordering {
    (a.validated, a.kind.specificity())
        .cmp(&(b.validated, b.kind.specificity()))
        .then_with(|| a.score.total_cmp(&b.score))
        .then_with(|| (a.words.end - a.words.start).cmp(&(b.words.end - b.words.start)))
        .then_with(|| b.words.start.cmp(&a.words.start))
        .then_with(|| b.kind.cmp(&a.kind))
}

/// Resolves overlapping candidates. Candidates scoring at least `threshold`
/// qualify for labeling and masking.
#[must_use]
pub fn resolve(inputs: &[ResolveInput], threshold: f32) -> Resolution {
    let mut order: Vec<usize> = (0..inputs.len())
        .filter(|&i| inputs[i].words.start < inputs[i].words.end)
        .collect();
    order.sort_by_key(|&i| (inputs[i].words.start, Reverse(inputs[i].words.end)));

    let mut resolution = Resolution::default();
    for cluster in clusters(inputs, &order) {
        let mut ranked = cluster;
        ranked.sort_by(|&a, &b| rank(&inputs[b], &inputs[a]));

        let mut winners: Vec<usize> = Vec::new();
        for &i in &ranked {
            let candidate = &inputs[i];
            match winners
                .iter()
                .find(|&&w| overlaps(&inputs[w].words, &candidate.words))
            {
                Some(&winner) => resolution.suppressed.push((i, winner)),
                None if candidate.score >= threshold => winners.push(i),
                None => {}
            }
            if candidate.score >= threshold {
                resolution.masked_words.extend(candidate.words.clone());
            }
        }
        resolution.winners.extend(winners);
    }
    resolution
        .winners
        .sort_by_key(|&i| (inputs[i].words.start, inputs[i].words.end));
    resolution
}

/// Groups candidates, already sorted by start, into clusters of transitively
/// overlapping spans (a sweep line).
fn clusters(inputs: &[ResolveInput], order: &[usize]) -> Vec<Vec<usize>> {
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut current_end = 0;
    for &i in order {
        let words = &inputs[i].words;
        if !current.is_empty() && words.start >= current_end {
            clusters.push(std::mem::take(&mut current));
        }
        current_end = if current.is_empty() {
            words.end
        } else {
            current_end.max(words.end)
        };
        current.push(i);
    }
    if !current.is_empty() {
        clusters.push(current);
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::{ResolveInput, resolve};
    use crate::entity::EntityKind;

    fn input(
        words: std::ops::Range<u32>,
        kind: EntityKind,
        validated: bool,
        score: f32,
    ) -> ResolveInput {
        ResolveInput {
            words,
            kind,
            validated,
            score,
        }
    }

    #[test]
    fn worked_example_from_algorithm_11() {
        let inputs = [
            input(3..21, EntityKind::Iban, true, 0.99),
            input(6..13, EntityKind::Tckn, true, 0.81),
            input(9..15, EntityKind::Phone, true, 0.55),
        ];
        let resolution = resolve(&inputs, 0.3);
        assert_eq!(resolution.winners, [0]);
        assert_eq!(resolution.suppressed, [(1, 0), (2, 0)]);
        assert_eq!(resolution.masked_words, (3..21).collect());
    }

    #[test]
    fn a_weaker_candidate_extending_the_winner_still_masks_its_words() {
        let inputs = [
            input(0..11, EntityKind::Tckn, true, 0.95),
            input(5..13, EntityKind::Phone, false, 0.4),
        ];
        let resolution = resolve(&inputs, 0.3);
        assert_eq!(resolution.winners, [0]);
        assert_eq!(resolution.masked_words, (0..13).collect());
    }

    #[test]
    fn touching_entities_are_both_labeled() {
        let inputs = [
            input(0..10, EntityKind::Phone, true, 0.9),
            input(10..21, EntityKind::Tckn, true, 0.9),
        ];
        assert_eq!(resolve(&inputs, 0.3).winners, [0, 1]);
    }

    #[test]
    fn candidates_below_the_threshold_are_neither_labeled_nor_masked() {
        let inputs = [input(0..3, EntityKind::Tckn, false, 0.1)];
        let resolution = resolve(&inputs, 0.3);
        assert!(resolution.winners.is_empty() && resolution.masked_words.is_empty());
    }
}
