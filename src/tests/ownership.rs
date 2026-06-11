use std::path::Path;

use crate::diag::{diagnostics_to_json, Diagnostic};
use crate::parser::parse_file;
use crate::typecheck::check;

fn diagnostics(source: &str) -> Vec<Diagnostic> {
    let program = parse_file(Path::new("ownership.fgx"), source).unwrap();
    check(&program)
}

fn has_code(diagnostics: &[Diagnostic], code: &str) -> bool {
    diagnostics.iter().any(|diagnostic| diagnostic.code == code)
}

#[test]
fn owned_struct_values_cannot_be_used_after_move() {
    let source = "\
struct Packet:
    value: u8

fn bad(packet: Packet) -> u8:
    let moved: Packet = packet
    return packet.value
";
    let diagnostics = diagnostics(source);
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"use_after_move\""));
    assert!(json.contains("\"expected\": \"Packet\""));
    assert!(json.contains("\"found\": \"moved_value\""));
    assert!(json.contains("pass_by_reference"));
}

#[test]
fn mutable_borrow_call_arguments_must_not_alias() {
    let source = "\
fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(xs: &mut [u8]) -> void:
    fill(xs, xs)
";
    let diagnostics = diagnostics(source);
    let json = diagnostics_to_json(&diagnostics);

    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("\"expected\": \"unique_mutable_borrow\""));
    assert!(json.contains("\"found\": \"aliased_borrow\""));
    assert!(json.contains("avoid_aliasing_mutable_argument"));
}

#[test]
fn local_borrow_aliases_resolve_to_original_roots() {
    let source = "\
fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    fill(xs, alias)
";
    let diagnostics = diagnostics(source);
    let json = diagnostics_to_json(&diagnostics);

    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("call to 'fill' aliases mutable borrow of 'xs'"));
}

#[test]
fn live_mutable_borrow_aliases_block_root_writes() {
    let future_alias_use = "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    xs[0] = 1
    alias[0] = 2
";
    let future_alias_diagnostics = diagnostics(future_alias_use);
    let json = diagnostics_to_json(&future_alias_diagnostics);

    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("live borrow alias 'alias' of 'xs'"));
    assert!(json.contains("\"found\": \"live_borrow_alias\""));

    let same_statement_alias_use = "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    xs[0] = alias[0]
";
    let json = diagnostics_to_json(&diagnostics(same_statement_alias_use));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );

    let alias_write = "\
fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    alias[0] = 1
";
    assert_eq!(diagnostics(alias_write), Vec::new());

    let reassigned_alias = "\
fn ok(xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    alias = ys
    xs[0] = 1
";
    assert_eq!(diagnostics(reassigned_alias), Vec::new());

    let root_write_after_last_alias_use = "\
fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    alias[0] = 1
    xs[0] = 2
";
    assert_eq!(diagnostics(root_write_after_last_alias_use), Vec::new());

    let unused_alias = "\
fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    xs[0] = 1
";
    assert_eq!(diagnostics(unused_alias), Vec::new());
}

#[test]
fn loop_carried_borrow_aliases_remain_live_across_iterations() {
    let outer_alias = "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    for i in 0 .. 2:
        alias[0] = 1
        xs[0] = 2
";
    let json = diagnostics_to_json(&diagnostics(outer_alias));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("live borrow alias 'alias' of 'xs'"));

    let loop_local_alias = "\
fn ok(xs: &mut [u8]) -> void:
    for i in 0 .. 2:
        let alias: &mut [u8] = xs
        alias[0] = 1
        xs[0] = 2
";
    assert_eq!(diagnostics(loop_local_alias), Vec::new());
}

#[test]
fn conditional_borrow_reassignments_join_possible_alias_roots() {
    let future_alias_use = "\
fn bad(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    xs[0] = 1
    alias[0] = 2
";
    let json = diagnostics_to_json(&diagnostics(future_alias_use));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("live borrow alias 'alias' of 'ys|xs'"));

    let maybe_alias_call = "\
fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    fill(xs, alias)
";
    let json = diagnostics_to_json(&diagnostics(maybe_alias_call));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("call to 'fill' aliases mutable borrow of 'ys|xs'"));

    let alias_last_use_before_root_write = "\
fn ok(flag: bool, xs: &mut [u8], ys: &mut [u8]) -> void:
    let alias: &mut [u8] = ys
    if flag:
        alias = xs
    alias[0] = 1
    xs[0] = 2
";
    assert_eq!(diagnostics(alias_last_use_before_root_write), Vec::new());
}

#[test]
fn mutable_reborrows_form_parent_child_liveness() {
    let child_then_parent = "\
fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    child[0] = 1
    alias[0] = 2
";
    assert_eq!(diagnostics(child_then_parent), Vec::new());

    let parent_before_child_last_use = "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    alias[0] = 1
    child[0] = 2
";
    let json = diagnostics_to_json(&diagnostics(parent_before_child_last_use));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("live borrow alias 'child' of 'xs'"));

    let same_statement_parent_use = "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    child[0] = alias[0]
