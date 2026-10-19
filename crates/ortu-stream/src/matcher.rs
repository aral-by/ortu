//! Tolerant token matching
//! ([algorithm 15](https://github.com/aral-by/ortu/blob/main/docs/algorithms/15-rehydration-and-provenance.md)).
//!
//! LLMs corrupt tokens: they drop brackets, change case and insert spaces. The
//! matcher accepts those forms but requires a known label and the complete
//! digest. The grammar is ASCII-only and bounded, so that the streaming
//! rehydrator can tell exactly which suffix of its buffer may still become a
//! token:
//!
//! ```text
//! ( "[" WS{0,3} )? LABEL ( "_INCOMPLETE" | "_INVALID" )? WS{0,3} ( ":" | "：" )?
//! WS{0,3} ( HEX{16} | HEX{8} ) ( WS{0,3} "]" )?
//! ```
//!
//! Labels match case-insensitively; `WS` is a space or a tab. A match without
//! its opening bracket must not follow a letter, digit or underscore
//! (`USERNAME:...` is not a `NAME` token), and a match without its closing
//! bracket must not be followed by one.

use std::ops::Range;

use ortu_core::ConfigError;
use regex::Regex;

const MARKERS: [&str; 2] = ["_INCOMPLETE", "_INVALID"];
const MAX_WS: usize = 3;

/// A token found in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenMatch {
    /// Byte range in the searched text.
    pub(crate) range: Range<usize>,
    /// The label in upper case (`TCKN`).
    pub(crate) label: String,
    /// The digest in lower case.
    pub(crate) digest: String,
}

/// Finds tokens with the labels of the engine's kinds.
#[derive(Debug, Clone)]
pub(crate) struct TokenMatcher {
    regex: Regex,
    labels: Vec<String>,
    max_len: usize,
}

impl TokenMatcher {
    /// A matcher for the given labels (`TCKN`, `NAME`). Labels that are not
    /// ASCII letters and digits are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if no label is usable.
    pub(crate) fn new(labels: impl IntoIterator<Item = String>) -> Result<Self, ConfigError> {
        let mut labels: Vec<String> = labels
            .into_iter()
            .map(|label| label.to_ascii_uppercase())
            .filter(|label| !label.is_empty() && label.bytes().all(|b| b.is_ascii_alphanumeric()))
            .collect();
        labels.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        labels.dedup();
        if labels.is_empty() {
            return Err(ConfigError::new("no token label to match"));
        }
        let alternation = labels
            .iter()
            .map(|label| case_insensitive(label))
            .collect::<Vec<_>>()
            .join("|");
        let markers = MARKERS
            .iter()
            .map(|marker| case_insensitive(marker))
            .collect::<Vec<_>>()
            .join("|");
        let pattern = format!(
            r"(?:\[[ \t]{{0,{MAX_WS}}})?({alternation})(?:{markers})?[ \t]{{0,{MAX_WS}}}[:：]?[ \t]{{0,{MAX_WS}}}([0-9a-fA-F]{{16}}|[0-9a-fA-F]{{8}})(?:[ \t]{{0,{MAX_WS}}}\])?"
        );
        let regex = Regex::new(&pattern)
            .map_err(|error| ConfigError::new(format!("token pattern: {error}")))?;
        let longest_label = labels.iter().map(String::len).max().unwrap_or(0);
        // "[" + WS + label + marker + WS + "：" (3 bytes) + WS + 16 hex + WS + "]"
        let max_len = 1
            + MAX_WS
            + longest_label
            + "_INCOMPLETE".len()
            + MAX_WS
            + 3
            + MAX_WS
            + 16
            + MAX_WS
            + 1;
        Ok(Self {
            regex,
            labels,
            max_len,
        })
    }

    /// The longest possible match, in bytes.
    pub(crate) fn max_len(&self) -> usize {
        self.max_len
    }

    /// Every token in `text`, left to right. In a stream, `before` and
    /// `after` are the characters around `text`, for the boundary rules.
    pub(crate) fn find_all(
        &self,
        text: &str,
        before: Option<char>,
        after: Option<char>,
    ) -> Vec<TokenMatch> {
        let mut found = Vec::new();
        let mut at = 0;
        while let Some(captures) = self.regex.captures_at(text, at) {
            let (Some(whole), Some(label), Some(digest)) =
                (captures.get(0), captures.get(1), captures.get(2))
            else {
                break;
            };
            let previous = match whole.start() {
                0 => before,
                start => text[..start].chars().next_back(),
            };
            let next = text[whole.end()..].chars().next().or(after);
            let left_ok = whole.as_str().starts_with('[') || !previous.is_some_and(is_word_char);
            let right_ok = whole.as_str().ends_with(']') || !next.is_some_and(is_word_char);
            if left_ok && right_ok {
                found.push(TokenMatch {
                    range: whole.range(),
                    label: label.as_str().to_ascii_uppercase(),
                    digest: digest.as_str().to_ascii_lowercase(),
                });
                at = whole.end();
            } else {
                // Retry one character later: another token may start inside.
                at = whole.start()
                    + text[whole.start()..]
                        .chars()
                        .next()
                        .map_or(1, char::len_utf8);
            }
        }
        found
    }

    /// Whether `text` could still grow into a token, or into a longer one, if
    /// more text followed. A token that ends with its closing bracket cannot.
    pub(crate) fn could_continue(&self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        if let Some(rest) = text.strip_prefix('[') {
            if spaces(rest).any(|r| self.after_open(r)) {
                return true;
            }
        }
        self.after_open(text)
    }

