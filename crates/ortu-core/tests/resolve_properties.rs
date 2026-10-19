//! Property tests for conflict resolution (algorithm 11; M2 acceptance).

use std::collections::BTreeSet;

use ortu_core::{EntityKind, ResolveInput, resolve};
use proptest::prelude::*;

const KINDS: [EntityKind; 7] = [
    EntityKind::Iban,
    EntityKind::CreditCard,
    EntityKind::Tckn,
    EntityKind::Vkn,
    EntityKind::Phone,
    EntityKind::Plate,
    EntityKind::PersonName,
];

fn input() -> impl Strategy<Value = ResolveInput> {
    // A few fixed scores produce ties, which is where order dependence hides.
    let score = prop_oneof![Just(0.2f32), Just(0.5), Just(0.8), 0.0f32..=1.0];
    (0u32..20, 1u32..8, 0..KINDS.len(), any::<bool>(), score).prop_map(
        |(start, len, kind, validated, score)| ResolveInput {
            words: start..start + len,
            kind: KINDS[kind],
            validated,
            score,
        },
    )
}

/// The parts of a resolution that must not depend on input order.
fn outcome(
    inputs: &[ResolveInput],
    threshold: f32,
) -> (BTreeSet<(u32, u32, EntityKind)>, BTreeSet<u32>) {
    let resolution = resolve(inputs, threshold);
    let winners = resolution
        .winners
        .iter()
        .map(|&i| (inputs[i].words.start, inputs[i].words.end, inputs[i].kind))
        .collect();
    (winners, resolution.masked_words)
}

proptest! {
    #[test]
    fn masked_words_cover_every_qualifying_candidate(
        inputs in prop::collection::vec(input(), 0..12),
        threshold in 0.0f32..=1.0,
    ) {
        let resolution = resolve(&inputs, threshold);
        for candidate in inputs.iter().filter(|c| c.score >= threshold) {
            for word in candidate.words.clone() {
                prop_assert!(resolution.masked_words.contains(&word));
            }
        }
        for word in &resolution.masked_words {
            prop_assert!(inputs.iter().any(|c| c.score >= threshold && c.words.contains(word)));
        }
    }

    #[test]
    fn winners_qualify_and_never_overlap(
        inputs in prop::collection::vec(input(), 0..12),
        threshold in 0.0f32..=1.0,
    ) {
        let resolution = resolve(&inputs, threshold);
        for (n, &a) in resolution.winners.iter().enumerate() {
            prop_assert!(inputs[a].score >= threshold);
            for &b in &resolution.winners[n + 1..] {
                let (x, y) = (&inputs[a].words, &inputs[b].words);
                prop_assert!(x.end <= y.start || y.end <= x.start, "{x:?} overlaps {y:?}");
            }
        }
    }

    #[test]
    fn a_qualifying_candidate_is_either_a_winner_or_suppressed(
        inputs in prop::collection::vec(input(), 0..12),
        threshold in 0.0f32..=1.0,
    ) {
        let resolution = resolve(&inputs, threshold);
        for (i, candidate) in inputs.iter().enumerate() {
            if candidate.score >= threshold {
                let accounted = resolution.winners.contains(&i)
                    || resolution.suppressed.iter().any(|&(loser, _)| loser == i);
                prop_assert!(accounted, "candidate {i} vanished");
            }
        }
    }

    #[test]
    fn the_result_does_not_depend_on_input_order(
        (inputs, shuffled) in prop::collection::vec(input(), 0..12)
            .prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle())),
        threshold in 0.0f32..=1.0,
    ) {
        prop_assert_eq!(outcome(&inputs, threshold), outcome(&shuffled, threshold));
    }
}
