use crate::format::format_human;
use crate::interp::run_entry_json;
use crate::parser::parse_human;
use crate::typecheck::check;

const WINDOW: &str = "fn score(xs: &[i32]) -> i32:\n    let middle: &[i32] = xs[1 .. 4]\n    return middle[0] + middle[2]\n";

#[test]
fn slice_range_parses_formats_and_type_checks() {
    let program = parse_human(WINDOW).unwrap();
    let formatted = format_human(&program);
    assert!(formatted.contains("xs[1 .. 4]"), "{formatted}");
    assert!(program.canonical_eq(&parse_human(&formatted).unwrap()));
    let diagnostics = check(&program);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

#[test]
fn slice_range_executes_with_half_open_bounds() {
    let program = parse_human(WINDOW).unwrap();
    let output = run_entry_json(
        &program,
        "score",
        r#"{"args":{"xs":[10,20,30,40,50]},"expect":{"return":60}}"#,
    )
    .unwrap();
    assert!(output.contains("\"status\": \"pass\""), "{output}");
}

#[test]
fn slice_range_traps_on_reversed_or_out_of_bounds_bounds() {
    let program = parse_human(
        "fn bad(xs: &[i32], a: usize, b: usize) -> usize:\n    return xs[a .. b].len\n",
    )
    .unwrap();
    for input in [
        r#"{"args":{"xs":[1,2,3],"a":2,"b":1}}"#,
        r#"{"args":{"xs":[1,2,3],"a":0,"b":4}}"#,
    ] {
        let error = run_entry_json(&program, "bad", input).unwrap_err();
        assert!(error.contains("slice_range_out_of_bounds"), "{error}");
    }
}

#[test]
fn slice_range_bounds_and_base_are_checked() {
    let bad_bound =
        parse_human("fn bad(xs: &[i32]) -> &[i32]:\n    return xs[false .. 2]\n").unwrap();
    assert!(check(&bad_bound)
        .iter()
        .any(|diagnostic| diagnostic.code == "slice_range_bound_not_integer"));

    let bad_base = parse_human("fn bad(x: i32) -> &[i32]:\n    return x[0 .. 1]\n").unwrap();
    assert!(check(&bad_base)
        .iter()
        .any(|diagnostic| diagnostic.code == "slice_range_type"));
}

#[test]
fn constant_slice_ranges_prove_disjointness_but_overlap_is_rejected() {
    let prefix = "fn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\n";
    let disjoint = parse_human(&format!(
        "{prefix}fn ok(xs: &mut [u8]) -> void:\n    fill(xs[0 .. 2], xs[2 .. 4])\n"
    ))
    .unwrap();
    assert!(
        !check(&disjoint)
            .iter()
            .any(|diagnostic| diagnostic.code == "mutable_borrow_alias"),
        "disjoint ranges should not alias"
    );

    let overlapping = parse_human(&format!(
        "{prefix}fn bad(xs: &mut [u8]) -> void:\n    fill(xs[0 .. 3], xs[2 .. 4])\n"
    ))
    .unwrap();
    assert!(check(&overlapping)
        .iter()
        .any(|diagnostic| diagnostic.code == "mutable_borrow_alias"));
}

#[test]
fn dynamic_slice_ranges_remain_conservatively_overlapping() {
    let program = parse_human(
        "fn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\nfn bad(xs: &mut [u8], split: usize) -> void:\n    fill(xs[0 .. split], xs[split .. 4])\n",
    )
    .unwrap();
    assert!(check(&program)
        .iter()
        .any(|diagnostic| diagnostic.code == "mutable_borrow_alias"));
}
