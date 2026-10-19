//! `PostCall` mode: the whole call at once, for persistent systems
//! ([algorithm 12](https://github.com/aral-by/ortu/blob/main/docs/algorithms/12-cross-turn-assembly.md)).
//!
//! Caller turns that continue a number across a turn boundary are analyzed as
//! one: the turn before ends with digits, the turn after starts with digits,
//! and only assistant backchannels (`evet`, `hı hı`) or a repeated question for
//! an identifier lie between them; the group spans at most `max_open_turns`
//! caller turns. The analyzer then sees the joined digits as one lattice, with
//! every cue of the group in reach. Digits after a cue or question that never
//! form a value are masked as unfinished. Realtime decisions are not reused:
//! `PostCall` has strictly more information.

use std::collections::BTreeSet;
use std::ops::Range;

use ortu_core::{EntityKind, Speaker, Turn, TurnAnalysis, ValidatorOutcome};
use ortu_crypto::Tokenizer;
use ortu_normalize::NormalizedTurn;

use crate::engine::Inner;
use crate::fragment::{self, overlap};
use crate::mask::{Found, Masker, PostCallResult, RedactedTurn, WordSpan, render};

/// Turns analyzed together: one assistant turn, or caller turns joined by a
/// number spoken across them.
enum Unit {
    Assistant(usize, Box<TurnAnalysis>),
    Caller {
        members: Vec<usize>,
        expecting: Option<EntityKind>,
    },
}

/// Runs `PostCall` over the turns of one call.
pub(crate) fn run(inner: &Inner, tokenizer: &Tokenizer, turns: &[Turn]) -> PostCallResult {
    let normalized: Vec<NormalizedTurn> =
        turns.iter().map(|t| inner.analyzer.normalize(t)).collect();
    let mut found: Vec<Found> = Vec::new();
    let mut masked: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); turns.len()];

    for unit in units(inner, turns, &normalized) {
        match unit {
            Unit::Assistant(i, analysis) => {
                masked[i].extend(analysis.masked_words.iter().copied());
                found.extend(
                    analysis
                        .entities
                        .into_iter()
                        .map(|entity| Found::detected(&turns[i], entity)),
                );
            }
            Unit::Caller { members, expecting } => {
                let (joined, offsets) = join(turns, &members);
                let analysis = inner
                    .analyzer
                    .analyze_turn(&joined, &inner.hints(expecting), 0);
                let group = Group {
                    joined: &joined,
                    turns,
                    offsets: &offsets,
                };
                group.split(inner, &analysis, expecting, &mut found, &mut masked);
            }
        }
    }

    let mut transforms = Masker {
        inner,
        tokenizer,
        map: None,
    };
    let mut next_id = 0;
    let entities = transforms.reports(found, &mut next_id, |index| {
        turns.iter().find(|t| t.index == index)
    });
    let redacted = turns
        .iter()
        .zip(&masked)
        .map(|(turn, masked)| RedactedTurn {
            index: turn.index,
            speaker: turn.speaker,
            text: render(turn, masked, &entities),
        })
        .collect();
    PostCallResult {
        turns: redacted,
        entities,
    }
}

