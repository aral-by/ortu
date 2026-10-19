//! Each input byte selects a word from a vocabulary of number words, fillers,
//! interruptions and corrections, so the fuzzer explores the lattice search
//! itself rather than spending its time on words that are not numbers.

#![no_main]

mod invariants;

use libfuzzer_sys::fuzz_target;
use ortu_normalize::{NormalizeConfig, normalize};

const VOCABULARY: &[&str] = &[
    "sıfır", "bir", "iki", "üç", "dört", "beş", "altı", "yedi", "sekiz", "dokuz", "on", "yirmi",
    "otuz", "kırk", "elli", "altmış", "yetmiş", "seksen", "doksan", "yüz", "bin", "milyon",
    "milyar", "uc", "dort", "bes", "kirk", "yuz", "0", "12", "0532", "12'de", "ııı", "şey",
    "bir saniye", "yok yok", "pardon", "hayır", "te", "re", "a", "be", "kapı",
];

fuzz_target!(|data: &[u8]| {
    let words: Vec<&str> = data
        .iter()
        .flat_map(|byte| VOCABULARY[usize::from(*byte) % VOCABULARY.len()].split(' '))
        .collect();
    let turn = normalize(&words, &NormalizeConfig::default());
    invariants::check(&words, &turn);
});
