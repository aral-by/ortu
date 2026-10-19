//! Transcript data model (docs/systemdesign.md, sections 5.1 and 5.2).

use std::ops::Range;

/// Where text comes from. Selects normalization and span defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Channel {
    /// STT output: spoken numbers, fillers, word timestamps, audio redaction.
    #[default]
    Speech,
    /// Written text: casing and punctuation are strong cues; no audio redaction.
    Text,
}

/// Who spoke a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Speaker {
    /// The citizen or customer.
    Caller,
    /// A human agent.
    Agent,
    /// A voice or chat bot.
    Bot,
    /// Unknown speaker.
    Unknown,
}

impl Speaker {
    /// Whether the speaker works for the organization (its questions open a
    /// cross-turn cue for the caller's next turn).
    #[must_use]
    pub fn is_assistant(self) -> bool {
        matches!(self, Self::Agent | Self::Bot)
    }
}

/// One STT word.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    /// The word as STT wrote it.
    pub text: String,
    /// Start time in milliseconds.
    pub start_ms: u64,
    /// End time in milliseconds.
    pub end_ms: u64,
    /// STT confidence in `0.0..=1.0`, when the engine provides it.
    pub confidence: Option<f32>,
}

/// Whether word timestamps came from STT or were estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TimingQuality {
    /// Word-level timestamps from STT.
    #[default]
    Exact,
    /// Spread evenly over the turn; audio padding is increased (section 11.1).
    Estimated,
}

/// One uninterrupted utterance by one speaker.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// Position of the turn in its call.
    pub index: u32,
    /// Who spoke.
    pub speaker: Speaker,
    /// The words, in order.
    pub words: Vec<Word>,
    /// Start time in milliseconds.
    pub start_ms: u64,
    /// End time in milliseconds.
    pub end_ms: u64,
    /// Where the word timestamps came from.
    pub timing: TimingQuality,
}

impl Turn {
    /// Builds a turn from text without word timestamps: the turn's duration is
    /// spread evenly over its words, and the timing is marked as estimated.
    #[must_use]
    pub fn from_text(index: u32, speaker: Speaker, text: &str, start_ms: u64, end_ms: u64) -> Self {
        let raw: Vec<&str> = text.split_whitespace().collect();
        let count = u64::try_from(raw.len()).unwrap_or(u64::MAX).max(1);
        let duration = end_ms.saturating_sub(start_ms);
        let words = raw
            .iter()
            .zip(0u64..)
            .map(|(text, i)| Word {
                text: (*text).to_owned(),
                start_ms: start_ms + duration * i / count,
                end_ms: start_ms + duration * (i + 1) / count,
                confidence: None,
            })
            .collect();
        Self {
            index,
            speaker,
            words,
            start_ms,
            end_ms,
            timing: TimingQuality::Estimated,
        }
    }

    /// The turn text: the words joined by single spaces. All byte offsets in
    /// ortu refer to this string (section 5.2).
    #[must_use]
    pub fn text(&self) -> String {
        self.words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Mean STT confidence over a word range, if any word in it has one.
    #[must_use]
    pub fn mean_confidence(&self, words: Range<u32>) -> Option<f32> {
        let start = usize::try_from(words.start).ok()?;
        let end = usize::try_from(words.end).ok()?;
        let values: Vec<f32> = self
            .words
            .get(start..end)?
            .iter()
            .filter_map(|word| word.confidence)
            .collect();
        if values.is_empty() {
            return None;
        }
        let count = u16::try_from(values.len()).ok()?;
        Some(values.iter().sum::<f32>() / f32::from(count))
    }
}

/// A whole call, as the batch API receives it (section 5.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// Opaque call identifier; not personal data. Derives call-scoped token keys.
    pub call_id: String,
    /// The tenant whose keys and policy apply.
    pub tenant_id: String,
    /// Where the text comes from.
    pub channel: Channel,
    /// The turns, in order.
    pub turns: Vec<Turn>,
}

/// Exact position in the source text: UTF-8 byte ranges of the turn texts
/// (section 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    /// Turn where the span starts.
    pub turn_start: u32,
    /// First byte, inclusive.
    pub byte_start: u32,
    /// Turn where the span ends.
    pub turn_end: u32,
    /// Last byte, exclusive.
    pub byte_end: u32,
}

impl SourceSpan {
    /// A span inside a single turn.
    #[must_use]
    pub fn within_turn(turn: u32, bytes: Range<u32>) -> Self {
        Self {
            turn_start: turn,
            byte_start: bytes.start,
            turn_end: turn,
            byte_end: bytes.end,
        }
    }
}

/// How a span is aligned to the source (section 5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanSnap {
    /// Speech default: widened to every word it touches.
    #[default]
    Word,
    /// Text default: only the value itself.
    Exact,
}

#[cfg(test)]
mod tests {
    use super::{Speaker, TimingQuality, Turn};

    #[test]
    fn from_text_spreads_time_and_marks_it_estimated() {
        let turn = Turn::from_text(0, Speaker::Caller, "bir iki  üç", 1000, 1300);
        assert_eq!(turn.timing, TimingQuality::Estimated);
        let times: Vec<(u64, u64)> = turn.words.iter().map(|w| (w.start_ms, w.end_ms)).collect();
        assert_eq!(times, [(1000, 1100), (1100, 1200), (1200, 1300)]);
        assert_eq!(turn.text(), "bir iki üç");
    }

    #[test]
    fn mean_confidence_ignores_words_without_one() {
        let mut turn = Turn::from_text(0, Speaker::Caller, "a b c", 0, 3);
        turn.words[0].confidence = Some(0.8);
        turn.words[2].confidence = Some(0.6);
        assert!((turn.mean_confidence(0..3).unwrap() - 0.7).abs() < 1e-6);
        assert_eq!(turn.mean_confidence(1..2), None);
    }
}
