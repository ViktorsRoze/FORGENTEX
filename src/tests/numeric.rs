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

fn codes(source: &str) -> Vec<String> {
    validate(&human(source))
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn exact_is_transitive_through_the_call_graph() {
    // The contract is worth nothing if it stops at the function boundary: this
    // program declared an exact function whose whole body was a call into
    // floating point, and it used to check clean.
    let source = "\
fn approximate(x: i32) -> i32:
    return i32(f64(x) * 0.1)

@exact
fn allegedly_exact(x: i32) -> i32:
    return approximate(x)
";
    let diagnostics = validate(&human(source));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "exact_arithmetic_calls_inexact");
    // The message has to name the callee, or a reader cannot act on it.
    assert!(
        diagnostics[0].message.contains("approximate"),
        "{}",
        diagnostics[0].message
    );

    // The callee itself carries no diagnostic: floating point is the default,
    // and only the @exact claim is being checked.
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| Some(diagnostic.node.as_str()) == Some("@function.approximate")));
}

#[test]
fn a_transitive_report_names_both_the_callee_and_the_root_cause() {
    // Naming only the immediate callee sends a reader to a function that is
    // itself clean-looking, so the chain's far end has to be in the message.
    let source = "\
fn leaf(x: i32) -> i32:
    return i32(f64(x) * 0.5)

fn middle(x: i32) -> i32:
    return leaf(x)

@exact
fn top(x: i32) -> i32:
    return middle(x)
";
    let diagnostics = validate(&human(source));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let message = &diagnostics[0].message;
    assert!(message.contains("middle"), "{message}");
    assert!(message.contains("leaf"), "{message}");
}

#[test]
fn named_types_are_resolved_when_deciding_whether_a_type_is_exact() {
    // A struct that merely holds an f32 is floating point wherever it appears,
    // however deeply it is nested. The check used to see only the type's name.
    let source = "\
struct Sample:
    value: f32

struct Wrapper:
    inner: Sample

@exact
fn takes_wrapper(w: Wrapper) -> i32:
    return 1
";
    assert!(
        codes(source).contains(&"exact_arithmetic_uses_float".to_string()),
        "a nested f32 field must make the parameter type inexact"
    );

    // The same shape with an integer field stays exact, so the rule is about
    // the field types rather than about named types being suspect.
    let integral = "\
struct Sample:
    value: i32

struct Wrapper:
    inner: Sample

@exact
fn takes_wrapper(w: Wrapper) -> i32:
    return 1
";
    assert!(validate(&human(integral)).is_empty());
}

#[test]
fn recursive_call_groups_terminate_and_are_still_checked() {
    // Mutual recursion is the case a depth-first "does this reach a float"
    // query has to special-case; the fixpoint treats it as an ordinary edge.
    let source = "\
@exact
fn ping(n: i32) -> i32:
    if n <= 0:
        return 0
    return pong(n - 1)

fn pong(n: i32) -> i32:
    return ping(n - 1) + i32(0.5)
";
    assert!(codes(source).contains(&"exact_arithmetic_calls_inexact".to_string()));

    // A recursive group with no floating point anywhere must not be reported,
    // or the analysis would merely be refusing to terminate quietly.
    let clean = "\
@exact
fn even(n: i32) -> i32:
    if n <= 0:
        return 1
    return odd(n - 1)

@exact
fn odd(n: i32) -> i32:
    if n <= 0:
        return 0
    return even(n - 1)
";
    assert!(validate(&human(clean)).is_empty());

    // Direct self-recursion, too.
    let looping = "\
@exact
fn countdown(n: i32) -> i32:
    if n <= 0:
        return 0
    return countdown(n - 1)
";
    assert!(validate(&human(looping)).is_empty());
}

#[test]
fn a_lambda_parameter_type_is_part_of_the_signature_it_declares() {
    // The body was checked but the parameters were not, so `|x: f32| x` read as
    // float-free.
    let source = "\
@exact
fn uses_lambda(x: i32) -> i32:
    let f: fn(f32) -> f32 = |v: f32| v
    return x
";
    assert!(codes(source).contains(&"exact_arithmetic_uses_float".to_string()));
}
