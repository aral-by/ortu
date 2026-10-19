//! Python binding for ortu (docs/systemdesign.md, section 13.3).
//!
//! A thin wrapper over `ortu-stream`: no detection, normalization,
//! cryptography or policy logic lives here (decision D1). Values cross the
//! boundary as JSON-compatible Python objects through Python's own `json`
//! module, so turns, tool arguments and results are plain dicts and lists.
//! The GIL is released while the Rust core works.

use std::fs;

use ortu_core::{
    EntityKind, Policy, SecretValue, Speaker, Transcript, Turn, ValidatorOutcome, Weights, Word,
};
use ortu_crypto::{EnvKeyProvider, SecretKey, StaticKeyProvider};
use ortu_stream::{EntityReport, MaskedTurn, PostCallResult, ReadBack, RehydrateStream};
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;
use serde_json::{Value, json};

pyo3::create_exception!(
    ortu,
    OrtuError,
    PyException,
    "The engine or a session could not do what was asked; no text was released."
);
pyo3::create_exception!(
    ortu,
    GuardError,
    OrtuError,
    "A value leaving the LLM was refused. `reason` names why (`unknown_token`, `unknown_value`, ...) and `kind` the kind of personal data, if known."
);

fn value_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

fn ortu_error(error: &ortu_stream::Error) -> PyErr {
    match error {
        ortu_stream::Error::Config(_) | ortu_stream::Error::Key(_) => value_error(error),
        _ => OrtuError::new_err(error.to_string()),
    }
}

fn guard_error(py: Python<'_>, error: &ortu_stream::GuardError) -> PyErr {
    let err = GuardError::new_err(error.to_string());
    let value = err.value(py);
    // Setting attributes on a fresh exception instance cannot fail in
    // practice; if it did, the exception still carries its message.
    let _ = value.setattr("reason", error.reason());
    let _ = value.setattr("kind", error.kind().map(EntityKind::config_name));
    err
}

/// Python object to JSON, through `json.dumps`.
fn to_json(py: Python<'_>, object: &Bound<'_, PyAny>) -> PyResult<Value> {
    let text: String = PyModule::import(py, "json")?
        .call_method1("dumps", (object,))?
        .extract()?;
    serde_json::from_str(&text).map_err(value_error)
}

/// JSON to a Python object, through `json.loads`.
fn from_json(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    let text = serde_json::to_string(value).map_err(value_error)?;
    Ok(PyModule::import(py, "json")?
        .call_method1("loads", (text,))?
        .unbind())
}

fn kind(name: &str) -> PyResult<EntityKind> {
    EntityKind::from_config_name(name).ok_or_else(|| value_error(format!("unknown kind `{name}`")))
}

fn speaker(name: &str) -> PyResult<Speaker> {
    match name {
        "caller" => Ok(Speaker::Caller),
        "agent" => Ok(Speaker::Agent),
        "bot" => Ok(Speaker::Bot),
        "unknown" => Ok(Speaker::Unknown),
        other => Err(value_error(format!(
            "unknown speaker `{other}`; expected caller, agent, bot or unknown"
        ))),
    }
}

fn speaker_name(speaker: Speaker) -> &'static str {
    match speaker {
        Speaker::Caller => "caller",
        Speaker::Agent => "agent",
        Speaker::Bot => "bot",
        Speaker::Unknown => "unknown",
    }
}

fn validator_name(outcome: ValidatorOutcome) -> &'static str {
    match outcome {
        ValidatorOutcome::Passed => "passed",
        ValidatorOutcome::NotApplicable => "not_applicable",
        ValidatorOutcome::Failed(ortu_core::FailureReason::TooShort) => "too_short",
        ValidatorOutcome::Failed(ortu_core::FailureReason::TooLong) => "too_long",
        ValidatorOutcome::Failed(ortu_core::FailureReason::Checksum) => "checksum",
        ValidatorOutcome::Failed(ortu_core::FailureReason::Format) => "format",
    }
}

