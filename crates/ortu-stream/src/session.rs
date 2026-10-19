//! A live call in Realtime mode: every final turn is masked immediately, with
//! no hold-back, and values the caller spreads over several turns are
//! assembled eagerly
//! ([algorithm 12](https://github.com/aral-by/ortu/blob/main/docs/algorithms/12-cross-turn-assembly.md)).

use std::collections::BTreeSet;
use std::fmt;
use std::mem;
use std::sync::Arc;

use ortu_core::{EntityKind, Turn, TurnAnalysis, ValidatorOutcome};
use ortu_crypto::Tokenizer;
use ortu_normalize::DigitLattice;
use zeroize::Zeroizing;

use crate::engine::Inner;
use crate::error::Error;
use crate::fragment::{self, Fragment};
use crate::map::SessionMap;
use crate::mask::{Found, MaskedTurn, Masker, PostCallResult, WordSpan, render};
use crate::postcall;

/// Session state between turns (algorithm 12, state machine).
#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    /// The assistant asked for a kind; the caller's next turn is read with
    /// that cue.
    Expecting(EntityKind),
    /// The caller has started a value and may continue it in the next turn.
    Open(OpenRun),
}

#[derive(Debug)]
struct OpenRun {
    kind: EntityKind,
    digits: Zeroizing<String>,
    parts: Vec<WordSpan>,
    turns: u8,
}

/// One live call. Holds the session map (token to raw value) in memory only;
/// dropping the session zeroizes it.
pub struct Session {
    pub(crate) inner: Arc<Inner>,
    pub(crate) call_id: String,
    pub(crate) tenant_id: String,
    pub(crate) tokenizer: Tokenizer,
    pub(crate) map: SessionMap,
    turns: Vec<Turn>,
    phase: Phase,
    next_id: u32,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("call_id", &self.call_id)
            .field("tenant_id", &self.tenant_id)
            .field("turns", &self.turns.len())
            .field("tokens", &self.map.len())
            .finish_non_exhaustive()
    }
}

impl Session {
    pub(crate) fn new(
        inner: Arc<Inner>,
        call_id: String,
        tenant_id: String,
    ) -> Result<Self, Error> {
        let tokenizer = inner.tokenizer(&tenant_id, &call_id)?;
        Ok(Self {
            inner,
            call_id,
            tenant_id,
            tokenizer,
            map: SessionMap::default(),
            turns: Vec::new(),
            phase: Phase::Idle,
            next_id: 0,
        })
    }

    /// The call this session belongs to.
    #[must_use]
    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    /// The tenant whose keys and policy apply.
    #[must_use]
    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    /// Masks a final STT turn and returns it at once: there is no hold-back.
    /// Assistant turns are masked too. A value the caller has not finished
    /// is masked as `[KIND_INCOMPLETE:...]`; when a later turn completes it,
    /// that turn carries the token of the whole value.
    ///
    /// # Errors
    ///
    /// Returns an error if the turn's `index` is not greater than the previous
    /// turn's.
    pub fn push(&mut self, turn: Turn) -> Result<MaskedTurn, Error> {
        if let Some(previous) = self.turns.last() {
            if turn.index <= previous.index {
                return Err(Error::TurnOutOfOrder {
                    previous: previous.index,
                    pushed: turn.index,
                });
            }
        }
        let (found, masked_words) = if turn.speaker.is_assistant() {
            self.assistant_turn(&turn)
        } else {
            self.caller_turn(&turn)
        };
        let mut masker = Masker {
            inner: &self.inner,
            tokenizer: &self.tokenizer,
            map: Some(&mut self.map),
        };
        let stored = &self.turns;
        let current = &turn;
        let entities = masker.reports(found, &mut self.next_id, |index| {
            if index == current.index {
                Some(current)
            } else {
                stored.iter().find(|t| t.index == index)
            }
        });
        let masked_turn = MaskedTurn {
            index: turn.index,
            speaker: turn.speaker,
            text: render(&turn, &masked_words, &entities),
            entities,
        };
        self.turns.push(turn);
        Ok(masked_turn)
    }

    /// Ends the call and reprocesses every turn in `PostCall` mode for the
    /// persistent transcript (section 9.1). Realtime decisions are not reused.
    /// The session map and the stored turns are zeroized when the session is
    /// dropped here.
    #[must_use]
    pub fn finish(self) -> PostCallResult {
        postcall::run(&self.inner, &self.tokenizer, &self.turns)
    }

    fn analyze(&self, turn: &Turn, expecting: Option<EntityKind>) -> TurnAnalysis {
        self.inner
            .analyzer
            .analyze_turn(turn, &self.inner.hints(expecting), self.next_id)
    }

