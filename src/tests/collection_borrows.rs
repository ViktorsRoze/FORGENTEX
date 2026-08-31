use crate::lifetimes::analyze_contracts;
use crate::parser::parse_human;
use crate::typecheck::check;

fn diagnostics(source: &str) -> Vec<crate::diag::Diagnostic> {
    check(&parse_human(source).unwrap())
}

#[test]
fn vec_and_array_indexing_preserve_borrow_provenance() {
    for source in [
        "fn first(values: Vec[&[u8]]) -> &[u8]:\n    return values[0]\n",
        "fn first(values: [&[u8]; 2]) -> &[u8]:\n    return values[1]\n",
        "fn first(values: Vec[Vec[&[u8]]]) -> &[u8]:\n    return values[0][1]\n",
    ] {
        let found = diagnostics(source);
        assert!(found.is_empty(), "{source}\n{found:?}");
    }
}

#[test]
fn collection_borrow_contract_points_to_the_container_parameter() {
    let program = parse_human(
        "fn first(values: Vec[Vec[&[u8]]]) -> &[u8]:\n    let row: Vec[&[u8]] = values[0]\n    let item: &[u8] = row[1]\n    return item\n",
    )
    .unwrap();
    let diagnostics = check(&program);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let contract = analyze_contracts(&program)
        .into_iter()
        .find(|contract| contract.function == "@function.first")
        .unwrap();
    assert_eq!(
        contract
            .returns
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["values"]
    );
}

#[test]
fn generic_identity_and_wrapper_preserve_borrow_provenance() {
    let program = parse_human(
        "fn identity[T](value: T) -> T:\n    return value\n\nfn forward[T](value: T) -> T:\n    return identity(value)\n\nfn keep(xs: &[u8]) -> &[u8]:\n    return forward(xs)\n",
    )
    .unwrap();
    let diagnostics = check(&program);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    for (function, expected) in [
        ("@function.identity", "value"),
        ("@function.forward", "value"),
        ("@function.keep", "xs"),
    ] {
        let contracts = analyze_contracts(&program);
        let contract = contracts
            .iter()
            .find(|contract| contract.function == function)
            .unwrap();
        assert_eq!(contract.returns.len(), 1, "{function}: {contract:?}");
        assert_eq!(contract.returns[0].name, expected);
    }
}

#[test]
fn generic_conditional_return_tracks_both_borrow_arguments() {
    let program = parse_human(
        "fn choose[T](flag: bool, left: T, right: T) -> T:\n    if flag:\n        return left\n    return right\n\nfn keep(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:\n    return choose(flag, xs, ys)\n",
    )
    .unwrap();
    let diagnostics = check(&program);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let contract = analyze_contracts(&program)
        .into_iter()
        .find(|contract| contract.function == "@function.keep")
        .unwrap();
    assert_eq!(
        contract
            .returns
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        vec!["xs", "ys"]
    );
}

#[test]
fn nested_vec_borrow_places_distinguish_constant_indices() {
    let prefix = "fn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\n";
    let disjoint = format!(
        "{prefix}fn ok(values: Vec[Vec[&mut [u8]]]) -> void:\n    fill(values[0][0], values[0][1])\n"
    );
    let found = diagnostics(&disjoint);
    assert!(found.is_empty(), "{found:?}");

    let overlapping = format!(
        "{prefix}fn bad(values: Vec[Vec[&mut [u8]]]) -> void:\n    fill(values[0][1], values[0][1])\n"
    );
    assert!(diagnostics(&overlapping)
        .iter()
        .any(|diagnostic| diagnostic.code == "mutable_borrow_alias"));
}

#[test]
fn nested_vec_dynamic_indices_remain_conservatively_overlapping() {
    let source = "fn fill(left: &mut [u8], right: &mut [u8]) -> void:\n    return\n\nfn bad(values: Vec[Vec[&mut [u8]]], i: usize, j: usize) -> void:\n    fill(values[0][i], values[0][j])\n";
    assert!(diagnostics(source)
        .iter()
        .any(|diagnostic| diagnostic.code == "mutable_borrow_alias"));
}

#[test]
fn generic_mutable_borrow_parameters_use_their_concrete_call_types() {
    let generic = "fn choose[T](left: T, right: T) -> T:\n    return left\n\n";
    let distinct = format!(
        "{generic}fn ok(xs: &mut [u8], ys: &mut [u8]) -> &mut [u8]:\n    return choose(xs, ys)\n"
    );
    let found = diagnostics(&distinct);
    assert!(found.is_empty(), "{found:?}");

    let overlapping =
        format!("{generic}fn bad(xs: &mut [u8]) -> &mut [u8]:\n    return choose(xs, xs)\n");
    assert!(diagnostics(&overlapping)
        .iter()
        .any(|diagnostic| diagnostic.code == "mutable_borrow_alias"));
}

#[test]
fn generic_wrapper_parameters_preserve_concrete_mutable_alias_checks() {
    let source = "fn choose[T](left: T, right: T) -> T:\n    return left\n\nfn forward[T](left: T, right: T) -> T:\n    return choose(left, right)\n\nfn bad(xs: &mut [u8]) -> &mut [u8]:\n    return forward(xs, xs)\n";
    assert!(diagnostics(source)
        .iter()
        .any(|diagnostic| diagnostic.code == "mutable_borrow_alias"));
}