/// A turn from `{"speaker": ..., "text": ...}` or `{"speaker": ...,
/// "words": [{"text", "start_ms", "end_ms", "confidence"}]}`, with optional
/// `index`, `start_ms` and `end_ms`. `default_index` applies when `index` is
/// absent.
fn turn(value: &Value, default_index: u32) -> PyResult<Turn> {
    let field = |name: &str| value.get(name);
    let millis = |name: &str| field(name).and_then(Value::as_u64).unwrap_or(0);
    let index = match field("index").and_then(Value::as_u64) {
        Some(index) => u32::try_from(index).map_err(value_error)?,
        None => default_index,
    };
    let speaker = speaker(
        field("speaker")
            .and_then(Value::as_str)
            .ok_or_else(|| value_error("a turn needs a `speaker`"))?,
    )?;
    let (start_ms, end_ms) = (millis("start_ms"), millis("end_ms"));
    if let Some(text) = field("text").and_then(Value::as_str) {
        return Ok(Turn::from_text(index, speaker, text, start_ms, end_ms));
    }
    let words = field("words")
        .and_then(Value::as_array)
        .ok_or_else(|| value_error("a turn needs `text` or `words`"))?
        .iter()
        .map(|word| {
            let text = word
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| value_error("a word needs `text`"))?;
            #[allow(clippy::cast_possible_truncation)]
            let confidence = word
                .get("confidence")
                .and_then(Value::as_f64)
                .map(|c| c as f32);
            Ok(Word {
                text: text.to_owned(),
                start_ms: word.get("start_ms").and_then(Value::as_u64).unwrap_or(0),
                end_ms: word.get("end_ms").and_then(Value::as_u64).unwrap_or(0),
                confidence,
            })
        })
        .collect::<PyResult<Vec<Word>>>()?;
    Ok(Turn {
        index,
        speaker,
        words,
        start_ms,
        end_ms,
        timing: ortu_core::TimingQuality::Exact,
    })
}

fn report(entity: &EntityReport) -> Value {
    json!({
        "id": entity.id.0,
        "kind": entity.kind.config_name(),
        "parts": entity.parts.iter().map(|part| json!({
            "turn": part.turn,
            "words": [part.words.start, part.words.end],
        })).collect::<Vec<_>>(),
        "span": {
            "turn_start": entity.span.turn_start,
            "byte_start": entity.span.byte_start,
            "turn_end": entity.span.turn_end,
            "byte_end": entity.span.byte_end,
        },
        "score": entity.score,
        "review": entity.review,
        "validator": validator_name(entity.validator),
        "replacement": entity.replacement,
        "detector": entity.detector,
        "cross_turn": entity.cross_turn,
    })
}

fn masked_turn(turn: &MaskedTurn) -> Value {
    json!({
        "index": turn.index,
        "speaker": speaker_name(turn.speaker),
        "text": turn.text,
        "entities": turn.entities.iter().map(report).collect::<Vec<_>>(),
    })
}

fn post_call(result: &PostCallResult) -> Value {
    json!({
        "turns": result.turns.iter().map(|turn| json!({
            "index": turn.index,
            "speaker": speaker_name(turn.speaker),
            "text": turn.text,
        })).collect::<Vec<_>>(),
        "entities": result.entities.iter().map(report).collect::<Vec<_>>(),
    })
}

/// Detection, policy and keys, shared by every session.
#[pyclass(frozen, module = "ortu")]
struct Engine {
    engine: ortu_stream::Engine,
}