    fn assistant_turn(&mut self, turn: &Turn) -> (Vec<Found>, BTreeSet<u32>) {
        let analysis = self.analyze(turn, None);
        // `evet`, `hı hı`: the caller may still be dictating. Asking again for
        // the kind being dictated ("TC kimlik numaranız?") does not interrupt
        // it either; any other assistant turn does.
        let same_kind = matches!(
            (&self.phase, analysis.expecting_next),
            (Phase::Open(run), Some(kind)) if run.kind == kind
        );
        if !analysis.normalized.is_backchannel() && !same_kind {
            self.phase = analysis
                .expecting_next
                .map_or(Phase::Idle, Phase::Expecting);
        }
        let found = analysis
            .entities
            .into_iter()
            .map(|entity| Found::detected(turn, entity))
            .collect();
        (found, analysis.masked_words)
    }

    fn caller_turn(&mut self, turn: &Turn) -> (Vec<Found>, BTreeSet<u32>) {
        let expecting = match &self.phase {
            Phase::Idle => None,
            Phase::Expecting(kind) => Some(*kind),
            Phase::Open(run) => Some(run.kind),
        };
        let analysis = self.analyze(turn, expecting);
        let mut masked = analysis.masked_words.clone();
        let mut found: Vec<Found> = analysis
            .entities
            .iter()
            .cloned()
            .map(|entity| Found::detected(turn, entity))
            .collect();

        // A question or an open run applies to this turn only; whatever
        // happens below decides the next phase.
        let phase = mem::take(&mut self.phase);
        let mut continued = None;
        if let Phase::Open(run) = phase {
            if let Some(lattice) = analysis.normalized.leading_lattice() {
                continued = Some(lattice.words.clone());
                self.continue_run(run, lattice, turn.index, &mut found, &mut masked);
            }
        }

        // Every other unfinished value is masked; the one that ends the turn
        // also opens a run the next turn can continue.
        let trailing = analysis
            .normalized
            .trailing_lattice()
            .map(|lattice| lattice.words.clone());
        for lattice in &analysis.normalized.lattices {
            if continued.as_ref() == Some(&lattice.words) {
                continue;
            }
            let validated = |words: &std::ops::Range<u32>| {
                found.iter().any(|f| {
                    f.validator == ValidatorOutcome::Passed && f.overlaps(turn.index, words)
                })
            };
            let Some(fragment) = fragment::fragment(&analysis, lattice, expecting, validated)
            else {
                continue;
            };
            let opens = trailing.as_ref() == Some(&lattice.words)
                && matches!(self.phase, Phase::Idle)
                && self.inner.max_open_turns > 1;
            self.mask_fragment(
                fragment,
                expecting,
                turn.index,
                opens,
                &mut found,
                &mut masked,
            );
        }
        (found, masked)
    }

    /// The turn starts with digits while a value is open: join them.
    fn continue_run(
        &mut self,
        run: OpenRun,
        lattice: &DigitLattice,
        turn: u32,
        found: &mut Vec<Found>,
        masked: &mut BTreeSet<u32>,
    ) {
        // Prefer the reading that completes the value; otherwise the best one.
        let joined = |digits: &str| -> Zeroizing<String> {
            Zeroizing::new(format!("{}{digits}", run.digits.as_str()))
        };
        let mut choice = None;
        for reading in &lattice.readings {
            let digits = joined(&reading.digits);
            if let Some(assembled) = ortu_tr::assemble(run.kind, &digits) {
                let passed = assembled.outcome == ValidatorOutcome::Passed;
                if passed || choice.is_none() {
                    choice = Some((digits, assembled));
                }
                if passed {
                    break;
                }
            }
        }
        let Some((digits, assembled)) = choice else {
            return;
        };
        found.retain(|f| !f.overlaps(turn, &lattice.words));
        masked.extend(lattice.words.clone());
        let mut parts = run.parts;
        parts.push(WordSpan {
            turn,
            words: lattice.words.clone(),
        });
        let turns = run.turns.saturating_add(1);
        if assembled.needs_more() && turns < self.inner.max_open_turns {
            self.phase = Phase::Open(OpenRun {
                kind: run.kind,
                digits,
                parts: parts.clone(),
                turns,
            });
        }
        found.push(fragment::assembled(
            &self.inner,
            run.kind,
            parts,
            assembled,
            true,
        ));
    }

    /// Masks an unfinished value as `[KIND_INCOMPLETE:...]`; `opens` keeps a
    /// run open for the next turn.
    fn mask_fragment(
        &mut self,
        fragment: Fragment,
        expecting: Option<EntityKind>,
        turn: u32,
        opens: bool,
        found: &mut Vec<Found>,
        masked: &mut BTreeSet<u32>,
    ) {
        found.retain(|f| !f.overlaps(turn, &fragment.words));
        masked.extend(fragment.words.clone());
        let parts = vec![WordSpan {
            turn,
            words: fragment.words,
        }];
        if opens {
            self.phase = Phase::Open(OpenRun {
                kind: fragment.kind,
                digits: fragment.digits,
                parts: parts.clone(),
                turns: 1,
            });
        }
        found.push(fragment::assembled(
            &self.inner,
            fragment.kind,
            parts,
            fragment.assembled,
            expecting == Some(fragment.kind),
        ));
    }
}
