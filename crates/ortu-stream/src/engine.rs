//! The engine: configuration shared by every session of a deployment
//! (docs/systemdesign.md, section 13.1).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::Arc;

use ortu_core::{
    Analyzer, AuditSink, ConfigError, CueRule, Detector, DiscardAuditSink, EntityKind, Policy,
    SessionHints, Transcript, Transform, Weights,
};
use ortu_crypto::{KeyProvider, Tokenizer};
use ortu_normalize::turkish_fold;

use crate::error::Error;
use crate::mask::PostCallResult;
use crate::matcher::TokenMatcher;
use crate::postcall;
use crate::session::Session;

/// How a value is spoken or shown when the LLM's reply is read back
/// (section 10.8, rule 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadBack {
    /// The real value (`Ahmet Yılmaz`).
    Full,
    /// Only the last four characters (`*******0146`).
    Partial,
}

impl ReadBack {
    /// The default for a kind: names and addresses are read back in full,
    /// because the agent addresses the caller with them; identifiers, card
    /// numbers, phone numbers and everything else only partially.
    #[must_use]
    pub fn default_for(kind: EntityKind) -> Self {
        match kind {
            EntityKind::PersonName
            | EntityKind::PersonSurname
            | EntityKind::FullName
            | EntityKind::Address => Self::Full,
            _ => Self::Partial,
        }
    }
}

/// Default for `max_open_turns` (section 9.3).
pub const DEFAULT_MAX_OPEN_TURNS: u8 = 3;

/// Detection, policy, keys and the boundaries of the LLM loop. Cheap to clone;
/// every session shares it.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl fmt::Debug for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Engine")
            .field("policy", self.inner.analyzer.policy())
            .field("max_open_turns", &self.inner.max_open_turns)
            .finish_non_exhaustive()
    }
}

pub(crate) struct Inner {
    pub(crate) analyzer: Analyzer,
    keys: Box<dyn KeyProvider>,
    pub(crate) audit: Box<dyn AuditSink>,
    pub(crate) matcher: TokenMatcher,
    read_back: BTreeMap<EntityKind, ReadBack>,
    accepts_invalid: BTreeSet<(String, EntityKind)>,
    pub(crate) max_open_turns: u8,
    agent_names: Vec<String>,
}

impl Inner {
    pub(crate) fn policy(&self) -> &Policy {
        self.analyzer.policy()
    }

    pub(crate) fn read_back(&self, kind: EntityKind) -> ReadBack {
        self.read_back
            .get(&kind)
            .copied()
            .unwrap_or_else(|| ReadBack::default_for(kind))
    }

    pub(crate) fn accepts_invalid(&self, tool: &str, kind: EntityKind) -> bool {
        self.accepts_invalid.contains(&(tool.to_owned(), kind))
    }

    pub(crate) fn hints(&self, expecting: Option<EntityKind>) -> SessionHints {
        SessionHints {
            expecting,
            agent_names: self.agent_names.clone(),
        }
    }

    pub(crate) fn tokenizer(&self, tenant_id: &str, call_id: &str) -> Result<Tokenizer, Error> {
        Ok(Tokenizer::new(
            self.keys.as_ref(),
            tenant_id,
            self.policy().token_scope,
            call_id,
        )?)
    }
}

impl Engine {
    /// Starts configuring an engine.
    #[must_use]
    pub fn builder() -> EngineBuilder {
        EngineBuilder::default()
    }

    /// The decision policy in use.
    #[must_use]
    pub fn policy(&self) -> &Policy {
        self.inner.policy()
    }

    /// Opens a session for one live call: `push` turns as STT finalizes them,
    /// send the masked text to the LLM, guard its tool calls, rehydrate its
    /// replies, and `finish` for the persistent transcript.
    ///
    /// # Errors
    ///
    /// Returns an error if the key provider has no usable key for the tenant.
    pub fn session(
        &self,
        call_id: impl Into<String>,
        tenant_id: impl Into<String>,
    ) -> Result<Session, Error> {
        Session::new(Arc::clone(&self.inner), call_id.into(), tenant_id.into())
    }

    /// Processes a whole call at once in `PostCall` mode (section 9.1): the
    /// output for transcripts, search indexes and audio redaction.
    ///
    /// # Errors
    ///
    /// Returns an error if the key provider has no usable key for the tenant.
    pub fn redact(&self, transcript: &Transcript) -> Result<PostCallResult, Error> {
        let tokenizer = self
            .inner
            .tokenizer(&transcript.tenant_id, &transcript.call_id)?;
        Ok(postcall::run(&self.inner, &tokenizer, &transcript.turns))
    }
}