    fn after_open(&self, s: &str) -> bool {
        if s.is_empty() {
            return true;
        }
        self.labels
            .iter()
            .any(|label| match strip_prefix_ci(s, label) {
                Some(rest) => after_label(rest),
                None => s.len() < label.len() && is_prefix_ci(s, label),
            })
    }
}

fn after_label(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let marked = MARKERS
        .iter()
        .any(|marker| match strip_prefix_ci(s, marker) {
            Some(rest) => after_marker(rest),
            None => s.len() < marker.len() && is_prefix_ci(s, marker),
        });
    marked || after_marker(s)
}

/// `WS{0,3} (":" | "：")? WS{0,3}` and then the digest.
fn after_marker(s: &str) -> bool {
    spaces(s).any(|r| {
        if r.is_empty() {
            return true;
        }
        let colon = r.strip_prefix(':').or_else(|| r.strip_prefix('：'));
        std::iter::once(r)
            .chain(colon)
            .any(|r| spaces(r).any(digest))
    })
}

fn digest(s: &str) -> bool {
    let hex = s.bytes().take(17).take_while(u8::is_ascii_hexdigit).count();
    if hex == s.len() && hex < 16 {
        return true;
    }
    [16, 8].into_iter().any(|n| hex >= n && closing(&s[n..]))
}

/// `(WS{0,3} "]")?` after the digest: only whitespace so far can still be
/// followed by the bracket.
fn closing(s: &str) -> bool {
    let ws = s.bytes().take_while(|b| matches!(b, b' ' | b'\t')).count();
    ws == s.len() && ws <= MAX_WS
}

/// `s` with 0 to 3 leading spaces or tabs removed, as far as present.
fn spaces(s: &str) -> impl Iterator<Item = &str> {
    let ws = s
        .bytes()
        .take(MAX_WS)
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count();
    (0..=ws).map(move |n| &s[n..])
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &s[prefix.len()..])
}

fn is_prefix_ci(s: &str, of: &str) -> bool {
    of.get(..s.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(s))
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `[Tt][Cc][Kk][Nn]`: case-insensitive for ASCII only. Unicode case folding
/// would let `K` (Kelvin sign) match `K`.
fn case_insensitive(literal: &str) -> String {
    literal
        .chars()
        .map(|c| {
            if c.is_ascii_alphabetic() {
                format!("[{}{}]", c.to_ascii_uppercase(), c.to_ascii_lowercase())
            } else {
                regex::escape(&c.to_string())
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::TokenMatcher;

    fn matcher() -> TokenMatcher {
        TokenMatcher::new(["TCKN", "NAME", "PHONE", "IBAN", "ADDRESS"].map(String::from)).unwrap()
    }

    fn found(text: &str) -> Vec<(String, String, &str)> {
        matcher()
            .find_all(text, None, None)
            .into_iter()
            .map(|m| (m.label, m.digest, &text[m.range]))
            .collect()
    }

    #[test]
    fn corrupted_forms_are_accepted() {
        for text in [
            "[TCKN:df1c492e]",
            "[tckn:DF1C492E]",
            "TCKN:df1c492e",
            "[ TCKN : df1c492e ]",
            "TCKN df1c492e",
            "[TCKN：df1c492e]",
            "[TCKN_INCOMPLETE:df1c492e]",
            "\"TCKN_invalid:df1c492e\"",
        ] {
            let matches = found(text);
            assert_eq!(matches.len(), 1, "{text}");
            assert_eq!(matches[0].0, "TCKN");
            assert_eq!(matches[0].1, "df1c492e");
        }
    }

    #[test]
    fn surrounding_text_is_kept_out_of_the_match() {
        let text = "numaranız TCKN:df1c492e, teşekkürler";
        assert_eq!(found(text)[0].2, "TCKN:df1c492e");
        let text = "isim [NAME:55832c68].";
        assert_eq!(found(text)[0].2, "[NAME:55832c68]");
    }

    #[test]
    fn partial_digests_and_embedded_labels_are_not_tokens() {
        for text in [
            "[TCKN:df1c492]",
            "USERNAME:55832c68",
            "TCKN:df1c492e5",
            "TCKN:df1c492e12345",
            "[UNKNOWN:df1c492e]",
        ] {
            assert_eq!(found(text), [], "{text}");
        }
    }

    #[test]
    fn an_empty_label_set_is_rejected() {
        assert!(TokenMatcher::new(["", "B-AD"].map(String::from)).is_err());
    }

    #[test]
    fn sixteen_digit_digests_are_accepted() {
        assert_eq!(found("[IBAN:0123456789abcdef]")[0].1, "0123456789abcdef");
    }

    #[test]
    fn boundaries_use_the_surrounding_stream_characters() {
        let m = matcher();
        assert_eq!(m.find_all("NAME:55832c68", Some('R'), None), []);
        assert_eq!(m.find_all("NAME:55832c68", Some(' '), None).len(), 1);
        assert_eq!(m.find_all("NAME:55832c68", None, Some('x')), []);
    }

    #[test]
    fn prefixes_that_may_still_become_tokens() {
        let m = matcher();
        for text in [
            "[",
            "[ ",
            "T",
            "tc",
            "[TCK",
            "TCKN",
            "TCKN_INC",
            "TCKN:",
            "TCKN: df1c",
            "TCKN:df1c492e",
            "TCKN:df1c492e ",
            "[IBAN:0123456789abcdef",
            "A",
        ] {
            assert!(m.could_continue(text), "{text:?}");
        }
        for text in [
            "",
            "x",
            "[TCKN:df1c492e]",
            "TCKN:df1c492e.",
            "TCKNX",
            "TCKN:df1c492e5678912345",
            "TCKN:df1c492e  x",
        ] {
            assert!(!m.could_continue(text), "{text:?}");
        }
    }
}
