//! Audio redaction for ortu.
//!
//! Maps masked spans to word timestamps, pads and merges the resulting
//! regions, and covers them with a tone, silence or noise using
//! raised-cosine crossfades (docs/systemdesign.md, section 11; algorithm 16).
//!
//! Runs on `PostCall` output only. Implementation lands in milestone M6.
