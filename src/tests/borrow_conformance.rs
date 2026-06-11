use std::collections::BTreeSet;

use crate::borrow_conformance::run_borrow_conformance;

#[test]
fn borrow_conformance_suite_covers_v026_categories() {
    let report = run_borrow_conformance();

    assert_eq!(report.schema, "forgentex.borrow_conformance.v1");
    assert_eq!(report.status, "pass");
    assert_eq!(report.failed, 0);
    assert_eq!(report.total, report.passed);
    assert!(report.total >= 25);

    let categories = report
        .cases
        .iter()
        .map(|case| case.category.as_str())
        .collect::<BTreeSet<_>>();
    for category in [
        "moves",
        "borrows",
        "reborrows",
        "loops",
        "branches",
        "lifetimes",
        "projections",
        "contracts",
        "raw",
    ] {
        assert!(categories.contains(category), "missing category {category}");
    }

    for id in [
        "moves.use_after_move",
        "borrows.mutable_call_alias_rejected",
        "borrows.parent_before_child_rejected",
        "loops.loop_carried_alias_rejected",
        "loops.loop_local_alias_ok",
        "branches.conditional_alias_reassign_rejected",
        "branches.conditional_alias_call_rejected",
        "branches.conditional_alias_last_use_ok",
        "lifetimes.conditional_return_ok",
        "lifetimes.unproven_conditional_arg_rejected",
        "projections.disjoint_fields_ok",
        "projections.constant_indices_disjoint_ok",
        "projections.unknown_index_overlap_rejected",
        "contracts.report_wrapper_mutates",
        "raw.raw_pointer_param_safe_fn_rejected",
        "raw.raw_pointer_deref_safe_fn_rejected",
    ] {
        assert!(
            report.cases.iter().any(|case| case.id == id && case.passed),
            "missing passing conformance case {id}"
        );
    }
}

#[test]
fn borrow_conformance_json_is_stable_for_agents() {
    let report = run_borrow_conformance();
    let json = report.to_json();

    assert!(json.contains("\"schema\": \"forgentex.borrow_conformance.v1\""));
    assert!(json.contains("\"status\": \"pass\""));
    assert!(json.contains("\"failed\": 0"));
    assert!(json.contains("\"id\": \"loops.loop_carried_alias_rejected\""));
    assert!(json.contains("\"id\": \"branches.conditional_alias_reassign_rejected\""));
    assert!(json.contains("\"id\": \"lifetimes.conditional_return_ok\""));
    assert!(json.contains("\"id\": \"lifetimes.unproven_conditional_arg_rejected\""));
    assert!(json.contains("\"id\": \"contracts.report_choose_returns\""));
    assert!(json.contains("\"id\": \"raw.raw_pointer_param_safe_fn_rejected\""));
    assert!(json.contains("\"expected_returns\": [\"xs\", \"ys\"]"));
    assert!(json.contains("\"found_mutates\": [\"xs\"]"));

    let text = report.to_text();
    assert!(text.contains("Forgentex borrow conformance suite"));
    assert!(text.contains("status=pass"));
    assert!(text.contains("branches.conditional_alias_call_rejected"));
    assert!(text.contains("contracts.report_wrapper_mutates"));
}