#[pymethods]
impl Engine {
    /// `policy` and `weights` are TOML documents (the contents of `ortu.toml`
    /// and `weights.toml`). `key` is the tenant token key as 64 hex
    /// characters; without it keys come from `ORTU_TOKEN_KEY_<TENANT>` or
    /// `ORTU_TOKEN_KEY`. `read_back` maps kinds to `full` or `partial`;
    /// `accept_invalid` lists `(tool, kind)` pairs.
    #[new]
    #[pyo3(signature = (
        policy = None,
        *,
        key = None,
        weights = None,
        read_back = None,
        accept_invalid = None,
        max_open_turns = ortu_stream::DEFAULT_MAX_OPEN_TURNS,
        agent_names = None,
    ))]
    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn new(
        policy: Option<&str>,
        key: Option<&str>,
        weights: Option<&str>,
        read_back: Option<Vec<(String, String)>>,
        accept_invalid: Option<Vec<(String, String)>>,
        max_open_turns: u8,
        agent_names: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let mut builder = ortu_stream::Engine::builder().max_open_turns(max_open_turns);
        if let Some(policy) = policy {
            builder = builder.policy(Policy::from_toml_str(policy).map_err(value_error)?);
        }
        if let Some(weights) = weights {
            builder = builder.weights(Weights::from_toml_str(weights).map_err(value_error)?);
        }
        builder = match key {
            Some(hex) => builder.key_provider(StaticKeyProvider::single(
                SecretKey::from_hex(hex).map_err(value_error)?,
            )),
            None => builder.key_provider(EnvKeyProvider),
        };
        for (name, mode) in read_back.unwrap_or_default() {
            let mode = match mode.as_str() {
                "full" => ReadBack::Full,
                "partial" => ReadBack::Partial,
                other => return Err(value_error(format!("unknown read-back `{other}`"))),
            };
            builder = builder.read_back(kind(&name)?, mode);
        }
        for (tool, name) in accept_invalid.unwrap_or_default() {
            builder = builder.accept_invalid(tool, kind(&name)?);
        }
        for name in agent_names.unwrap_or_default() {
            builder = builder.agent_name(&name);
        }
        let engine = builder.build().map_err(|error| ortu_error(&error))?;
        Ok(Self { engine })
    }

    /// An engine configured from an `ortu.toml` file.
    #[staticmethod]
    #[pyo3(signature = (path, *, key = None))]
    fn from_config(path: &str, key: Option<&str>) -> PyResult<Self> {
        let policy = fs::read_to_string(path).map_err(value_error)?;
        Self::new(
            Some(&policy),
            key,
            None,
            None,
            None,
            ortu_stream::DEFAULT_MAX_OPEN_TURNS,
            None,
        )
    }

    /// Opens a session for one live call.
    #[pyo3(signature = (call_id, tenant_id = "default"))]
    fn session(&self, call_id: &str, tenant_id: &str) -> PyResult<Session> {
        let session = self
            .engine
            .session(call_id, tenant_id)
            .map_err(|error| ortu_error(&error))?;
        Ok(Session {
            session: Some(session),
            next_index: 0,
        })
    }

    /// Processes a whole call in post-call mode. `turns` is a list of turn
    /// dicts; turns without an `index` are numbered in order.
    #[pyo3(signature = (turns, call_id, tenant_id = "default"))]
    fn redact(
        &self,
        py: Python<'_>,
        turns: &Bound<'_, PyAny>,
        call_id: &str,
        tenant_id: &str,
    ) -> PyResult<Py<PyAny>> {
        let Value::Array(items) = to_json(py, turns)? else {
            return Err(value_error("`turns` must be a list"));
        };
        let turns = items
            .iter()
            .zip(0u32..)
            .map(|(item, index)| turn(item, index))
            .collect::<PyResult<Vec<Turn>>>()?;
        let transcript = Transcript {
            call_id: call_id.to_owned(),
            tenant_id: tenant_id.to_owned(),
            channel: ortu_core::Channel::Speech,
            turns,
        };
        let engine = &self.engine;
        let result = py
            .detach(|| engine.redact(&transcript))
            .map_err(|error| ortu_error(&error))?;
        from_json(py, &post_call(&result))
    }
}

/// One live call. The session map holding raw values lives in Rust memory
/// only and is zeroized when the session finishes or is collected.
#[pyclass(module = "ortu")]
struct Session {
    session: Option<ortu_stream::Session>,
    next_index: u32,
}

impl Session {
    fn open(&self) -> PyResult<&ortu_stream::Session> {
        self.session
            .as_ref()
            .ok_or_else(|| OrtuError::new_err("the session is finished"))
    }

    fn open_mut(&mut self) -> PyResult<&mut ortu_stream::Session> {
        self.session
            .as_mut()
            .ok_or_else(|| OrtuError::new_err("the session is finished"))
    }
}