/// Configures an [`Engine`]. Detectors, cue rules, weights and policy default
/// to the Turkish set and the shipped defaults; a key provider is required.
pub struct EngineBuilder {
    policy: Policy,
    weights: Weights,
    detectors: Option<Vec<Box<dyn Detector>>>,
    cue_rules: Option<Vec<CueRule>>,
    keys: Option<Box<dyn KeyProvider>>,
    audit: Box<dyn AuditSink>,
    read_back: BTreeMap<EntityKind, ReadBack>,
    accepts_invalid: BTreeSet<(String, EntityKind)>,
    max_open_turns: u8,
    agent_names: Vec<String>,
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self {
            policy: Policy::default(),
            weights: Weights::default(),
            detectors: None,
            cue_rules: None,
            keys: None,
            audit: Box::new(DiscardAuditSink),
            read_back: BTreeMap::new(),
            accepts_invalid: BTreeSet::new(),
            max_open_turns: DEFAULT_MAX_OPEN_TURNS,
            agent_names: Vec::new(),
        }
    }
}

impl fmt::Debug for EngineBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EngineBuilder")
            .field("policy", &self.policy)
            .field("has_key_provider", &self.keys.is_some())
            .finish_non_exhaustive()
    }
}

impl EngineBuilder {
    /// The decision policy (`ortu.toml`).
    #[must_use]
    pub fn policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    /// Score weights (`config/weights.toml`).
    #[must_use]
    pub fn weights(mut self, weights: Weights) -> Self {
        self.weights = weights;
        self
    }

    /// Replaces the Turkish detectors.
    #[must_use]
    pub fn detectors(mut self, detectors: Vec<Box<dyn Detector>>) -> Self {
        self.detectors = Some(detectors);
        self
    }

    /// Replaces the Turkish cue rules.
    #[must_use]
    pub fn cue_rules(mut self, rules: Vec<CueRule>) -> Self {
        self.cue_rules = Some(rules);
        self
    }

    /// The source of token keys. Required: unkeyed tokens over small value
    /// spaces are equivalent to plaintext (section 10.3).
    #[must_use]
    pub fn key_provider(mut self, keys: impl KeyProvider + 'static) -> Self {
        self.keys = Some(Box::new(keys));
        self
    }

    /// Where guard decisions are recorded. Defaults to discarding them.
    #[must_use]
    pub fn audit_sink(mut self, sink: impl AuditSink + 'static) -> Self {
        self.audit = Box::new(sink);
        self
    }

    /// Overrides how a kind is read back.
    #[must_use]
    pub fn read_back(mut self, kind: EntityKind, read_back: ReadBack) -> Self {
        self.read_back.insert(kind, read_back);
        self
    }

    /// Lets `tool` receive `_INCOMPLETE` and `_INVALID` values of `kind`, for
    /// tools that exist to check them (section 10.8, rule 3).
    #[must_use]
    pub fn accept_invalid(mut self, tool: impl Into<String>, kind: EntityKind) -> Self {
        self.accepts_invalid.insert((tool.into(), kind));
        self
    }

    /// How many caller turns a value may be spread over (default 3).
    #[must_use]
    pub fn max_open_turns(mut self, turns: u8) -> Self {
        self.max_open_turns = turns;
        self
    }

    /// A name of the agent or bot, never masked as a person name.
    #[must_use]
    pub fn agent_name(mut self, name: &str) -> Self {
        self.agent_names.push(turkish_fold(name));
        self
    }

    /// Builds the engine.
    ///
    /// # Errors
    ///
    /// Returns an error if no key provider is set, if a kind uses the
    /// `encrypt` transform (the vault arrives in M5), or if `max_open_turns`
    /// is zero.
    pub fn build(self) -> Result<Engine, Error> {
        let keys = self.keys.ok_or_else(|| {
            ConfigError::new("a key provider is required: tokens are keyed (section 10.3)")
        })?;
        let encrypting = EntityKind::BUILT_IN
            .into_iter()
            .find(|kind| self.policy.transform_for(*kind) == Transform::Encrypt);
        if let Some(kind) = encrypting {
            return Err(ConfigError::new(format!(
                "`{}` uses transform = \"encrypt\", which needs the vault (M5)",
                kind.config_name()
            ))
            .into());
        }
        if self.max_open_turns == 0 {
            return Err(ConfigError::new("max_open_turns must be at least 1").into());
        }
        let detectors = self.detectors.unwrap_or_else(ortu_tr::default_detectors);
        let labels: BTreeSet<String> = EntityKind::BUILT_IN
            .into_iter()
            .chain(detectors.iter().flat_map(|d| d.kinds().iter().copied()))
            .map(EntityKind::label)
            .collect();
        let matcher = TokenMatcher::new(labels)?;
        let analyzer = Analyzer::new(
            detectors,
            self.cue_rules.unwrap_or_else(ortu_tr::default_cue_rules),
            self.weights,
            self.policy,
        );
        Ok(Engine {
            inner: Arc::new(Inner {
                analyzer,
                keys,
                audit: self.audit,
                matcher,
                read_back: self.read_back,
                accepts_invalid: self.accepts_invalid,
                max_open_turns: self.max_open_turns,
                agent_names: self.agent_names,
            }),
        })
    }
}
