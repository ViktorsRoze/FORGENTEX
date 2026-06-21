use crate::numeric::validate;
use crate::parser::parse_human;

use super::parse_fixture;

fn human(source: &str) -> crate::ast::Program {
    parse_human(source).expect("source should parse")
}

#[test]
fn exact_integer_function_passes() {
    let source = "\
@exact
fn half(x: i32) -> i16:
    return i16((x * 16384) >> 15)
";
    assert!(validate(&human(source)).is_empty());
}

#[test]
fn exact_function_with_float_param_is_rejected() {
    let source = "\
@exact
fn bad(x: f32) -> i16:
    return i16(x)
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(codes.contains(&"exact_arithmetic_uses_float".to_string()));
}

#[test]
fn exact_function_with_float_literal_is_rejected() {
    let source = "\
@exact
fn bad(x: i32) -> i32:
    let scaled: i32 = i32(f32(x) * 0.5)
    return scaled
";
    let codes: Vec<String> = validate(&human(source))
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(codes.contains(&"exact_arithmetic_uses_float".to_string()));
}

#[test]
fn unannotated_float_function_is_allowed_fast_default() {
    // No @exact: floats are fine. Fast-by-default policy.
    let source = "\
fn gain(x: f32) -> f32:
    return x * 0.5
";
    assert!(validate(&human(source)).is_empty());
}

#[test]
fn audio_dsp_exact_functions_check_clean() {
    // apply_gain_q15 / mix_q15 / clamp_i16_from_i32 are @exact and integer.
    let program = parse_fixture("examples/audio/src/dsp.fgx");
    assert!(validate(&program).is_empty());
}
