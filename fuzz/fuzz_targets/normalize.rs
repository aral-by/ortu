//! Arbitrary bytes, split into words: the whole normalization step must never
//! panic and must keep every offset inside the input.

#![no_main]

mod invariants;

use libfuzzer_sys::fuzz_target;
use ortu_normalize::{NormalizeConfig, normalize};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let words: Vec<&str> = text.split_whitespace().collect();
    let turn = normalize(&words, &NormalizeConfig::default());
    invariants::check(&words, &turn);
});
