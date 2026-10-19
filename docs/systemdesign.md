# ortu: PII Detection, Masking and Encryption for Turkish Speech and LLM Applications

> **Status:** Design document v1.1 (ready for implementation)
> **Language:** Rust core, CLI, Python binding (PyO3), HTTP/gRPC server. All crates use the `ortu-*` prefix.
> **License:** MIT OR Apache-2.0. Copyright (c) Aral Büyükyanbolu.
> **Scope:** A vendor-neutral, open-source PII layer for anyone in Turkey building call-center, voice-agent or LLM applications. Speech is the priority channel; written text is supported by the data model and fully covered in v1.x (see §2).
> **Origin:** A Belzeka tool, built out of a production need at the Belzeka call-center platform. Belzeka's products consume ortu as an external library, through published releases (see §21.1).
>
> **Changes in v1.1:** session-scoped token map, rehydration and tool-argument provenance checks (§10.8); Realtime/PostCall processing modes with no hold-back in realtime (§9); byte-range spans and `Channel` (§5); surrogate rule (§10.9); milestones reordered around a vertical slice (§17); decision log added (§19); adopter-specific content removed from this document.
>
> **Companion documents:** [data-flow.md](data-flow.md) maps every piece of data (origin, form, destination, lifetime). [algorithms/](algorithms/README.md) documents each algorithm in its own file, with data-flow diagrams and worked examples.

---

## 0. How to use this document

This document is the **single source of truth** for the project. Implementers follow these rules:

1. Milestones (§17) are implemented **in order**. A milestone is not started until the acceptance criteria of the previous one are met.
2. Trait names, type names and behaviors defined here are preserved. Any deviation requires an ADR (Architecture Decision Record) under `docs/adr/`.
3. Every commit on `main` passes the quality gate, `cargo xtask check`: rustfmt, Clippy with warnings as errors, tests, the MSRV build, cargo-deny and cargo-audit (ADR-0003).
4. **Real personal data never enters the repository.** All test and evaluation data is synthetic (§14).
5. When something is ambiguous, check the open questions in §20. If the answer is not there, implement the safest option (recall-first, fail-closed) and record it in an ADR.
6. **This document and the repository are public.** No adopter's (including Belzeka's) internal architecture, security findings, customer information or proprietary code goes here. Adopter-specific integration notes live in the adopter's own repository.

### Terminology

| Term | Meaning |
|---|---|
| KVKK | Turkish Personal Data Protection Law No. 6698, the Turkish counterpart of GDPR |
| TCKN | Turkish national identity number (11 digits, checksummed) |
| VKN | Turkish tax identification number (10 digits, checksummed) |
| GİB | Turkish Revenue Administration, publisher of the VKN algorithm |
| STT / TTS | Speech-to-text / text-to-speech |
| Turn | One uninterrupted utterance by one speaker |
| Caller / Agent | The person calling / the human or bot answering |

Turkish words appearing in code blocks and tables (number words, cue phrases, fillers) are **data**, not identifiers. All identifiers, configuration keys, comments and file names are in English.

---

## 1. Problem statement

In call centers, citizens and customers say personal data **out loud**:

```
"TC kimlik numaram on iki üç yüz kırk beş... bir saniye... altmış yedi seksen dokuz on"
  (My national ID is twelve three hundred forty-five... one second... sixty-seven eighty-nine ten)
"Adım Ahmet, soyadım Yılmaz"
  (My name is Ahmet, my surname is Yılmaz)
"IBAN'ım TR otuz üç sıfır sıfır altı bir..."
  (My IBAN is TR thirty-three zero zero six one...)
```

The STT output contains these values verbatim, and the transcript then flows into LLMs, analytics systems, search indexes, logs and quality-assurance teams. Under KVKK this is a risk to the principles of **data minimization** and **purpose limitation**.

LLM-driven voice agents (STT, then LLM, then TTS) make this worse. The transcript is sent in real time to an LLM that is often hosted abroad, and the LLM carries these values into tool-call arguments. Hiding values from the LLM is not enough: the tool still needs the real value, and a value the LLM invents or corrupts can reach another citizen's record.

### Why existing tools fall short

| Problem | Example | Behavior of Presidio and similar tools |
|---|---|---|
| Numbers are spoken as words | "on iki üç yüz kırk beş" | Regex cannot match |
| Values arrive in fragments | "TC'm... bir saniye... 123" | Sentence-level processing misses them |
| STT makes mistakes | 10 digits instead of 11 | Either misses or produces false positives |
| Turkish-specific formats | TCKN, VKN, TR IBAN, license plates | Missing or weak support |
| Turkish casing rules | `İ/i`, `I/ı` | Incorrect case folding |
| Audio recordings contain PII too | WAV files | Text only |
| LLM agents need real values back | Tool calls, read-back | Out of scope for detection-only tools |

### The core of the solution

A deterministic, auditable engine that is **speech-aware**, **Turkish-native** and **streaming-capable**. It masks **both text and audio**, safely **restores real values inside an LLM loop** and offers **reversible encryption**.

---

## 2. Goals and non-goals

### Goals

- **G1. High recall.** A leak is more expensive than a false positive. The default policy is recall-first.
- **G2. Speech awareness.** Spoken numbers, fillers ("ııı", "şey", "bir saniye"), and values split across turns.
- **G3. Turkish-native validators.** TCKN, VKN, TR IBAN, phone, license plate, credit card (Luhn), email, birth date, name, address.
- **G4. Real time.** Streaming API with p99 < 5 ms per utterance (NER disabled). Realtime mode **never holds turns back**; masking adds no waiting time to a voice agent's response (§9).
- **G5. Audio redaction.** Use word timestamps to cover PII regions in recordings with a beep or silence.
- **G6. Reversibility.** An encrypted vault that authorized parties can decrypt with a key. Destroying the key makes the data permanently unreadable (crypto-shredding).
- **G7. Auditability.** Every decision has explainable evidence: which detector fired, which context cue, which score. Raw values are **never** logged.
- **G8. Reproducibility.** Anyone can reproduce the evaluation and benchmarks with a single command.
- **G9. LLM-loop safety.** The LLM only sees tokens. Steps that need real values (tool calls, read-back) go through the session map. A value the LLM invents or corrupts cannot reach an external system (§10.8).
- **G10. Channel independence.** The core data model carries both speech (`Channel::Speech`) and written text (`Channel::Text`). v1 prioritizes speech; full support for written channels such as chat, email and logs lands in v1.x.

### Non-goals

- Building an STT engine. ortu processes the **output** of any STT.
- **Protecting the STT stage.** If STT runs in the cloud, the raw audio has already left before ortu sees any text. ortu protects every step after STT output; keeping audio in-country requires a local or self-hosted STT.
- Guaranteeing "anonymization" in the legal sense. By default ortu performs **pseudonymization** (§3).
- Large ML models in v1. NER is an optional feature defined in §8.4.
- Languages other than Turkish. The architecture is extensible, but v1 targets Turkish only.
- Framework integrations (LiveKit Agents, Pipecat and similar). ortu exposes a framework-neutral API; adopters write the wiring (§21).

### 2.1 Use cases

Speech is the priority (G10); every other use case reuses the same engine through a later milestone. The README mirrors this table.

| Use case | What ortu does | Delivered in | Scope notes |
|---|---|---|---|
| Call-center transcripts (municipalities, hospitals, banks, couriers) | Detects and masks spoken PII in STT output before transcripts are stored or analyzed | v0.1 (M0-M3); remaining detectors M4 | The origin of the project |
| Call recordings | Beeps out the regions where PII was spoken (§11) | M6 | Speech channel, PostCall only |
| Filter in front of an LLM | Tokenizes before text reaches the LLM; rehydrates answers and tool calls under provenance checks (§10.8) | M3 as a library; an OpenAI-compatible gateway on `ortu-server` after M10 | The gateway is a separate product surface (§4.3) |
| Chatbot, webchat, WhatsApp | Same engine on `Channel::Text` | M4 (text defaults), full support v1.x | |
| Emails and support tickets | Written-channel redaction | v1.x | |
| Log scrubbing | Batch redaction of text logs | M8 (`ortu redact --format text`) | No log-pipeline plugins are planned |
| CI checks | Fails a build when PII enters a repository, fixtures or logs | M8 (`ortu scan --fail-on-pii`, exit code 4, ADR-0002) | |
| Training data preparation | Batch redaction before training or fine-tuning | M8 (CLI and Python) | Indirect identifiers (job plus district, rare conditions) are not detected |
| OCR output | Redaction of text from scanned documents and forms | After v1 | Needs an OCR error model (`0/O`, `1/l`) next to the STT noise model (§14.1) |
| Moving data to test environments | Masks free-text fields when production data is copied | M8 for free text | Structured columns with realistic fake values need surrogates, which v1 excludes (D9, §10.9); database connectors are a separate product |

---

## 3. Legal framework and threat model

### 3.1 Anonymization vs. pseudonymization

| Mode | Reversible | Under KVKK | ortu strategy |
|---|---|---|---|
| Redaction | No | Approaches anonymous data (context-dependent) | `Redact` |
| Partial mask | No | Context-dependent | `Mask` |
| Keyed token | No (without the key), linkable | Pseudonymization | `Token` |
| Encryption | Yes (with the key) | Pseudonymization | `Encrypt` |

No project document or announcement claims "100% anonymous" or "ensures KVKK compliance". The accurate wording is: *"A PII detection and pseudonymization engine that supports KVKK data minimization."*