";
    let json = diagnostics_to_json(&diagnostics(same_statement_parent_use));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );

    let sibling_reborrows = "\
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let left: &mut [u8] = alias
    let right: &mut [u8] = alias
    left[0] = 1
    right[0] = 2
";
    let json = diagnostics_to_json(&diagnostics(sibling_reborrows));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("live borrow alias 'right' of 'xs'"));
}

#[test]
fn mutable_call_activation_respects_reborrow_liveness() {
    let nested_helper = "\
fn first(xs: &[u8]) -> u8:
    return xs[0]

fn set_first(xs: &mut [u8], value: u8) -> void:
    xs[0] = value

fn ok(xs: &mut [u8]) -> void:
    set_first(xs, first(xs))
";
    assert_eq!(diagnostics(nested_helper), Vec::new());

    let child_then_parent_call = "\
fn fill(xs: &mut [u8]) -> void:
    xs[0] = 1

fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    child[0] = 1
    fill(alias)
";
    assert_eq!(diagnostics(child_then_parent_call), Vec::new());

    let parent_call_before_child_last_use = "\
fn fill(xs: &mut [u8]) -> void:
    xs[0] = 1

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    fill(alias)
    child[0] = 1
";
    let json = diagnostics_to_json(&diagnostics(parent_call_before_child_last_use));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("mutable call argument for 'fill' 'alias'"));
}

#[test]
fn projection_alias_analysis_allows_disjoint_field_borrows() {
    let source = "\
struct Pair:
    left: &mut [u8]
    right: &mut [u8]

fn ok(pair: Pair) -> void:
    let left: &mut [u8] = pair.left
    let right: &mut [u8] = pair.right
    left[0] = 1
    right[0] = 2
";
    assert_eq!(diagnostics(source), Vec::new());
}

#[test]
fn projection_alias_analysis_rejects_overlapping_field_borrows() {
    let source = "\
struct Pair:
    left: &mut [u8]
    right: &mut [u8]

fn bad(pair: Pair) -> void:
    let left: &mut [u8] = pair.left
    let also_left: &mut [u8] = pair.left
    left[0] = 1
    also_left[0] = 2
";
    let json = diagnostics_to_json(&diagnostics(source));

    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("live borrow alias 'also_left' of 'pair.left'"));
}

#[test]
fn projection_alias_analysis_checks_call_arguments() {
    let disjoint = "\
struct Pair:
    left: &mut [u8]
    right: &mut [u8]

fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn ok(pair: Pair) -> void:
    fill(pair.left, pair.right)
";
    assert_eq!(diagnostics(disjoint), Vec::new());

    let overlapping = "\
struct Pair:
    left: &mut [u8]
    right: &mut [u8]

fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(pair: Pair) -> void:
    fill(pair.left, pair.left)
";
    let json = diagnostics_to_json(&diagnostics(overlapping));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("call to 'fill' aliases mutable borrow of 'pair.left'"));
}

#[test]
fn moves_inside_conditionals_and_loops_affect_later_reads() {
    let conditional = "\
struct Packet:
    value: u8

fn bad(packet: Packet, flag: bool) -> u8:
    if flag:
        let moved: Packet = packet
    return packet.value
";
    let conditional_diagnostics = diagnostics(conditional);
    assert!(has_code(&conditional_diagnostics, "use_after_move"));

    let looped = "\
struct Packet:
    value: u8

fn bad(packet: Packet) -> u8:
    for i in 0 .. 1:
        let moved: Packet = packet
    return packet.value
";
    let loop_diagnostics = diagnostics(looped);
    assert!(has_code(&loop_diagnostics, "use_after_move"));
}

#[test]
fn field_sensitive_move_paths_preserve_unmoved_siblings() {
    let ok = "\
struct Packet:
    value: u8

struct Pair:
    left: Packet
    right: Packet

fn ok(pair: Pair) -> u8:
    let left: Packet = pair.left
    return pair.right.value
";
    assert_eq!(diagnostics(ok), Vec::new());

    let repeated_field = "\
struct Packet:
    value: u8

struct Pair:
    left: Packet
    right: Packet

fn bad(pair: Pair) -> u8:
    let left: Packet = pair.left
    return pair.left.value
";
    let repeated = diagnostics(repeated_field);
    let repeated_json = diagnostics_to_json(&repeated);
    assert!(repeated_json.contains("\"error\": \"use_after_move\""));
    assert!(repeated_json.contains("pair.left"));
    assert!(repeated_json.contains("\"expected\": \"Packet\""));

    let whole_after_field = "\
struct Packet:
    value: u8

struct Pair:
    left: Packet
    right: Packet

fn bad(pair: Pair) -> Pair:
    let left: Packet = pair.left
    return pair
";
    let whole = diagnostics(whole_after_field);
    let whole_json = diagnostics_to_json(&whole);
    assert!(whole_json.contains("\"error\": \"use_after_move\""));
    assert!(whole_json.contains("field 'left' was moved"));
    assert!(whole_json.contains("\"found\": \"partially_moved_value\""));

    let reinitialized = "\
struct Packet:
    value: u8

struct Pair:
    left: Packet
    right: Packet

fn ok(pair: Pair) -> Pair:
    let left: Packet = pair.left
    pair.left = left
    return pair
";
    assert_eq!(diagnostics(reinitialized), Vec::new());
}

