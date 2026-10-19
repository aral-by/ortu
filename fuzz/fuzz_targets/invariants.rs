//! Invariants shared by the normalization fuzz targets. They mirror the
//! property tests in crates/ortu-normalize/tests/properties.rs.

use ortu_normalize::NormalizedTurn;

pub fn check(words: &[&str], turn: &NormalizedTurn) {
    let word_count = u32::try_from(words.len()).unwrap();
    assert_eq!(turn.words.len(), words.len());
    assert_eq!(turn.roles.len(), words.len());
    for lattice in &turn.lattices {
        assert!(lattice.words.start < lattice.words.end && lattice.words.end <= word_count);
        assert!(!lattice.readings.is_empty() && lattice.readings.len() <= 8);
        for reading in &lattice.readings {
            assert_eq!(reading.digits.len(), reading.source.len());
            assert!(reading.source.iter().all(|w| lattice.words.contains(&w.0)));
            assert!(reading.prior > 0.0 && reading.prior <= 1.0);
        }
    }
    for run in &turn.spelled {
        assert_eq!(run.text.chars().count(), run.source.len());
        assert!(run.words.end <= word_count);
    }
}