Tokenized data is still personal data from the point of view of the key holder. ortu does not remove obligations for cross-border transfers; it reduces the data and the risk that is transferred.

### 3.2 Threat model

| Actor | Access | Protection |
|---|---|---|
| Downstream LLM / analytics | Masked transcript | Never sees raw PII |
| The LLM's own output | Tool arguments, response text | Invented or corrupted values are rejected by the provenance check (§10.8) |
| Persistent stores (DB, search index) | Persistent transcript | Only PostCall output is written; no raw values (§9.1) |
| Internal staff (unauthorized) | Logs, dashboards | No raw PII in logs; tokens cannot be reversed without the key |
| Internal staff (authorized) | Reveal API + KMS permission | Every access is written to the audit log |
| Database leak | Vault ciphertext + tokens | AES-256-GCM; keys held separately in a KMS |
| Dictionary attack | Tokens | **Unkeyed hashes are forbidden**; HMAC is mandatory (§10.3) |
| Cloud STT provider | Raw audio | **Out of scope** (§2) |

**Critical note:** The TCKN space is about 10^10 values. `SHA256(tckn)` can be brute-forced in seconds. Deterministic tokens therefore use an **HMAC with a tenant-specific secret key**.

### 3.3 Fail-closed principle

If the engine encounters an error (parse error, unreachable key, timeout), it **does not emit the raw text**. One of two behaviors is configured:

- `FailMode::Block` (default): return an error; no text flows.
- `FailMode::RedactAll`: pass the text through with every digit sequence and every name candidate redacted.

---

## 4. Architecture overview

### 4.1 Pipeline

```
 STT output (words + timestamps + confidence scores)
        |
        v
 +--------------------+
 | 1. Ingest          |  Convert to the Transcript model, keep source offsets
 +---------+----------+
           v
 +--------------------+
 | 2. Normalize       |  Turkish case folding, filler marking,
 |                    |  spoken number -> digits (offset-mapped lattice)
 +---------+----------+
           v
 +--------------------+
 | 3. Detect          |  Parallel detectors -> candidate spans
 +---------+----------+
           v
 +--------------------+
 | 4. Validate        |  Checksum/format validation (TCKN, IBAN, VKN, Luhn)
 +---------+----------+
           v
 +--------------------+
 | 5. Score context   |  Cue window ("TC'm", "adım", "IBAN")
 +---------+----------+
           v
 +--------------------+
 | 6. Assemble        |  Cross-turn fragment assembly (session state machine)
 +---------+----------+
           v
 +--------------------+
 | 7. Resolve         |  Resolve overlapping spans by priority and score
 +---------+----------+
           v
 +--------------------+
 | 8. Decide          |  Policy thresholds -> Mask / Review / Pass
 +---------+----------+
           v
 +--------------------+      +----------------------+
 | 9. Transform       |----->| Vault (optional)     |  AES-256-GCM, KMS-wrapped DEK
 +---------+----------+      +----------------------+
           v
 +--------------------+
 | 10. Redact audio   |  Span -> word timestamps -> audio region masking
 +---------+----------+
           v
   Masked transcript + entity list (no raw values) + audit event
```

### 4.2 Processing modes

The pipeline runs in two modes (§9.1). **Realtime** runs on every final turn with no hold-back and produces the text sent to the LLM. **PostCall** runs once over the whole call when it ends and produces the text and audio that go to persistent systems.

### 4.3 Deployment forms

All logic lives in Rust. Deployment forms only expose the same core in different ways:

| Form | When | Rationale |
|---|---|---|
| **Embedded library** (Rust crate, or Python native extension via PyO3) | Realtime voice-agent loop | No network hop per turn; the session map holding raw values never leaves the process |
| **`ortu-server`** (Rust, HTTP + gRPC) | Other languages, post-call batch jobs, audio redaction workers, a future LLM gateway | Language-neutral access, central key and vault management, horizontal scaling |
| **`ortu` CLI** | Batch processing, `scan --fail-on-pii` in CI, debugging | Single binary |

Rule: the Python binding and the server only wrap the core. Detection, normalization, cryptography and policy logic are **never** reimplemented outside Rust (decision D1, §19).

### 4.4 Crate layout (Cargo workspace)

| Crate | Responsibility | Depends on | Created in |
|---|---|---|---|
| `ortu-core` | Data model, traits, pipeline orchestration, policy, conflict resolution | `ortu-normalize` | M0 |
| `ortu-normalize` | Turkish case folding, spoken number parser, fillers, offset map; Turkish word lists in `lexicon.rs` | `unicode-normalization` only (ADR-0004) | M0 |
| `ortu-tr` | Turkish detectors, validators and gazetteers | `ortu-core` | M0 |
| `ortu-crypto` | HMAC tokens, AES-GCM vault, `KeyProvider`, zeroization | `ortu-core` | M0 |
| `ortu-audio` | Timestamp mapping, PCM masking, WAV I/O | `ortu-core` | M0 |
| `ortu-stream` | Session API, Realtime/PostCall modes, cross-turn assembler, session map, rehydration, provenance check | `ortu-core`, `ortu-tr`, `ortu-crypto` | M0 |
| `ortu-cli` | The `ortu` binary | every crate except `ortu-eval` | M0 |
| `ortu-eval` | Synthetic data generator, metrics, baseline comparison | every crate except `ortu-cli` | M0 |
| `ortu-py` | PyO3 binding (maturin); thin, framework-neutral wrapper | every library crate | M3 |
| `ortu-ner` (optional) | ONNX-based NER behind a feature flag | `ortu-core` | v1.1 |
| `ortu-server` | HTTP + gRPC service (`axum` + `tonic`), session management, health and metrics | every library crate | M10 |

Crates are created by the milestone in the last column, together with their first real content. M0 creates the eight crates that form the dependency graph of the engine, so that layering mistakes fail to compile from day one. The workspace also contains `xtask`, the development-only quality gate (ADR-0003); it is not part of the engine and is never published.

**Rule:** `ortu-core` and `ortu-normalize` perform no I/O, hold no global state and consist of pure functions. `unsafe_code` is forbidden workspace-wide through `[workspace.lints]` (§16).

---

## 5. Data model

### 5.1 Input

```rust
pub struct Transcript {
    pub call_id: CallId,          // opaque, not PII
    pub tenant_id: TenantId,
    pub channel: Channel,
    pub turns: Vec<Turn>,
}

pub enum Channel {
    /// STT output: spoken numbers, fillers, word timestamps, audio redaction.
    Speech,
    /// Written text: casing and punctuation are strong cues; no audio redaction.
    Text,
}

pub struct Turn {
    pub index: u32,
    pub speaker: Speaker,         // Caller | Agent | Bot | Unknown
    pub words: Vec<Word>,         // word-level STT output
    pub start_ms: u64,
    pub end_ms: u64,
}

pub struct Word {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: Option<f32>,  // STT confidence, when available
}
```

For STT engines without word timestamps, `Turn::from_text(...)` distributes time evenly across words and marks the turn `TimingQuality::Estimated`; audio padding then increases automatically.

### 5.2 Spans and offsets

All positions are stored in **source coordinates**. Normalization changes the text, so a bidirectional offset map is mandatory.

The canonical coordinate is a **UTF-8 byte range**; word alignment is applied as a per-channel policy. The source text of a Speech turn is its words joined by a single space, and each word's byte range is derived from that string.

```rust
/// Exact position in the source text (UTF-8 byte range, per turn).
pub struct SourceSpan {
    pub turn_start: u32,
    pub byte_start: u32,   // inclusive
    pub turn_end: u32,
    pub byte_end: u32,     // exclusive
}

pub enum SpanSnap {
    /// Speech default: the span is widened to every word it touches.
    Word,
    /// Text default: only the value itself.
    Exact,
}
```

**On the Speech channel**, spans are widened to word boundaries. Reasons: (1) the word is the atomic unit of STT, (2) audio redaction depends on word timestamps, and (3) masking half a word leaks information. If one STT word contains several entities (for example `"12345678901TR"`), the span covers the whole word. Over-masking is better than leaking.

**On the Text channel**, only the value is masked in run-together input such as `TC:12345678901`, producing `TC:[TCKN:7f3a9c2e]`.

Every Speech span can be converted to word indices (`WordRef`); audio redaction (§11) uses those indices.

Why byte ranges rather than word indices: see decision D3 in §19.

### 5.3 Entity

```rust
pub struct Entity {
    pub id: EntityId,               // unique within a call
    pub kind: EntityKind,
    pub span: SourceSpan,           // may span several turns
    pub words: Range<u32>,          // word indices, for audio regions and masking
    pub score: f32,                 // 0.0..=1.0
    pub review: bool,               // between review and mask threshold: masked and flagged
    pub evidence: Evidence,         // explainability
    pub value: SecretValue,         // canonical raw value, zeroized, hidden from Debug
}

#[non_exhaustive]
pub enum EntityKind {
    Tckn, Vkn, Iban, Phone, CreditCard, Email,
    PersonName, PersonSurname, FullName,
    Address, Plate, BirthDate, PassportNo,
    Custom(&'static str),
}

pub struct Evidence {
    pub detector: &'static str,
    pub validator: ValidatorOutcome,                // Passed | Failed(reason) | NotApplicable
    pub cues: Vec<CueHit>,                          // context cues that fired
    pub normalization: Option<NormalizationTrace>,  // spoken-number reading, numeric kinds only
    pub cross_turn: bool,                           // a cue came from the previous assistant turn
    pub score: ScoreBreakdown,                      // every term of the score (section 8.1)
}
```

