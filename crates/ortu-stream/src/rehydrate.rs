//! Rehydration of LLM replies for TTS or chat
//! ([algorithm 15](https://github.com/aral-by/ortu/blob/main/docs/algorithms/15-rehydration-and-provenance.md)):
//! tokens become real values according to the read-back policy, and a token
//! the session never issued fails instead of passing through.

use ortu_core::SecretValue;
use zeroize::Zeroizing;

use crate::engine::ReadBack;
use crate::error::GuardError;
use crate::map::Lookup;
use crate::mask::partial;
use crate::session::Session;

/// Rehydrated text. It holds real values, so it is a [`SecretValue`]: it can
/// be handed to TTS or a chat client, but not logged by accident.
#[derive(Debug, Clone)]
pub struct Rehydrated {
    /// The text with tokens replaced.
    pub text: SecretValue,
    /// How many tokens were replaced.
    pub replaced: usize,
}

impl Session {
    /// Replaces the tokens in an LLM reply according to the read-back policy:
    /// names in full, identifiers partially (`*******0146`).
    ///
    /// # Errors
    ///
    /// Returns [`GuardError::UnknownToken`] or [`GuardError::AmbiguousToken`]
    /// for a token this session cannot resolve.
    pub fn rehydrate(&self, text: &str) -> Result<Rehydrated, GuardError> {
        self.rehydrate_within(text, None, None)
    }

    /// A streaming rehydrator for a reply that arrives in chunks.
    #[must_use]
    pub fn rehydrator(&self) -> Rehydrator<'_> {
        Rehydrator {
            session: self,
            stream: RehydrateStream::default(),
        }
    }

    /// Rehydrates `text`, which sits between `before` and `after` in a stream.
    fn rehydrate_within(
        &self,
        text: &str,
        before: Option<char>,
        after: Option<char>,
    ) -> Result<Rehydrated, GuardError> {
        let mut out = Zeroizing::new(String::with_capacity(text.len()));
        let mut last = 0;
        let mut replaced = 0;
        for found in self.inner.matcher.find_all(text, before, after) {
            let entry =
                self.map
                    .get(&found.label, &found.digest)
                    .map_err(|lookup| match lookup {
                        Lookup::Unknown => GuardError::UnknownToken {
                            label: found.label.clone(),
                        },
                        Lookup::Ambiguous => GuardError::AmbiguousToken {
                            label: found.label.clone(),
                        },
                    })?;
            out.push_str(&text[last..found.range.start]);
            match self.inner.read_back(entry.kind) {
                ReadBack::Full => out.push_str(entry.value.expose_secret()),
                ReadBack::Partial => out.push_str(&partial(entry.value.expose_secret())),
            }
            last = found.range.end;
            replaced += 1;
        }
        out.push_str(&text[last..]);
        Ok(Rehydrated {
            text: SecretValue::new(out.as_str()),
            replaced,
        })
    }
}

/// The buffer of a streaming rehydration, independent of the session borrow
/// so that bindings can keep it next to a session handle. Only a suffix that
/// may still become a token is held back (at most one token length); every
/// other character is released at once, so TTS latency is unaffected.
#[derive(Debug, Default)]
pub struct RehydrateStream {
    buffer: Zeroizing<String>,
    before: Option<char>,
}

impl RehydrateStream {
    /// Adds a chunk and returns the text that is now final.
    ///
    /// # Errors
    ///
    /// Returns an error for a token the session cannot resolve.
    pub fn feed(&mut self, session: &Session, chunk: &str) -> Result<SecretValue, GuardError> {
        self.buffer.push_str(chunk);
        let hold = self.hold_from(session);
        let ready = Zeroizing::new(self.buffer[..hold].to_owned());
        self.buffer.replace_range(..hold, "");
        // The held text is already known: its first character decides whether
        // a token at the end of `ready` is glued to a word.
        let after = self.buffer.chars().next();
        self.release(session, &ready, after)
    }

    /// Releases whatever is still held at the end of the reply.
    ///
    /// # Errors
    ///
    /// Returns an error for a token the session cannot resolve.
    pub fn finish(&mut self, session: &Session) -> Result<SecretValue, GuardError> {
        let rest = Zeroizing::new(std::mem::take(&mut *self.buffer));
        self.release(session, &rest, None)
    }

    fn release(
        &mut self,
        session: &Session,
        text: &str,
        after: Option<char>,
    ) -> Result<SecretValue, GuardError> {
        let rehydrated = session.rehydrate_within(text, self.before, after)?;
        if let Some(last) = text.chars().next_back() {
            self.before = Some(last);
        }
        Ok(rehydrated.text)
    }

    /// The earliest position from which the buffer may still become a token.
    fn hold_from(&self, session: &Session) -> usize {
        let matcher = &session.inner.matcher;
        let floor = self.buffer.len().saturating_sub(matcher.max_len());
        self.buffer
            .char_indices()
            .map(|(i, _)| i)
            .filter(|&i| i >= floor)
            .find(|&i| matcher.could_continue(&self.buffer[i..]))
            .unwrap_or(self.buffer.len())
    }
}

/// Streaming rehydration of one LLM reply.
#[derive(Debug)]
pub struct Rehydrator<'a> {
    session: &'a Session,
    stream: RehydrateStream,
}

impl Rehydrator<'_> {
    /// Adds a chunk of the reply and returns the text that is now final.
    ///
    /// # Errors
    ///
    /// Returns an error for a token the session cannot resolve.
    pub fn feed(&mut self, chunk: &str) -> Result<SecretValue, GuardError> {
        self.stream.feed(self.session, chunk)
    }

    /// Ends the reply and returns what was held back.
    ///
    /// # Errors
    ///
    /// Returns an error for a token the session cannot resolve.
    pub fn finish(mut self) -> Result<SecretValue, GuardError> {
        self.stream.finish(self.session)
    }
}