#[pymethods]
impl Session {
    /// Masks a final STT turn and returns it at once (no hold-back): a dict
    /// with `index`, `speaker`, `text` (send this to the LLM) and `entities`.
    fn push(&mut self, py: Python<'_>, turn: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let value = to_json(py, turn)?;
        let turn = self::turn(&value, self.next_index)?;
        self.next_index = turn.index.saturating_add(1);
        let session = self.open_mut()?;
        let masked = py
            .detach(|| session.push(turn))
            .map_err(|error| ortu_error(&error))?;
        from_json(py, &masked_turn(&masked))
    }

    /// Tokenizes a value the application already holds (caller ID, CRM
    /// field) before it goes into a prompt.
    fn register(&mut self, kind: &str, value: &str) -> PyResult<String> {
        let kind = self::kind(kind)?;
        let session = self.open_mut()?;
        Ok(session.register(kind, &SecretValue::new(value)).to_string())
    }

    /// Checks the arguments of a tool call and returns them with real values.
    /// Raises `GuardError` when a value did not come from this session.
    fn guard_tool_args(
        &self,
        py: Python<'_>,
        tool: &str,
        args: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let args = to_json(py, args)?;
        let session = self.open()?;
        let released = py
            .detach(|| session.guard_tool_args(tool, &args))
            .map_err(|error| guard_error(py, &error))?;
        from_json(py, &released)
    }

    /// Masks a tool result before it re-enters the LLM context.
    fn mask_tool_result(
        &mut self,
        py: Python<'_>,
        tool: &str,
        result: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let result = to_json(py, result)?;
        let session = self.open_mut()?;
        let masked = py.detach(|| session.mask_tool_result(tool, &result));
        from_json(py, &masked)
    }

    /// Replaces the tokens in an LLM reply according to the read-back policy.
    fn rehydrate(&self, py: Python<'_>, text: &str) -> PyResult<String> {
        let rehydrated = self
            .open()?
            .rehydrate(text)
            .map_err(|error| guard_error(py, &error))?;
        Ok(rehydrated.text.expose_secret().to_owned())
    }

    /// A streaming rehydrator for a reply that arrives in chunks.
    fn rehydrator(slf: Bound<'_, Self>) -> Rehydrator {
        Rehydrator {
            session: slf.unbind(),
            stream: RehydrateStream::default(),
        }
    }

    /// Ends the call: the persistent transcript in post-call mode, as a dict
    /// with `turns` and `entities`. The session map is zeroized.
    fn finish(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let session = self
            .session
            .take()
            .ok_or_else(|| OrtuError::new_err("the session is finished"))?;
        let result = py.detach(move || session.finish());
        from_json(py, &post_call(&result))
    }
}

/// Streaming rehydration of one LLM reply.
#[pyclass(module = "ortu")]
struct Rehydrator {
    session: Py<Session>,
    stream: RehydrateStream,
}

#[pymethods]
impl Rehydrator {
    /// Adds a chunk and returns the text that is final now.
    fn feed(&mut self, py: Python<'_>, chunk: &str) -> PyResult<String> {
        let session = self.session.borrow(py);
        let out = self
            .stream
            .feed(session.open()?, chunk)
            .map_err(|error| guard_error(py, &error))?;
        Ok(out.expose_secret().to_owned())
    }

    /// Ends the reply and returns what was held back.
    fn finish(&mut self, py: Python<'_>) -> PyResult<String> {
        let session = self.session.borrow(py);
        let out = self
            .stream
            .finish(session.open()?)
            .map_err(|error| guard_error(py, &error))?;
        Ok(out.expose_secret().to_owned())
    }
}

/// The `ortu` Python module.
#[pymodule]
fn ortu(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Engine>()?;
    m.add_class::<Session>()?;
    m.add_class::<Rehydrator>()?;
    m.add("OrtuError", m.py().get_type::<OrtuError>())?;
    m.add("GuardError", m.py().get_type::<GuardError>())?;
    Ok(())
}