`EntityKind` also carries its `ortu.toml` name (`credit_card`), its token label (`TCKN`; names, surnames and full names share `NAME`) and its specificity for conflict resolution. Detectors emit `Candidate`s (kind, words, canonical value, validator outcome, lattice prior, optional score floor); the analyzer scores and resolves them into entities (ADR-0006).

### 5.4 `SecretValue` (security type)

```rust
pub struct SecretValue(Zeroizing<String>);

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<redacted>)")
    }
}
// No Display, Serialize or PartialEq impl.
// The raw value is reachable only through `expose_secret()`.
```

This prevents PII from leaking through `println!`, `tracing` or `serde_json` **at compile time**, and rules out timing-dependent comparisons by accident. Rust has no friend visibility, so `expose_secret()` is public; its name makes every call site easy to find, and each one is reviewed (tokenization, encryption, the tool boundary, read-back).

### 5.5 Output

```rust
pub struct RedactionResult {
    pub turns: Vec<RedactedTurn>,          // masked text
    pub entities: Vec<EntityReport>,       // NO RAW VALUES: kind, span, score, token, evidence
    pub audio_regions: Vec<AudioRegion>,   // input to audio redaction
    pub stats: RunStats,
}
```

The JSON schema is versioned under `schemas/redaction_result.v1.json`.

---

## 6. Normalization (`ortu-normalize`)

This is the **most critical and most distinctive** layer of the project.

### 6.1 Turkish case folding

Rust's `to_lowercase()` maps `İ` to `i̇` (i plus a combining dot) and `I` to `i`; both are wrong for Turkish.

```
İ -> i,  I -> ı,  everything else -> Unicode lowercase
```

NFC normalization runs before and, where lowercasing creates a new composition, after folding, so that folding is idempotent. Apostrophe variants (`'`, `’`, `‘`, `´`) collapse to a single form. Suffix splitting separates stem and suffix in forms such as `TC'm`, `TC'mi` and `numaram`, which cue matching relies on (§7.3).

Every Turkish word list used by normalization (number words, fillers, interruptions, correction markers, suffixes, letter names, spoken symbols) lives in one data-only module, `crates/ortu-normalize/src/lexicon.rs`. Entries are written in folded form, and tests enforce it (ADR-0004).

### 6.2 Fillers and interruptions

Dictionary-based marking. Tokens are **marked, not deleted**, so offsets stay intact:

- Fillers: `ııı`, `eee`, `hmm`, `şey`, `yani`, `işte`
- Interruptions: `bir saniye`, `bir dakika`, `dur`, `şöyle`, `bakayım`, `bakalım`
- Correction markers: `yok yok`, `pardon`, `hayır`. Between two digit runs they indicate that the following sequence **overrides** the previous one (§6.4); elsewhere they are ordinary content (`hayır` as a plain "no").

When digit sequences are joined, filler and interruption tokens count as **skippable gaps**, up to `max_gap_tokens` (default 4).

### 6.3 Spoken number parser

#### Lexicon

| Class | Words | Value |
|---|---|---|
| Units | sıfır, bir, iki, üç, dört, beş, altı, yedi, sekiz, dokuz | 0-9 |
| Tens | on, yirmi, otuz, kırk, elli, altmış, yetmiş, seksen, doksan | 10-90 |
| Hundred | yüz | x100 |
| Thousand | bin | x1000 |
| Million / billion | milyon, milyar | x10^6, x10^9 |

STT variants are part of the lexicon: unaccented or garbled spellings such as `uc` (üç), `dort`, `bes`, `alti`, `kirk`, `altmis`, `yetmis`, `doksn`. They are consulted only for words written without any Turkish-specific letter, so `uç` ("tip") and `ön` ("front") are never read as numbers (ADR-0004). Digit tokens (`12`, `5`) and mixed forms (`12'de`) are also supported.

#### Grouping grammar

Turkish numbers compose **from largest to smallest**: `üç yüz kırk beş` = 345. When a word cannot extend the current group, a **new group** starts.

```
group   := [unit? 'yüz']? ten? unit?
         | unit                          (single-digit reading)
extends := magnitude of next word < smallest filled magnitude of current group
                                 (Turkish has no `bir yüz` or `bir bin`: `yüz` and `bin` alone mean 100 and 1000)
```

Examples:

| Input | Groups | Digit string |
|---|---|---|
| `on iki üç yüz kırk beş` | `on iki` / `üç yüz kırk beş` | `12` `345` -> `12345` |
| `bir iki üç dört` | `bir` / `iki` / `üç` / `dört` | `1234` |
| `beş yüz otuz iki yüz yirmi üç kırk beş` | `beş yüz otuz iki` / `yüz yirmi üç` / `kırk beş` | `53212345` |
| `sıfır beş yüz otuz iki` | `sıfır` / `beş yüz otuz iki` | `0532` |
| `yirmi üç` | `yirmi üç` | `23` |
| `iki bin yirmi dört` | `iki bin yirmi dört` | `2024` (in a date context) |

#### Ambiguity and the lattice

Some sequences admit several readings. `yüz yirmi`, for example, reads as `120` as one group, or as `100` followed by `20` (`10020`) as two.

The parser does not return a single result. It produces a **lattice of at most K readings (default 8)**, each with a prior score:

- The natural grouping (every word extends its group whenever the grammar allows) gets prior 1.0. A digit-by-digit reading of single digits is natural too, because a unit can never extend another unit.
- Every forced split (a split where the grammar allowed extension) multiplies the prior by e^-1.
- A word recognized only through the flattened table (`uc` for `üç`) costs 0.7 of a split.

Costs are integers in thousandths and the search is a deterministic beam of width 32 (algorithm 03, ADR-0004).

**Selection is left to the detectors.** The TCKN detector looks for a reading that yields 11 digits with a valid checksum. A reading that passes validation wins even when its prior is low. This is the strongest idea in the system: *the checksum acts as an oracle that resolves STT ambiguity.* See decision D4 in §19.

#### Offset map

Every produced digit points back to its source word:

```rust
pub struct DigitRun {
    pub digits: String,
    pub source: Vec<WordRef>,      // digits[i] -> source word
    pub prior: f32,                // exp(-(cost - best cost)), 1.0 for the best reading
    pub trace: NormalizationTrace, // groups, forced splits, fallback lookups, cost
}
```

### 6.4 Corrections

In input such as `"beş üç iki... yok yok, beş üç üç"` ("five three two... no no, five three three"), the sequence after the correction marker **overrides** the previous one. Both sequences are masked, because the earlier one may also be PII. The corrected sequence is used as the value.

### 6.5 Spelled-out input

Letter-by-letter reading is supported for the `TR` prefix of an IBAN, for email addresses and for license plates:

- `te re`, `t r` -> `TR`
- `et`, `at` -> `@`; `nokta` -> `.`; `alt çizgi` -> `_`; `tire` -> `-`
- Turkish spelling alphabet: `Ankara'nın A'sı` ("A as in Ankara") -> `A` (v1.1)

Many letter names are also ordinary words (`ve`, `de`, `o`), so a spelled run needs at least two letters, or a single letter between two number words (the letter group of a plate such as `34 A 1234`). Downstream detectors validate every run. Letter names and spoken symbols live in `lexicon.rs`; email assembly from spoken symbols arrives with the email detector in M4.

---

## 7. Candidate generation and detection (`ortu-tr`)

### 7.1 Detector trait

```rust
pub trait Detector: Send + Sync {
    fn id(&self) -> &'static str;
    fn kinds(&self) -> &'static [EntityKind];
    fn stems(&self) -> &'static [&'static str] { &[] }   // registered for suffix splitting
    fn detect(&self, ctx: &DetectContext<'_>, out: &mut Vec<Candidate>);
}

pub trait Validator: Send + Sync {
    fn validate(&self, kind: EntityKind, value: &str) -> ValidatorOutcome;
}
```