/// Groups the call into analysis units, in order. The kind an assistant asked
/// for applies to the next caller unit; backchannels keep it.
fn units(inner: &Inner, turns: &[Turn], normalized: &[NormalizedTurn]) -> Vec<Unit> {
    let mut assistant: Vec<Option<TurnAnalysis>> = turns
        .iter()
        .map(|turn| {
            turn.speaker
                .is_assistant()
                .then(|| inner.analyzer.analyze_turn(turn, &inner.hints(None), 0))
        })
        .collect();
    // An assistant turn a dictated number may run across: `evet`, or asking
    // again for an identifier.
    let passable = |k: usize| {
        normalized[k].is_backchannel()
            || assistant[k]
                .as_ref()
                .and_then(|a| a.expecting_next)
                .is_some_and(ortu_tr::assembles)
    };
    let mut groups: Vec<Option<Vec<usize>>> = vec![None; turns.len()];
    let mut grouped = vec![false; turns.len()];
    for i in 0..turns.len() {
        if turns[i].speaker.is_assistant() || grouped[i] {
            continue;
        }
        let mut members = vec![i];
        while members.len() < usize::from(inner.max_open_turns) {
            let last = members[members.len() - 1];
            if normalized[last].trailing_lattice().is_none() {
                break;
            }
            let mut next = last + 1;
            while next < turns.len() && turns[next].speaker.is_assistant() && passable(next) {
                next += 1;
            }
            let continues = next < turns.len()
                && !turns[next].speaker.is_assistant()
                && normalized[next].leading_lattice().is_some();
            if !continues {
                break;
            }
            grouped[next] = true;
            members.push(next);
        }
        groups[i] = Some(members);
    }

    let mut units = Vec::new();
    let mut expecting = None;
    for i in 0..turns.len() {
        if let Some(analysis) = assistant[i].take() {
            if !normalized[i].is_backchannel() {
                expecting = analysis.expecting_next;
            }
            units.push(Unit::Assistant(i, Box::new(analysis)));
        } else if let Some(members) = groups[i].take() {
            units.push(Unit::Caller {
                members,
                expecting: expecting.take(),
            });
        }
    }
    units
}

/// The member turns as one caller turn, and where each member starts in it.
fn join(turns: &[Turn], members: &[usize]) -> (Turn, Vec<(usize, u32)>) {
    let mut words = Vec::new();
    let mut offsets = Vec::with_capacity(members.len());
    for &m in members {
        offsets.push((m, u32::try_from(words.len()).unwrap_or(u32::MAX)));
        words.extend(turns[m].words.iter().cloned());
    }
    let first = &turns[members[0]];
    let last = &turns[members[members.len() - 1]];
    let joined = Turn {
        index: first.index,
        speaker: Speaker::Caller,
        words,
        start_ms: first.start_ms,
        end_ms: last.end_ms,
        timing: first.timing,
    };
    (joined, offsets)
}

/// A joined caller turn and where its members start in it.
struct Group<'a> {
    joined: &'a Turn,
    turns: &'a [Turn],
    offsets: &'a [(usize, u32)],
}

impl Group<'_> {
    /// Words of the joined turn as parts of the member turns.
    fn parts(&self, words: &Range<u32>) -> Vec<WordSpan> {
        self.offsets
            .iter()
            .filter_map(|&(m, start)| {
                let len = u32::try_from(self.turns[m].words.len()).unwrap_or(u32::MAX);
                let lo = words.start.max(start);
                let hi = words.end.min(start.saturating_add(len));
                (lo < hi).then(|| WordSpan {
                    turn: self.turns[m].index,
                    words: lo - start..hi - start,
                })
            })
            .collect()
    }

    /// Maps entities, unfinished values and masked words back to the members.
    fn split(
        &self,
        inner: &Inner,
        analysis: &TurnAnalysis,
        expecting: Option<EntityKind>,
        found: &mut Vec<Found>,
        masked: &mut [BTreeSet<u32>],
    ) {
        let validated = |words: &Range<u32>| {
            analysis.entities.iter().any(|e| {
                e.evidence.validator == ValidatorOutcome::Passed && overlap(&e.words, words)
            })
        };
        let fragments: Vec<_> = analysis
            .normalized
            .lattices
            .iter()
            .filter_map(|lattice| fragment::fragment(analysis, lattice, expecting, validated))
            .collect();

        let mut words: BTreeSet<u32> = analysis.masked_words.clone();
        for entity in &analysis.entities {
            if fragments.iter().any(|f| overlap(&f.words, &entity.words)) {
                continue;
            }
            let parts = self.parts(&entity.words);
            let mut f = Found::detected(self.joined, entity.clone());
            f.cross_turn = entity.evidence.cross_turn || parts.len() > 1;
            f.parts = parts;
            found.push(f);
        }
        for fragment in fragments {
            words.extend(fragment.words.clone());
            let parts = self.parts(&fragment.words);
            found.push(fragment::assembled(
                inner,
                fragment.kind,
                parts,
                fragment.assembled,
                expecting == Some(fragment.kind),
            ));
        }
        for word in words {
            if let Some(&(m, start)) = self.offsets.iter().rev().find(|(_, start)| *start <= word) {
                masked[m].insert(word - start);
            }
        }
    }
}
