//! The shipped configuration files parse and match the compiled-in defaults,
//! so the documented example can never drift from the code.

use ortu_core::{FailMode, Mode, Policy, TokenScope, Transform, Weights};

const EXAMPLE_POLICY: &str = include_str!("../../../config/ortu.example.toml");
const WEIGHTS: &str = include_str!("../../../config/weights.toml");

#[test]
fn the_example_policy_parses_to_the_defaults() {
    let policy = Policy::from_toml_str(EXAMPLE_POLICY).unwrap();
    let defaults = Policy::default();
    assert_eq!(policy.mode, Mode::RecallFirst);
    assert!((policy.mask_threshold - defaults.mask_threshold).abs() < f32::EPSILON);
    assert!((policy.review_threshold - defaults.review_threshold).abs() < f32::EPSILON);
    assert_eq!(policy.fail_mode, FailMode::Block);
    assert_eq!(policy.token_scope, TokenScope::Call);
    assert_eq!(
        policy.kinds["credit_card"].transform,
        Some(Transform::Redact)
    );
    assert_eq!(
        policy.kinds["address"].keep_levels,
        ["province", "district"]
    );
}

#[test]
fn the_weights_file_equals_the_compiled_in_defaults() {
    assert_eq!(Weights::from_toml_str(WEIGHTS).unwrap(), Weights::default());
}