#[test]
fn safe_code_still_cannot_mutate_through_immutable_slices() {
    let source = "\
fn bad(xs: &[u8]) -> void:
    xs[0] = 1
";
    let diagnostics = diagnostics(source);

    assert!(has_code(&diagnostics, "immutable_write"));
}

#[test]
fn reference_return_must_be_tied_to_a_parameter_lifetime() {
    let ok = diagnostics("fn same(xs: &[u8]) -> &[u8]:\n    return xs\n");
    assert!(
        !has_code(&ok, "escaping_reference"),
        "returning a parameter borrow is inferable in v0"
    );

    let interprocedural = "\
fn same(xs: &[u8]) -> &[u8]:
    return xs

fn also_same(xs: &[u8]) -> &[u8]:
    return same(xs)
";
    assert!(
        !has_code(&diagnostics(interprocedural), "escaping_reference"),
        "direct borrow-return provenance should flow through calls"
    );

    let alias = "\
fn same(xs: &[u8]) -> &[u8]:
    let alias: &[u8] = xs
    return alias

fn also_same(xs: &[u8]) -> &[u8]:
    let local: &[u8] = same(xs)
    return local
";
    assert!(
        !has_code(&diagnostics(alias), "escaping_reference"),
        "local aliases of parameter-origin borrow returns stay parameter-origin"
    );

    let conditional = "\
fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    if flag:
        return xs
    return ys

fn caller(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    return choose(flag, xs, ys)
";
    assert!(
        !has_code(&diagnostics(conditional), "escaping_reference"),
        "conditional borrow-return summaries can preserve multiple parameter origins"
    );

    let unproven_argument = "\
fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    if flag:
        return xs
    return ys

fn bad(flag: bool, xs: &[u8]) -> &[u8]:
    let local: &[u8] = unknown_borrow(xs)
    return choose(flag, xs, local)
";
    assert!(
        has_code(&diagnostics(unproven_argument), "escaping_reference"),
        "every possible returned borrow argument must be parameter-origin"
    );

    let source = "\
fn bad(xs: &[u8]) -> &[u8]:
    return unknown_borrow(xs)
";
    let diagnostics = diagnostics(source);
    let json = diagnostics_to_json(&diagnostics);

    assert!(json.contains("\"error\": \"escaping_reference\""));
    assert!(json.contains("\"expected\": \"&[u8]\""));
    assert!(json.contains("return_owned_value"));
}

#[test]
fn interprocedural_lifetime_contracts_distinguish_mutating_calls() {
    let read_only_helper = "\
fn view(xs: &mut [u8]) -> u8:
    return xs[0]

fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    let value: u8 = view(alias)
    child[0] = value
";
    assert_eq!(diagnostics(read_only_helper), Vec::new());

    let mutating_helper = "\
fn write(xs: &mut [u8]) -> void:
    xs[0] = 1

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    write(alias)
    child[0] = 2
";
    let json = diagnostics_to_json(&diagnostics(mutating_helper));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("mutable call argument for 'write' 'alias'"));
}

#[test]
fn interprocedural_lifetime_contracts_propagate_mutating_callees() {
    let source = "\
fn write(xs: &mut [u8]) -> void:
    xs[0] = 1

fn wrapper(xs: &mut [u8]) -> void:
    write(xs)

fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    let child: &mut [u8] = alias
    wrapper(alias)
    child[0] = 2
";
    let json = diagnostics_to_json(&diagnostics(source));
    assert!(
        json.contains("\"error\": \"mutable_borrow_alias\""),
        "{json}"
    );
    assert!(json.contains("mutable call argument for 'wrapper' 'alias'"));
}

#[test]
fn raw_pointer_indexing_requires_unsafe_functions() {
    let safe = diagnostics("fn bad(ptr: *const u8) -> u8:\n    return ptr[0]\n");
    let json = diagnostics_to_json(&safe);

    assert!(json.contains("\"error\": \"raw_pointer_dereference_requires_unsafe\""));
    assert!(json.contains("\"error\": \"raw_pointer_requires_unsafe\""));
    assert!(json.contains("\"found\": \"*constu8\""));
    assert!(json.contains("replace_pointer_with_slice"));

    let unsafe_source = "unsafe fn ok(ptr: *const u8) -> u8:\n    return ptr[0]\n";
    assert_eq!(diagnostics(unsafe_source), Vec::new());
}

#[test]
fn raw_pointer_parameters_require_unsafe_functions() {
    let safe = diagnostics("fn bad(ptr: *const u8) -> void:\n    return\n");
    let json = diagnostics_to_json(&safe);

    assert!(json.contains("\"error\": \"raw_pointer_requires_unsafe\""));
    assert!(json.contains("raw pointer parameter 'ptr' requires unsafe code"));

    let unsafe_source = "unsafe fn ok(ptr: *const u8) -> void:\n    return\n";
    assert_eq!(diagnostics(unsafe_source), Vec::new());
}
