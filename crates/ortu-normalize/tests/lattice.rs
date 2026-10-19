//! The spoken-number examples from docs/systemdesign.md (section 6.3) and
//! docs/algorithms/02 and 03, as executable tests.
//!
//! Every example of the section 6.3 table has a snapshot of its full lattice,
//! so a change in readings, priors or group boundaries shows up in review.

use ortu_normalize::{NormalizeConfig, NormalizedTurn, normalize};

fn run(text: &str) -> NormalizedTurn {
    let words: Vec<&str> = text.split_whitespace().collect();
    normalize(&words, &NormalizeConfig::default())
}

fn best_readings(text: &str) -> Vec<String> {
    run(text)
        .lattices
        .iter()
        .map(|lattice| lattice.readings[0].digits.clone())
        .collect()
}

/// A stable, human-readable rendering of every lattice of a turn, one line per
/// run and per reading.
fn render(text: &str) -> String {
    let turn = run(text);
    let mut lines = vec![format!("input: {text}")];
    for (index, lattice) in turn.lattices.iter().enumerate() {
        let words = &lattice.words;
        lines.push(format!("run {index}: words {}..{}", words.start, words.end));
        for reading in &lattice.readings {
            let groups: Vec<String> = reading
                .trace
                .groups
                .iter()
                .map(|g| format!("{}..{}", g.start, g.end))
                .collect();
            lines.push(format!(
                "  {:<14} prior {:.3}  splits {}  fallbacks {}  groups [{}]",
                reading.digits,
                reading.prior,
                reading.trace.forced_splits,
                reading.trace.fallback_lookups,
                groups.join(", ")
            ));
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

const SECTION_6_3: [(&str, &str, &str); 6] = [
    (
        "twelve_three_hundred_forty_five",
        "on iki üç yüz kırk beş",
        "12345",
    ),
    ("digit_by_digit", "bir iki üç dört", "1234"),
    (
        "three_groups",
        "beş yüz otuz iki yüz yirmi üç kırk beş",
        "53212345",
    ),
    ("leading_zero", "sıfır beş yüz otuz iki", "0532"),
    ("twenty_three", "yirmi üç", "23"),
    ("year", "iki bin yirmi dört", "2024"),
];

#[test]
fn section_6_3_examples_have_the_documented_best_reading() {
    for (_, input, expected) in SECTION_6_3 {
        assert_eq!(best_readings(input), [expected], "input: {input}");
    }
}

#[test]
fn section_6_3_lattice_snapshots() {
    for (name, input, _) in SECTION_6_3 {
        insta::assert_snapshot!(name, render(input));
    }
}

#[test]
fn checksum_oracle_example_keeps_the_valid_reading_in_the_lattice() {
    // docs/algorithms/03, worked example 2: the synthetic TCKN 10020000126.
    let turn = run("yüz yirmi sıfır sıfır sıfır on iki altı");
    let readings = &turn.lattices[0].readings;

    assert_eq!(readings[0].digits, "120000126");
    let intended = readings
        .iter()
        .find(|r| r.digits == "10020000126")
        .expect("the 11-digit reading must survive pruning");
    assert_eq!(intended.trace.forced_splits, 1);
    assert!((intended.prior - (-1.0f32).exp()).abs() < 1e-6);
}

#[test]
fn fillers_and_interruptions_do_not_break_a_run() {
    let turn = run("on iki üç yüz ııı bir saniye kırk beş");
    assert_eq!(turn.lattices.len(), 1);
    assert_eq!(turn.lattices[0].readings[0].digits, "12345");
    assert_eq!(turn.lattices[0].words, 0..9);
}

#[test]
fn too_many_gap_words_end_the_run() {
    let turn = run("on iki ııı eee şey yani hmm kırk beş");
    assert_eq!(
        best_readings("on iki ııı eee şey yani hmm kırk beş"),
        ["12", "45"]
    );
    assert_eq!(turn.lattices[1].overrides, None);
}

#[test]
fn correction_links_the_corrected_run() {
    let turn = run("beş üç iki yok yok beş üç üç");
    assert_eq!(
        best_readings("beş üç iki yok yok beş üç üç"),
        ["532", "533"]
    );
    assert_eq!(turn.lattices[0].overrides, None);
    assert_eq!(turn.lattices[1].overrides, Some(0));
}

#[test]
fn plain_ascii_stt_output_is_recognized_through_the_fallback() {
    let turn = run("uc dort bes");
    let best = &turn.lattices[0].readings[0];
    assert_eq!(best.digits, "345");
    assert_eq!(best.trace.fallback_lookups, 3);
}

#[test]
fn words_with_turkish_letters_never_use_the_fallback() {
    // `uç` ("tip") and `ön` ("front") are not `üç` and `on`.
    assert_eq!(run("kalemin uç kısmı").lattices, Vec::new());
    assert_eq!(run("ön kapı").lattices, Vec::new());
}

#[test]
fn lone_bir_does_not_multiply_yuz_or_bin() {
    // Turkish says `yüz` (100) and `bin` (1000), never `bir yüz` or `bir bin`;
    // `bir milyon` is correct. Found by the reading-style property test.
    assert_eq!(best_readings("bir yüz"), ["1100"]);
    assert_eq!(best_readings("bir bin"), ["11000"]);
    assert_eq!(best_readings("bir milyon"), ["1000000"]);
    assert_eq!(best_readings("iki yüz"), ["200"]);
    assert_eq!(best_readings("iki bin"), ["2000"]);
}

#[test]
fn digit_tokens_and_suffixed_digits_are_literal_groups() {
    assert_eq!(best_readings("0532 on iki'de"), ["053212"]);
}

#[test]
fn very_long_runs_are_cut_into_bounded_runs() {
    let text = vec!["bir"; 100].join(" ");
    let turn = run(&text);
    let lengths: Vec<usize> = turn
        .lattices
        .iter()
        .map(|l| l.readings[0].digits.len())
        .collect();
    assert_eq!(lengths, [64, 36]);
}