`DetectContext` provides the source turn, its normalization (canonical words, roles, digit lattices, spelled runs), the cue index and `SessionHints` (the kind the previous assistant turn asked for, and the agent's own names). Detectors are **pure**; state is held only inside `ortu-stream`. Numeric detectors read only **word-aligned** digit windows: a window never starts or ends inside the digits of one spoken word, so `üç yüz` (`300`) is never split (ADR-0006).

### 7.2 Entity specifications

#### TCKN

- 11 digits; the first digit is not 0.
- `d10 = ((d1+d3+d5+d7+d9)*7 - (d2+d4+d6+d8)) mod 10`, using `rem_euclid` for negative intermediate values.
- `d11 = (d1+...+d10) mod 10`
- **Partial-match policy:** if a context cue is present (`TC`, `kimlik`, `T.C.`, `te ce`) and the sequence has 9 to 12 digits, it is masked with `score >= 0.7` even when the checksum fails. Rationale: STT may have dropped a digit; even a wrong value was spoken with the intent of disclosing PII.

#### VKN (tax identification number)

- 10 digits, GİB checksum:
  ```
  for i in 0..9:
      t = (d[i] + 10 - (i + 1)) mod 10
      v = (t * 2^(10 - (i + 1))) mod 9
      if t != 0 and v == 0: v = 9
      sum += v
  check = (10 - sum mod 10) mod 10 == d[9]
  ```
  The implementer must verify this formula against **known valid and invalid test vectors from official sources** and cite the source in an ADR.
- Cues: `vergi`, `vergi no`, `vergi numarası`, `VKN`

#### TR IBAN

- 26 characters: `TR` + 2 check digits + 5-digit bank code + 1 reserved digit (`0`) + 16-digit account number.
- ISO 13616 mod-97 validation, computed as a streaming remainder instead of a big integer.
- When read aloud, `TR` is often omitted: 24 digits plus an `iban` cue are validated with an assumed `TR` prefix.
- The prefix may be spelled (`te re`), written (`TR`) or written with the check digits attached (`TR33`); the body then completes the prefix.
- Partial: `iban` cue plus at least 16 digits is masked without a checksum (score floor 0.7).

#### Phone

- 10-digit national number: mobile `5xx`, landline `2xx/3xx/4xx` with one of the 82 geographic area codes (81 provinces; Istanbul has two), compiled into `ortu-tr` and checked code by code against the national numbering plan ICTA (BTK) notified to the ITU on 2 March 2018. Every `5xx` number counts as mobile: the plan's mobile, MVNO (510, 516, 524) and satellite (592) ranges all start with 5, and the whole block also covers later allocations. Non-personal ranges (800, 811, 812, 888, 898, 900) are not phone PII.
- Prefixes: `+90`, `0090`, `90`, `0`.
- Corporate `444 xx xx` numbers are **not PII**. Configurable (`PhoneDetector::mask_corporate`); not masked by default.
- Values are canonicalized to E.164 (`+905321234567`), so every reading of a number yields the same token.
- Cues: `telefon`, `numaram`, `cep`, `gsm`, `ulaşabilirsiniz`, `arayın`

#### Credit card

- 13 to 19 digits, Luhn, IIN ranges (Visa, Mastercard, Troy `9792`, Amex).
- **Always `Redact`.** Tokens and encryption are not used, so that deployments stay out of PCI-DSS scope. Policy cannot override this (decision D13).
- CVV and expiry context (`son kullanma`, `arkasındaki üç hane`, `güvenlik kodu`) also redacts the following 3 to 4 digits and the date.

#### Email

- Written form via regex; spoken form (`ahmet nokta yılmaz et gmail nokta com`) via regex after normalization.

#### Person name and surname

Three layers:

1. **Cue patterns (high confidence):** `adım X`, `ismim X`, `soyadım X`, `X Bey/Hanım`, `adım soyadım X Y`, and the caller's short answer (at most four words) after an assistant asks for a name. `ben X` arrives with the gazetteer in M4: without one, `ben belediyeden arıyorum` would mask `belediyeden`. A name never extends past punctuation in the source (`ismim Can, borcumu ...`), and titles that take an honorific (`Müdür Bey`) are not names.
2. **Gazetteer (medium confidence):** Turkish first-name list (~10k) and surname list (~50k), matched with Aho-Corasick. The source license must be compatible with open source (ADR).
3. **NER (optional, §8.4)**

Ambiguity rules:

- Names that are also common words (`Umut`, `Deniz`, `Barış`, `Gül`, `Yıldız`, `Kaya`) are **not masked** on a gazetteer hit alone; a cue or, on written channels, capitalization is required. The list lives in `data/ambiguous_names.txt`.
- The **agent's own name** can be exempted by configuration (for example `"Ben belediyenin dijital asistanıyım"`).
- Institution names (`Ahmet Yesevi Mahallesi`) fall under the address detector and are not counted as person names.

#### Address

Rule-based in v1:

- Triggers: `mahallesi/mah.`, `sokak/sk.`, `cadde/cd.`, `bulvar`, `no`, `numara`, `daire`, `kat`, `blok`, `site`, `apartmanı`
- Province and district gazetteer (81 provinces, 973 districts).
- Span expansion: backward from the trigger to the preceding noun phrase, forward to `no/daire` and the following number, at most `N` words (default 15).
- **Granularity policy:** province and district are usually not treated as PII (a municipal call may need them for routing). Default: mask the neighborhood level and below, keep province and district. Configurable.

#### License plate

- Pattern: `(0[1-9]|[1-7][0-9]|8[01])` + 1 to 3 letters + 2 to 4 digits (digit rules depend on the letter count).
- Spoken: `otuz dört a be ce yüz yirmi üç` -> `34 ABC 123`

#### Birth date

- Cue (`doğum tarihim`, `doğdum`, `doğumluyum`) plus a date expression (`on iki mart doksan sekiz`, `12.03.1998`).
- Without a cue, dates are not masked; appointment dates are not PII.

#### Passport

- `U` + 8 digits (Turkish passport); a cue is required.

### 7.3 Context cue system

```rust
pub struct CueRule {
    pub kind: EntityKind,
    pub patterns: Vec<CuePattern>,          // folded words; a trailing `*` makes the last word a prefix
    pub window_before: u16,                 // in words
    pub window_after: u16,
    pub boost: f32,                         // w_cue for this rule
    pub cross_turn: bool,                   // an assistant asking with this cue opens the next turn
}
```

Patterns are matched on the flattened stem of each word, so `IBAN'ım`, `ibanım` and `ıban` all match `iban`, and `numara*` matches `numaram` and `numarası`. Cues add evidence only and never become values, so the diacritic collisions that §6.3 guards against do not apply. The M2 rules (TCKN, IBAN, phone, and name questions) live in `crates/ortu-tr/src/data.rs`, written in folded form and checked by tests.

**Critical pattern: question and answer.** The agent or bot asks `"TC kimlik numaranızı alabilir miyim?"` ("May I have your national ID number?") and the caller says only the number in the **next turn**. The cue in the agent turn is written into session state, and the next caller turn is scored with that context (§9.3).

---

## 8. Scoring, conflict resolution and decisions

### 8.1 Score model

ortu uses a calibratable, explainable logistic score rather than a black box (decision D10):

```
logit = w_detector
      + w_validator * [validator passed]
      - w_invalid   * [validator failed and length exact]
      + sum_i w_cue_i * decay(distance_i)
      + w_prior     * lattice_prior
      + w_stt       * mean(stt_confidence)
      + w_cross     * [assembled across turns]
score = sigmoid(logit)
```

Weights live in `config/weights.toml` and are initially set by hand. In M7 they are calibrated on the evaluation set with a grid search or logistic regression, and the calibration report is written to `docs/calibration.md`.

`decay(d) = exp(-d / tau)`, default tau = 6 words. `w_cue_i` is the boost of the rule that fired. When STT reports no confidence for the span, the STT term is 0. A cue from the previous assistant turn counts as a cue at the edge of its window; `w_cross` is reserved for values assembled across turns (M3). Partial-match floors (TCKN 0.7, IBAN 0.7) are applied after the sigmoid. `config/weights.toml` holds the initial weights, and a test keeps it equal to the compiled-in defaults.

### 8.2 Conflict resolution

For candidates covering the same words:

1. A candidate that passed validation beats one that did not.
2. The more specific kind wins: `Iban > CreditCard > Tckn > Vkn > Phone > Plate > generic digits`.
3. On a tie, the higher score wins.
4. Losing candidates are not discarded; they are recorded in the audit trail with a `suppressed_by` field.
5. **Mask union:** regardless of the winner, the set of masked words is the **union** of all candidates with `score >= review_threshold` (both masked and masked-for-review candidates qualify). This protects recall.
6. Remaining ties are broken by the longer span, then the earlier start, so the result never depends on input order.

Algorithm: sort candidates by `(start, -len)` and process them with a sweep line, O(n log n).

### 8.3 Decision policy

```toml
[policy]
mode = "recall_first"        # recall_first | balanced | precision_first
mask_threshold = 0.50
review_threshold = 0.30      # between 0.30 and 0.50: mask and raise a review flag
fail_mode = "block"

[policy.kinds.credit_card]
transform = "redact"         # cannot be changed (§7.2)

[policy.kinds.address]
keep_levels = ["province", "district"]

[tokens]
scope = "call"               # call | tenant (section 10.2)
```

The full example is `config/ortu.example.toml`; a test keeps it equal to the defaults. Unknown keys and kinds are rejected. A mode sets both thresholds when they are omitted: `recall_first` 0.50/0.30, `balanced` 0.60/0.40, `precision_first` 0.75/0.60. Thresholds must satisfy `0 <= review <= mask <= 1`, and `credit_card` accepts no transform other than `redact`.

### 8.4 NER (optional, v1.1)

- Feature flag: `ner`.
- A small Turkish token-classification model via `ort` (ONNX Runtime): BERTurk-based, distilled and quantized to int8.
- Used only for `PersonName` and `Address`. It contributes a signal to the score model and never decides on its own.
- Its latency budget is measured separately; in streaming mode it runs asynchronously.

---

## 9. Sessions, processing modes and cross-turn assembly (`ortu-stream`)

In a live call the transcript arrives turn by turn. Text must never reach a downstream system (an LLM) **unmasked**. At the same time, a voice agent has to start answering roughly one second after the caller stops speaking, so masking must **not add any waiting time** to that budget.

### 9.1 Two processing modes

| Mode | Runs | Hold-back | Accuracy | Output goes to |
|---|---|---|---|---|
| `Realtime` | On every final turn | **None** | Recall-first, eager | The LLM, live dashboards |
| `PostCall` | Once, over the whole call, when it ends | n/a | Full cross-turn assembly | Persistent transcript, search index, summaries, analytics, audio redaction |

**Rule:** Realtime output is never written to a persistent system. The persistent transcript is always PostCall output.

Why two modes instead of one with hold-back: see decision D7 in §19.

### 9.2 API

```rust
let mut session = engine.session(call_id, tenant_id, Mode::Realtime);
for turn in stt_final_turns {
    let masked = session.push(turn)?;          // returns immediately; no hold-back
    send_to_llm(masked.text);
}
let post = session.finish()?;                  // PostCall: the whole call is reprocessed
persist(post.transcript, post.entities, post.audio_regions);
drop(session);                                 // the session map is zeroized (§10.8)
```

### 9.3 Eager masking in Realtime mode

Realtime mode never holds turns back. It keeps session state and masks each incoming turn **immediately** in that context:

- If a turn ends with an open digit sequence (for example a TCKN cue and 6 digits so far), the sequence is **masked immediately as partial PII** (`[TCKN_INCOMPLETE:...]`). Digits in the next turn are evaluated as a continuation of that sequence.
- If the sequence is completed in a later turn, a new token is issued for the assembled value and placed in the masked text of the completing turn. The LLM uses that token in tool calls.
- If an agent turn asked for PII (`"TC'nizi alabilir miyim"`), digit sequences in the following caller turn are scored as if a cue were present.
- An open sequence can be continued across at most `max_open_turns = 3` turns, and closes when the topic changes.
- **Cost:** the false-positive rate is higher than in PostCall mode. An appointment time spoken right after a TCKN question may be masked too. This is accepted deliberately under the recall-first policy.

### 9.4 State machine (both modes)

```
           +-----------+   PII question (agent)    +-----------+
   ------->|   Idle    |-------------------------->| Expecting |
           +-----+-----+                           +-----+-----+
                 | caller: open digit sequence           | caller: digits start
                 v                                       v
           +-----------+   sequence complete /     +-----------+
           |   Open    |<--------------------------|   Open    |
           |   Run     |   validator passed        |   Run     |
           +-----+-----+                           +-----------+
                 | complete | max_open_turns | topic changed
                 v
           +-----------+
           |  Commit   |---> emit assembled entity, return to Idle
           +-----------+
```

In Realtime mode every turn is emitted immediately, including while in `Open Run` (with a partial mask). In PostCall mode a single assembled entity is produced once the sequence completes.

**Partial STT results:** partial transcripts never go downstream; only final turns are processed. Partials may optionally be used to update session state earlier.

---

## 10. Transforms and cryptography (`ortu-crypto`)

### 10.1 Transform strategies

| Strategy | Example output | Reversible |
|---|---|---|
| `Redact` | `[TCKN]` | No |
| `Mask` | `*******8901` (last N digits; configurable per kind) | No |
| `Token` | `[TCKN:7f3a9c2e]` | No; the same value yields the same token |
| `Encrypt` | `[TCKN:7f3a9c2e]` plus a vault record | Yes, with an authorized key |

`Token` and `Encrypt` share the same token format; the difference is whether ciphertext is stored in the vault. A realistic fake-value (surrogate) strategy does not exist in v1 (§10.9).

### 10.2 Token format

```
[<KIND>:<hex8>]               e.g. [TCKN:7f3a9c2e], [NAME:1b9e44d0]
[<KIND>_INCOMPLETE:<hex8>]    too few digits (e.g. a 9-digit TCKN)
[<KIND>_INVALID:<hex8>]       correct length, failed validation
```

- `hex8` is the first 32 bits of `HMAC-SHA256(K_token[tenant], kind || 0x00 || canonical(value))`.
- `canonical(value)` is the per-kind normalized form: IBAN uppercase without spaces, phone in E.164, names after Turkish case folding.
- Collisions are negligible within a call. Tenant-wide collisions are checked in the vault; on a collision the token is extended to 64 bits.
- **Scope option:** `token_scope = "call" | "tenant"`. With `call`, the key is derived as `HKDF(K_token, call_id)`, so different calls of the same person cannot be linked (unlinkability). `tenant` scope allows linking for analytics. Default: `call` (decision D6).
- The validity marker lets the LLM ask the caller to repeat a value; an LLM cannot count the digits behind a token. The marker reveals only the validation outcome and nothing else about the value.

### 10.3 Why HMAC is mandatory

As noted in §3.2, the TCKN, phone and plate spaces are small; an unkeyed hash is equivalent to plaintext. If the `Token` strategy is configured without a key, the engine **refuses to start** (configuration error).

### 10.4 Vault and encryption

**Envelope encryption:**

```
KEK (in the KMS; never leaves the KMS, so never reaches ortu)
 +-- wraps --> DEK_tenant (versioned, rotated)
                +-- encrypts --> PII values (AES-256-GCM)
```

- Algorithm: AES-256-GCM with a random 96-bit nonce (`OsRng`). The message count per DEK is tracked, and the key is rotated well below 2^32.
- AAD: `tenant_id || call_id || entity_kind || token || key_version`. This prevents a ciphertext from being moved to another context.
- Record format:
  ```
  VaultRecord { token, kind, key_version, nonce, ciphertext, created_at, call_id }
  ```

**Traits (infrastructure-neutral):**

```rust
pub trait KeyProvider: Send + Sync {
    fn token_key(&self, tenant: &TenantId) -> Result<SecretKey>;
    fn data_key(&self, tenant: &TenantId, version: Option<KeyVersion>) -> Result<(SecretKey, KeyVersion)>;
}

pub trait VaultStore: Send + Sync {
    fn put(&self, rec: VaultRecord) -> Result<()>;
    fn get(&self, tenant: &TenantId, token: &Token) -> Result<Option<VaultRecord>>;
    fn shred_call(&self, tenant: &TenantId, call: &CallId) -> Result<u64>;
}
```

Bundled implementations:

- `KeyProvider`: `EnvKeyProvider` (development), `FileKeyProvider` (age-encrypted file). AWS KMS and Vault Transit adapters behind feature flags (v1.1). M2 implements `token_key` with `StaticKeyProvider` (in memory) and `EnvKeyProvider` (`ORTU_TOKEN_KEY_<TENANT>`, then `ORTU_TOKEN_KEY`, 64 hex characters); `data_key` and `FileKeyProvider` arrive with the vault in M5 (ADR-0006).
- `VaultStore`: `InMemoryVault` (tests), `SqliteVault`, `PostgresVault` (feature flag).

### 10.5 Reveal

- `reveal(token, actor, reason)`: every call **must** produce an audit event.
- The reveal API requires a separate permission; the CLI's `ortu reveal` refuses to run without `--reason`.

### 10.6 Crypto-shredding and the right to erasure

- With call-scoped tokens, `shred_call` deletes the vault records.
- Tenant- or period-level erasure: destroying a DEK version makes all ciphertext of that period permanently unreadable.

### 10.7 Memory safety

- All keys and every `SecretValue` use `zeroize::Zeroizing`.
- Token comparisons use `subtle::ConstantTimeEq`.
- `mlock` for keys is optional (feature `mlock`).
- Crates: `aes-gcm`, `hmac`, `sha2`, `hkdf`, `rand_core` from the RustCrypto ecosystem. Custom cryptographic implementations are **forbidden**.

### 10.8 Session map, rehydration and provenance checks

In an LLM-driven system the LLM never sees raw values, but some steps need them: tool calls (for example a TCKN for a debt lookup) and text read back to the user (TTS or chat). For this purpose the session keeps a `token -> SecretValue` map **in memory only**, for the duration of the call.

```rust
impl Session {
    /// Replaces tokens in LLM output with real values, according to the read-back policy.
    pub fn rehydrate(&self, text: &str) -> Result<Rehydrated, GuardError>;

    /// Checks tool arguments and replaces tokens with real values.
    pub fn guard_tool_args(
        &self,
        tool: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, GuardError>;

    /// Buffered rehydration for a streamed LLM response.
    pub fn rehydrator(&self) -> Rehydrator<'_>;

    /// Tokenizes an adopter-supplied value (caller ID, CRM field) before it is placed in a prompt.
    pub fn register(&mut self, kind: EntityKind, value: SecretValue) -> Token;

    /// Masks a tool result before it re-enters the LLM context; detected values join the map.
    pub fn mask_tool_result(
        &mut self,
        tool: &str,
        result: &serde_json::Value,
    ) -> Result<serde_json::Value, GuardError>;
}
```

Every path into the LLM context (`push`, `register`, `mask_tool_result`) writes to the map, and every path out of it toward an external system (`guard_tool_args`, `rehydrate`) reads from it. This is what makes rule 2 below sound: the LLM never had a raw value, so any raw value it produces did not come from the conversation.

Rules:

1. **Tolerant matching.** LLMs corrupt tokens: they drop brackets, change case or insert spaces. `rehydrate` locates the kind plus hex8 core with a tolerant pattern. A token-like string with no entry in the map is never passed through silently; it yields `GuardError` (fail-closed).
2. **Provenance check at the tool boundary.** Tool arguments are run through the detectors. Every PII-looking value in an argument must either be a token present in this session's map, or the argument must contain no PII at all. A raw TCKN, IBAN or phone number that is not in the map is a value the LLM invented or altered, and the call is **rejected** (`GuardError::UnknownValue`). This is what prevents an agent from querying another person's record.
3. **Invalid values.** If an argument contains a token marked `_INCOMPLETE` or `_INVALID`, the call is rejected unless the tool definition explicitly allows it.
4. **Streamed output.** LLM responses arrive in chunks, and a token can be split across two of them. The `Rehydrator` buffers only from a possible token start, and at most one token length; all other text passes through without delay.
5. **Read-back policy.** Rehydration of text going to TTS or chat is configured per kind. By default TCKNs and card numbers are **not read back**; they are spoken in partially masked form (`*******8901`).
6. **Lifecycle.** The map cannot be serialized or logged, and is never written to a shared store such as Redis or disk. It is zeroized on `finish()` or `drop`. Deployments should disable core dumps; the `mlock` feature is recommended to keep it out of swap (§10.7).
7. **With `ortu-server`.** When the session lives in the server, the map lives in server memory. mTLS between client and server is mandatory, and sessions close on a TTL. The embedded library is preferred for the realtime loop (§4.3).

Why this exists: see decision D8 in §19.

### 10.9 Surrogate (realistic fake value) rule

**Not available in v1** (decision D9). If it is ever added:

- A fake value with a valid checksum can coincide with a real person's value. Every surrogate is therefore recorded in the session map and **must** be converted back to the real value at the tool boundary.
- A surrogate that is not in the map, or cannot be converted back, must never reach an external system under any condition. The provenance check (§10.8, rule 2) enforces this.

---

## 11. Audio redaction (`ortu-audio`)

Runs only for `Channel::Speech`, on PostCall output. It needs an STT that provides word-level timestamps; without them it falls back to `TimingQuality::Estimated` (§5.1).

### 11.1 Flow

1. Each masked entity's `SourceSpan` is converted to word timestamps: `[start_ms(word_start), end_ms(word_end - 1)]`.
2. **Padding:** 120 ms before, 180 ms after (the end-of-speech tail is longer). With `TimingQuality::Estimated`, padding is multiplied by 2.5.
3. Regions that overlap or are separated by less than 250 ms are merged, so that a single digit between them cannot leak.
4. Regions are clipped to the bounds of the recording.
5. Masking modes:
   - `Silence`
   - `Tone(1 kHz, -20 dBFS)` (default; an audible beep tells the listener that masking happened)
   - `Noise(pink, -30 dBFS)`
6. A 5 ms **raised-cosine crossfade** is applied at the edges to avoid clicks.
7. In stereo or dual-channel recordings (agent and caller on separate channels), only the **relevant speaker's channel** is masked; in mono recordings, everything is.

### 11.2 Format support

- v1: WAV (PCM16, PCM32f), 8/16/48 kHz, mono and stereo, via `hound`.
- v1.1: MP3/OGG/FLAC input via `symphonia`; output is always WAV or Opus.
- Streaming mode: in-place masking over PCM chunks as the `AudioRegion` list arrives, so it can plug directly into media pipelines.

### 11.3 STT timing drift

STT timestamps can drift by 100 to 300 ms. The padding values are measured and calibrated in M7 using synthetic TTS audio with known alignments. The "PII syllable outside the masked region" rate (audio leak rate) is reported as a metric.

---

## 12. Observability and audit

- Structured logging with `tracing`. **No log field ever contains a raw value.** This is guaranteed by the type system (§5.4) and by a dedicated test in the workspace test suite: the full pipeline runs on synthetic PII, the captured log output is searched for the raw values, and the test **fails** if any is found.
- Audit event (`AuditEvent`): `call_id`, `tenant_id`, `entity_kind`, `token`, `detector`, `score`, `action` (`masked | revealed | shredded | tool_call_rejected | tool_args_released`), `actor`, `ts`. No raw values.
- Audit events are delivered through the `AuditSink` trait, implemented by the adopter (database, log pipeline, SIEM). `reveal` fails if the sink cannot record the event (fail-closed).
- Metrics (the `metrics` crate, Prometheus-compatible):
  - `ortu_entities_total{kind}`
  - `ortu_latency_ms` (histogram)
  - `ortu_open_runs`
  - `ortu_failclosed_total`
  - `ortu_tool_calls_rejected_total{reason}`
  - `ortu_rehydrate_failures_total`

---

## 13. Interfaces

### 13.1 Rust library

```rust
let engine = ortu::Engine::builder()
    .locale(Locale::TrTr)
    .policy(Policy::from_file("ortu.toml")?)
    .detectors(ortu_tr::default_detectors())
    .key_provider(EnvKeyProvider::new())
    .vault(SqliteVault::open("vault.db")?)
    .build()?;

let result = engine.redact(&transcript)?;
```

### 13.2 CLI

```
ortu scan      <transcript.json>                    # detection report, no raw values
ortu redact    <transcript.json> -o out.json [--strategy token|redact|mask|encrypt]
ortu audio     <call.wav> --transcript out.json -o call.redacted.wav
ortu reveal    <token> --call <id> --reason "..."   # audited
ortu shred     --call <id>
ortu eval      [--dataset data/eval/v1] [--baseline presidio]
ortu bench
ortu normalize "on iki üç yüz kırk beş"             # debug: prints the lattice
```

- Input formats: ortu JSON schema, plain text (`--format text`, no timestamps), and adapters for common STT outputs (Whisper JSON, Deepgram, Google STT; `--format whisper` and so on).
- Exit codes (ADR-0002): `0` success, `1` runtime error, `2` usage error (reported by clap), `3` fail-closed, `4` PII found (`scan --fail-on-pii`, for CI).
- Subcommands are added by the milestone that implements them; the binary never advertises a command that does not work yet.

### 13.3 Python (`ortu-py`, maturin)

```python
import ortu

engine = ortu.Engine.from_config("ortu.toml")

# Batch processing
result = engine.redact_words(words=[...], call_id="...")

# Voice agent loop with an LLM
session = engine.session(call_id="...", mode="realtime")
caller = session.register("phone", caller_id)                # token for the system prompt
masked = session.push(turn)                                  # masked.text goes to the LLM
args = session.guard_tool_args("query_debt", llm_args)       # real values; raises GuardError on rejection
result = session.mask_tool_result("query_debt", raw_result)  # tokens before the result reaches the LLM
spoken = session.rehydrate(llm_reply)                        # applies the read-back policy
post = session.finish()                                      # persistent transcript (PostCall)
```

- The binding is a thin wrapper; all logic lives in the Rust core (§4.3).
- The GIL is released during heavy work (`py.allow_threads`).
- `abi3` wheels (Python 3.9+) for manylinux, macOS and Windows; how they are built without hosted CI is decided in M9 (ADR-0003).
- Framework integrations (LiveKit Agents, Pipecat and similar) are out of scope; adopters build them on top of this API (§21).

### 13.4 Server (`ortu-server`, v1.x)

- HTTP (`axum`) and gRPC (`tonic`) with the same operations: `redact`, `scan`, `session/*` (`push`, `guard_tool_args`, `rehydrate`, `finish`), `audio/redact`, `reveal`.
- Authentication: mTLS plus a per-tenant service token; `reveal` requires a separate permission.
- `/healthz`, `/readyz`, and Prometheus `/metrics` (§12).
- Stateless requests scale horizontally; session requests are routed to the same instance (sticky) and close on a TTL.
- Distroless Docker image; core dumps disabled.

---

## 14. Evaluation (`ortu-eval`)

This is the layer that makes the project's claims **verifiable**.

### 14.1 Synthetic data generator

- **Valid identifier generators:** TCKN (with checksum), VKN, TR IBAN (real bank codes, valid mod-97), phone, plate, Luhn-valid cards (test IIN ranges).
- **Names:** sampled from the gazetteers.
- **Conversation templates:** municipal scenarios (water outage, garbage collection, municipal police, debt inquiry), hospital appointments, banking.
- **Reading-style randomization:** digit by digit, grouped (2/3/4), mixed; injected fillers and interruptions; splits across turns; corrections.
- **STT noise model:** digit drops and insertions (p = 0.02 to 0.05), homophone confusions (`altı/aldı`, `on/onu`), lost diacritics, merged and split words.
- **Negative examples (false-positive tests):** appointment dates, prices, quantities, 444 numbers, person names inside street names, names that are also common words.
- **LLM dialogues:** masked conversations with tool calls, used to measure token corruption and provenance-check behavior.
- **Seeded and deterministic:** `ortu-eval gen --seed 42 --n 10000` always produces the same dataset.

### 14.2 Gold data format

JSONL; each line holds a transcript and its expected entities (`kind`, word spans). Versioned under `data/eval/v1/`.

Planned for v1.1: an end-to-end **TTS, then real STT, then ortu** test set. Synthetic audio is produced with Turkish TTS and transcribed with Whisper, so that real STT errors are measured.

### 14.3 Metrics

- Per-kind **precision, recall and F2** (recall-weighted; the primary metric).
- **Leak rate:** unmasked PII words divided by total PII words (word level; the strictest metric).
- **Exact span match** and **partial overlap**, reported separately.
- Audio: PII duration outside masked regions (ms) divided by total PII duration.
- Latency: p50/p95/p99 per utterance; throughput (words/s, MB/s).
- **LLM robustness:** per model, the token corruption rate, rehydration success rate and the number of invented values caught by the provenance check.

### 14.4 Baselines

- Microsoft Presidio (default configuration for Turkish plus a hand-added TCKN regex, for a fair comparison).
- A plain regex baseline.
- The comparison scripts live in `eval/baselines/` and are reproducible with Docker.

### 14.5 Report

`ortu eval` produces `reports/eval-<version>.md` and `.json`: tables, a confusion matrix and the 20 worst errors (safe to show because the data is synthetic). README badges and tables are updated automatically from this report.

---

## 15. Performance targets

| Metric | Target (NER disabled) |
|---|---|
| p99 latency per utterance (40 words or fewer) | < 5 ms |
| Throughput (single core) | > 20 MB/s of plain text |
| Audio redaction | > 200x real time |
| Memory (engine + gazetteers) | < 50 MB |
| `guard_tool_args` / `rehydrate` p99 | < 1 ms |
| Hold-back added in Realtime mode | 0 ms (processing time only) |

Techniques:

- The `regex` crate: linear time with no backtracking, hence immune to ReDoS.
- `aho-corasick` (DFA) for gazetteers and cues, built once at compile time or startup.
- Minimal allocation on the hot path: `SmallVec`, `&str` spans, reused buffers.
- In batch mode, detectors run in parallel per turn with `rayon`; streaming mode runs deterministically on a single thread.
- Benchmarks with `criterion`; benchmark runs compare against the stored baseline and flag regressions above 10%.

---

## 16. Security engineering and quality

- `unsafe_code = "forbid"` in `[workspace.lints]`, inherited by every crate except the `ortu-py` FFI layer (M3), which opts out explicitly.
- Workspace Clippy policy: `all` and `pedantic` as warnings, plus `unwrap_used`, `expect_used`, `todo`, `dbg_macro`, `print_stdout` and `print_stderr` (allowed only in tests through `.clippy.toml`). The quality gate turns every warning into an error.
- `cargo-deny` (`deny.toml`): license allowlist (MIT, Apache-2.0 with or without the LLVM exception, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-3.0, Zlib), OpenSSL banned, duplicate-version warnings, crates.io as the only allowed source.
- `cargo-audit` in every run of the quality gate.
- **Fuzzing (`cargo-fuzz`, in `fuzz/`):** targets `normalize`, `parse_spoken_number`, `iban_validate`, `redact_pipeline` and `rehydrate`. Invariants: no panics, no out-of-bounds offsets, word count preserved from input to output. Coverage-guided fuzzing needs Linux or macOS; on Windows, seeded randomized stress tests with the same generators and invariants run through `cargo xtask fuzz` (ADR-0005).
- **Property-based tests (`proptest`):**
  - Every valid TCKN is detected regardless of reading style.
  - After redaction, no input PII value appears as a substring of the output.
  - The `normalize` offset map always points to a valid source index.
  - `Encrypt` followed by `reveal` round-trips to the original value.
  - Rehydration of randomly corrupted tokens either returns the correct value or `GuardError`; it **never returns a wrong value**.
  - A tool argument containing a valid TCKN, IBAN or phone number that is not in the session map is always rejected.
  - Streaming rehydration of an LLM response split at random points equals rehydration of the unsplit response.
- **Snapshot tests (`insta`):** normalization lattices and redaction output.
- **Leak test:** the log-scanning test from §12.
- **Release security:** SBOM (CycloneDX), signed releases (cosign / GitHub attestations), and a responsible disclosure process in `SECURITY.md`.
- Edition 2024; MSRV: Rust 1.85, checked by the quality gate (ADR-0001). The development toolchain is pinned in `rust-toolchain.toml` and bumped in dedicated commits.

---

## 17. Milestones and acceptance criteria

| # | Milestone | Scope | Acceptance criteria |
|---|---|---|---|
| **M0** | Skeleton | Workspace with the eight M0 crates (§4.4), quality gate `cargo xtask check` (fmt, clippy, test, MSRV, deny, audit), licenses, `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, ADR template | `cargo xtask check` passes; `cargo run -p ortu-cli -- --help` works |
| **M1** | Normalization | Turkish case folding, filler and interruption marking, spoken number parser with lattice and offset map, spelled-out input | Every example in the §6.3 table is covered by a snapshot test; 1M randomized stress iterations without a panic or invariant violation (`cargo xtask fuzz`, ADR-0005); offset proptest passes |
| **M2** | Core detectors and scoring | TCKN, phone, IBAN, person name (cue-based); cue system; baseline §8 (hand-set weights, conflict resolution, policy); `ortu.toml`; `Token` (HMAC, call scope) | Validators pass published reference examples where they exist (SWIFT IBAN registry) and otherwise vectors from an independent implementation of the published algorithm (ADR-0006); at least 20 positive and 20 negative end-to-end cases per kind; conflict-resolution proptests; 1M randomized stress iterations of the analyzer |
| **M3** | Session and LLM loop | Realtime/PostCall modes, cross-turn assembly, session map, `rehydrate`, streaming `Rehydrator`, `guard_tool_args` (provenance check), basic `ortu-py` API | Agent question then caller answer scenario; TCKN split across 3 turns; zero hold-back in Realtime; the rehydration and provenance proptests from §16; end-to-end example from Python |
| **v0.1** | **Vertical slice** | M0 to M3 | Can run in **shadow mode** inside a voice agent: produces metrics only (kind, score, latency; no raw values) without affecting decisions |
| **M4** | Remaining detectors | VKN, card (with CVV and expiry), email, plate, birth date, passport, address (rule-based), name gazetteer; Text channel defaults | M2 test criteria for every new kind; Text channel tests |
| **M5** | Transforms and cryptography | `Redact`/`Mask`/`Encrypt`, vault traits, SQLite and in-memory vaults, reveal with audit, shred | Round-trip proptest; tampering with AAD makes decryption fail; log leak test green |
| **M6** | Audio | Span to time, padding, merging, tone and silence, crossfade, channel selection | Region accuracy within one sample on synthetic WAV; no click at crossfades (spectral test); 200x real time |
| **M7** | Evaluation and benchmarks | Synthetic generator, metrics, LLM robustness measurement, Presidio baseline, calibration, report generation | `cargo xtask eval` produces the report in one command; TCKN/IBAN/phone recall >= 0.98; leak rate reported; benchmark results in the README |
| **M8** | CLI and packaging | All CLI commands, STT format adapters, wheels | The example notebook runs after `pip install ortu`; every CLI command is covered by an integration test |
| **M9** | Release | Documentation site (mdBook), README, examples, SBOM, signed v1.0.0, crates.io and PyPI | The README's "get started in 5 minutes" steps work on a clean machine |
| **M10** | `ortu-server` | HTTP and gRPC service, session management, Docker image (§13.4) | The Python API's integration tests pass against the server; load-test report |

Error patterns observed in shadow mode are fed back into the synthetic generator as patterns only, never as raw values. Recall targets refer to **synthetic data with STT noise**, and the report states this explicitly.

Why this order: see decision D11 in §19.

---

## 18. Repository layout

```
ortu/
├── Cargo.toml                 # workspace: members, shared metadata, dependencies, lints     M0
├── rust-toolchain.toml        # pinned development toolchain                                M0
├── deny.toml                  # cargo-deny policy (§16)                                      M0
├── .clippy.toml               # Clippy MSRV and test exemptions                              M0
├── .rustfmt.toml              # formatting                                                   M0
├── .cargo/config.toml         # `cargo xtask` alias                                          M0
├── .editorconfig, .gitattributes, .gitignore                                                 M0
├── crates/                    # one directory per crate, created as listed in §4.4
│   ├── ortu-core/
│   ├── ortu-normalize/
│   ├── ortu-tr/
│   │   └── data/              # large curated datasets: gazetteers, provinces/districts     M4
│   ├── ortu-crypto/
│   ├── ortu-audio/
│   ├── ortu-stream/
│   ├── ortu-cli/
│   ├── ortu-eval/
│   ├── ortu-py/                                                                              M3
│   └── ortu-server/                                                                          M10
├── xtask/                     # quality gate: `cargo xtask check` (ADR-0003)               M0
├── config/
│   ├── ortu.example.toml                                                                     M2
│   └── weights.toml                                                                          M2
├── schemas/                   # JSON schemas of CLI and server output (versioned)            M8
├── data/eval/v1/              # synthetic gold set (generator output, seeded)                M7
├── eval/baselines/            # Presidio comparison, Dockerfile                              M7
├── fuzz/                      # cargo-fuzz targets, own workspace, nightly (ADR-0005)        M1
├── benches/                                                                                  M7
├── examples/                  # Rust, Python, CLI, voice agent with an LLM                   M3
├── docs/
│   ├── systemdesign.md        # this document                                                M0
│   ├── data-flow.md           # data inventory, trust zones, flows                           M0
│   ├── algorithms/            # one document per algorithm                                   M0
│   ├── adr/                   # architecture decision records                                M0
│   ├── book/                  # mdBook                                                       M9
│   └── calibration.md                                                                        M7
├── reports/                                                                                  M7
├── .github/
│   └── dependabot.yml         # dependency update proposals, verified with the quality gate   M0
├── LICENSE-MIT                                                                               M0
├── LICENSE-APACHE                                                                            M0
├── NOTICE                     # attribution: "A Belzeka tool", copyright                     M0
├── SECURITY.md                                                                               M0
├── CONTRIBUTING.md                                                                           M0
└── README.md                                                                                 M0
```

The right-hand column is the milestone that creates each entry. Nothing is created before its milestone, so the repository never contains empty placeholders. Tool configuration files keep the names the tools look for at the workspace root; `.clippy.toml` and `.rustfmt.toml` use the dotted form both tools support, to keep the root listing short.

---

## 19. Design decision log

Each entry records the context, the options considered, the decision and its consequences. Changes to these decisions require an ADR.

### D1. Rust core with thin bindings

- **Context:** The engine sits on the latency-critical path of a voice agent and handles secrets in memory.
- **Options:** Python (closest to the Presidio ecosystem), Go, Rust.
- **Decision:** Rust core; Python and the server only wrap it.
- **Why:** The p99 budget of 5 ms per utterance leaves no room for GC pauses. The type system lets us make raw values unprintable at compile time (§5.4), and `zeroize` gives us control over secret lifetimes that a garbage-collected runtime does not. The `regex` crate guarantees linear-time matching. One core means the CLI, the Python package and the server can never disagree on a detection.
- **Consequences:** Higher contributor onboarding cost; PyO3 and maturin add build complexity to the build and release process.

### D2. Embedded library for the realtime loop, server for everything else

- **Context:** The most demanding adopter is a Python voice agent.
- **Options:** Sidecar service only; embedded library only; both.
- **Decision:** Both, with the embedded library preferred for realtime (§4.3).
- **Why:** A sidecar adds a network hop to every turn, and the session map holding raw values would have to live in another process. The server still matters for other languages, batch jobs and a future LLM gateway.
- **Consequences:** Two delivery surfaces must stay behaviorally identical; the same integration tests run against both (M10).

### D3. Byte-range spans with word snapping

- **Context:** v1.0 defined spans as word indices, which suits speech but cannot express `TC:12345678901` in written text without masking the cue as well.
- **Options:** Keep word spans and add a second span type for text; switch to byte spans everywhere.
- **Decision:** Byte ranges as the canonical coordinate; word alignment as a per-channel policy (`SpanSnap`).
- **Why:** One coordinate system keeps the resolver, the audit format and the JSON schema single-shaped. Speech behavior is unchanged because snapping restores word-level masking.
- **Consequences:** Every Speech turn needs a deterministic word-to-byte mapping (words joined by a single space).

### D4. Checksum as an oracle over a lattice

- **Context:** Spoken numbers are ambiguous (`yüz yirmi` is `120` or `10020`), and STT adds noise.
- **Options:** Commit to the single most likely parse; return every parse; return a bounded lattice.
- **Decision:** A lattice of at most K = 8 readings; detectors pick the reading that validates.
- **Why:** Validators carry far more information than a grammar prior. A single best parse throws that information away before the component that can use it ever sees it.
- **Consequences:** Detectors see up to K readings per digit run; this cost is bounded by K and measured in the benchmarks.

### D5. Recall-first and fail-closed by default

- **Context:** A leaked TCKN cannot be recalled; a masked appointment time costs a little usability.
- **Decision:** Recall-first thresholds, partial-match masking with cues, mask union across candidates, and `FailMode::Block`.
- **Consequences:** False positives are expected and are measured explicitly (negative examples in §14.1).

### D6. Keyed HMAC tokens with call scope by default

- **Context:** The TCKN space is small enough to brute-force any unkeyed hash.
- **Decision:** HMAC with a tenant key; call-scoped derivation by default; tenant scope as an explicit opt-in.
- **Why:** Call scope prevents cross-call linking of the same person, which is the privacy-preserving default. Analytics that need linking can opt in deliberately.

### D7. No hold-back in realtime; two processing modes

- **Context:** v1.0 held a turn for up to 2 turns or 4000 ms while a digit sequence was open. That was designed with recordings and analytics in mind.
- **Options:** Keep the hold-back with a shorter timeout; eager masking only; eager masking in realtime plus a full pass after the call.
- **Decision:** Eager masking with no hold-back in `Realtime`, and full cross-turn assembly in `PostCall`.
- **Why:** A voice agent has to start answering about one second after the caller stops. Any hold-back is directly audible as a slow bot. Eager masking keeps recall at the cost of some false positives in the live loop, and the PostCall pass restores accuracy for everything that is persisted.
- **Consequences:** Persistent systems must take PostCall output only (§9.1); adopters must call `finish()`.

### D8. Session map, rehydration and provenance checks

- **Context:** v1.0 assumed that "the LLM never sees raw values" was the whole story. In an agent with tools, the tool needs the real value, TTS cannot read a token aloud, and an LLM can invent a plausible TCKN.
- **Decision:** A call-scoped in-memory map, tolerant rehydration, buffered streaming rehydration, and a provenance check at the tool boundary (§10.8).
- **Why:** Without rehydration the engine cannot be used in the very loop it is meant to protect. Without the provenance check, an invented or corrupted value could query a different citizen's record, which is worse than a leak.
- **Consequences:** Raw values live in process memory for the duration of a call, so map lifecycle rules (§10.8, rule 6) become part of the security model.

### D9. No surrogates in v1

- **Context:** Realistic fake values can help LLMs that handle bracketed tokens poorly.
- **Decision:** Not in v1.
- **Why:** A checksum-valid fake value can collide with a real person's value. If a surrogate ever escaped to a tool, the agent would act on a stranger's record. Tokens are sufficient for current LLMs, and the M7 robustness measurement will show whether that remains true.

### D10. Explainable logistic scoring over an ML classifier

- **Decision:** A hand-structured logistic model with calibrated weights.
- **Why:** Every decision must be explainable in an audit (G7), and weights must be reviewable by someone who is not an ML engineer. NER is added later only as one more signal (§8.4).

### D11. Vertical slice before breadth

- **Context:** The v1.0 plan delivered Python bindings in M8, so no real conversation would have been seen for months.
- **Decision:** M0 to M3 form a vertical slice (v0.1) that can run in shadow mode inside a real voice agent.
- **Why:** Synthetic data cannot fully model real STT errors. The fastest way to learn what actually fails is to run the narrowest end-to-end version against real traffic without affecting decisions.

### D12. Vendor-neutral core; framework wiring lives with adopters

- **Decision:** No LiveKit, Pipecat or other framework code in this repository.
- **Why:** Framework APIs change quickly, and adopters typically pin them exactly. A small, stable core API (`push`, `guard_tool_args`, `rehydrate`, `finish`) is easier to keep compatible than a set of framework plugins.

### D13. Credit cards are always redacted

- **Decision:** No token, mask or encryption option for card numbers.
- **Why:** Storing or tokenizing card data pulls a deployment into PCI-DSS scope. Redaction keeps every adopter out of it by default, and no policy setting can accidentally undo that.

---

## 20. Open questions (to settle before implementation)

1. **Project name:** `ortu`. Availability on crates.io and PyPI must be checked.
2. **Gazetteer source:** The licenses of the first-name and surname lists must be compatible with open source. If no suitable source exists, build a synthetic, curated list.
3. **Default token scope:** `call` (recommended, privacy) or `tenant` (analytics)?
4. **Address granularity:** Keep province and district by default? (Recommended for municipal use: yes.)
5. **Adopter-specific rules:** Institution dictionaries and custom detectors are written in the adopter's own code through the `Detector` trait. No adopter-specific code enters the ortu core.
6. **LLM robustness of the token format:** Measure in M3 how often `[TCKN:7f3a9c2e]` is corrupted by different LLMs; change the format through an ADR if needed.
7. **`ortu-server` protocol order:** gRPC or HTTP first? (Recommended: HTTP first, then gRPC.)
8. **Nomadic `850` numbers:** the numbering plan lists them as location-independent numbers. They are mostly held by businesses but can be assigned to individuals; M2 does not treat them as personal. Decide with evidence from shadow mode.

---

## 21. Reference integration: voice agent with an LLM

```
Phone/SIP --> Media server --> STT (streaming)
                  |                 | final turn
                  |                 v
                  |        ortu Session.push()   (Realtime)
                  |                 | masked text
                  |                 v
                  |               LLM ---- tool call ---> guard_tool_args() ---> internal systems (real values)
                  |                 |
                  |                 v response
                  |           rehydrate() (read-back policy) ---> TTS
                  |
                  v  end of call
            Recording (audio) <-- AudioRegions <-- Session.finish() (PostCall) --> persistent transcript,
                                                                                    search, summaries, analytics
```

Principles:

- Raw PII enters ortu at the first step after STT output. The LLM, persistent stores and analytics systems never see raw values.
- The only places that need real values are the tool boundary and read-back, and both go through the session map.
- The persistent transcript is always PostCall output.
- **Limit:** if STT runs in the cloud, raw audio has left before ortu is involved. ortu does not protect that transfer (§2).

Framework-specific wiring (LiveKit Agents, Pipecat and similar) is written by the adopter: `push` when a user turn completes, `guard_tool_args` in a layer that wraps tool calls, `rehydrate` before TTS, and `finish` when the session closes.

### 21.1 Origin and adopters

ortu is a Belzeka tool. It grew out of the need, at the Belzeka call-center platform, to protect Turkish call transcripts before they reach LLMs and persistent systems. Existing tools did not handle Turkish spoken numbers, STT errors or the LLM tool loop, so ortu was designed as a separate, vendor-neutral library and is developed in the open. Belzeka's products consume ortu like any other adopter, through published releases and as an external dependency. No Belzeka-product-specific code enters this repository; attribution is carried by the `NOTICE` file and the copyright line.

The README states "used in production" only after an adopter has completed shadow mode and ortu is actually affecting decisions.

---

## 22. Release checklist

- [ ] The first screen of the README shows a before/after transcript and a beeped audio demo (synthetic)
- [ ] `ortu eval` table: per-kind recall and precision, leak rate, Presidio comparison
- [ ] Benchmark table with hardware details
- [ ] A "reproduce" section: `git clone && cargo xtask eval` yields the same numbers
- [ ] Terminal recording: the lattice via `ortu normalize`, then `ortu redact`
- [ ] Legal note: pseudonymization vs. anonymization; not legal advice
- [ ] crates.io, PyPI, MSRV and license badges
- [ ] Architecture diagram (a visual version of §4.1)
